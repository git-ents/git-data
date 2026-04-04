# eä — Revised Design: Abstract Transaction Kernel

**Project:** `ea` **Organization:** `git-ainur` **Status:** Draft — revised design document **Author:** Joey Carpinelli **Date:** April 2026 **Supersedes:** [ea-design-doc.md](ea-design-doc.md) (retained as background reading)

---

## 0. Relationship to Prior Document

The original design document described `ea` as a concrete library implementing composable typed primitives over git's object store.
Through iterative design refinement, the scope has narrowed and the abstraction level has risen.
`ea` is not a git library.
It is the set of abstract type definitions and algebraic laws that any content-addressed transactional storage backend must satisfy.
Git is one valid backend.
Prolly trees are another.
The original document remains useful as a description of what a git-specific backend (`ea-gix`) would look like, and its analysis of performance characteristics, scaling limits, and merge strategies still applies to that backend.

This document describes the kernel itself.

## 1. Thesis (Revised)

Every content-addressed transactional storage system reinvents the same core abstractions: a content-addressed store, a named mutable pointer, an append-only log, a keyed mutable map, and a transaction protocol that composes them.
These abstractions are independent of the backing store.
`ea` extracts them as a minimal set of Rust trait definitions with algebraic laws, enabling backend authors to provide implementations and application authors to write backend-agnostic logic over composable primitives.

The library contains no I/O, no storage backend, no serialization format.
It is a contract.

## 2. Foundation Traits

Two foundation traits define the storage interface.
All higher-level primitives are expressed in terms of these.

### 2.1 ContentAddressable

A store that maps values to deterministic hashes and retrieves values by hash.

```text
trait ContentAddressable {
    type Hash: Eq + Clone;
    type Value;

    fn store(&self, value: &Self::Value) -> Self::Hash;
    fn retrieve(&self, hash: &Self::Hash) -> Option<Self::Value>;
    fn contains(&self, hash: &Self::Hash) -> bool;
}
```

**Laws:**

- **Determinism.** `store(v)` always returns the same hash for the same value.
- **Round-trip.** `retrieve(store(v))` returns `Some(v')` where `v' == v`.
- **Referential transparency.**
  Two values with the same hash are semantically identical.

These laws are testable via property-based testing against any backend implementation.

### 2.2 Pointer

A named mutable reference to a hash, supporting atomic compare-and-swap.

```text
trait Pointer {
    type Hash: Eq + Clone;

    fn read(&self) -> Option<Self::Hash>;
    fn cas(&self, expected: Option<Self::Hash>, new: Self::Hash) -> Result<(), CasFailure>;
}
```

**Laws:**

- **Atomicity.**
  A CAS either fully succeeds or fully fails; no intermediate state is observable.
- **Linearizability.**
  Concurrent CAS operations on the same pointer are totally ordered.
- **Consistency.**
  After a successful `cas(old, new)`, `read()` returns `new` (absent further writes).

## 3. Primitives

Two primitives are defined over the foundation traits.
All other data structures in the original design document (metadata, source, ephemeral) are expressible as one of these two with policy annotations.

### 3.1 Chain

An ordered, append-only log of entries.
Each entry is content-addressed.
The chain itself is a content-addressed value: a list of entry hashes (or a linked structure where each entry references its predecessor).

```text
trait Chain {
    type Store: ContentAddressable;
    type Entry;

    fn head(&self) -> Option<<Self::Store as ContentAddressable>::Hash>;
    fn append(&self, entry: Self::Entry) -> <Self::Store as ContentAddressable>::Hash;
    fn entries(&self) -> impl Iterator<Item = Self::Entry>;
}
```

**Structural identity:** A chain is a ledger where keys are monotonically assigned positions and only the next position is writable.
This is a restriction, not a separate concept.
It is kept as a distinct trait because its merge semantics differ fundamentally from a ledger's.

**Merge semantics:** Forked chains are merged by causal interleaving (topological sort on entry metadata: Lamport timestamp, vector clock, or structural parent pointer).
Duplicate entries (same content hash) are collapsed.

### 3.2 Ledger

A keyed, mutable map from names to content-addressed values.
Each version of the ledger is itself a content-addressed value (a snapshot of all key-value pairs as a tree or map structure).

```text
trait Ledger {
    type Store: ContentAddressable;
    type Key;
    type Value;

    fn get(&self, key: &Self::Key) -> Option<Self::Value>;
    fn put(&self, key: Self::Key, value: Self::Value) -> <Self::Store as ContentAddressable>::Hash;
    fn delete(&self, key: &Self::Key) -> <Self::Store as ContentAddressable>::Hash;
    fn entries(&self) -> impl Iterator<Item = (Self::Key, Self::Value)>;
}
```

**Merge semantics:** Forked ledgers are merged key-by-key.
Distinct-key mutations auto-merge.
Same-key mutations are conflicts, resolved by a pluggable policy (see Section 5).

### 3.3 Recursive Composition

Chain and ledger values can themselves be chains or ledgers.
A chain entry can be a full ledger snapshot.
A ledger value can be a chain head hash.
Composition recurses to arbitrary depth; the base case is any content-addressed value that is not itself a primitive (opaque blobs, application-defined types).

A type marker — a small tag on each node — tells the merge dispatcher which strategy to apply.
The tag type is abstract; its concrete representation is backend-defined.

### 3.4 Policy Annotations

The original document's five primitive types reduce to two primitives plus annotations:

| Original primitive | Realization |
|---|---|
| Chain | `Chain` |
| Ledger | `Ledger` |
| Metadata | `Ledger` + `derived` (rebuildable, never authoritative) |
| Source | `Ledger` + `write-once` (key is content hash, no overwrite) |
| Ephemeral | `Chain` or `Ledger` + `local-only` (excluded from sync) |

Annotations are metadata on the primitive instance, not separate types.
The merge dispatcher reads annotations to adjust strategy (e.g., skip merging for `local-only`, rebuild rather than merge for `derived`).

## 4. Transaction Protocol

A transaction composes reads and writes across any number of primitives, committed atomically via a single pointer CAS.

```text
trait Transaction {
    type Store: ContentAddressable;
    type Ptr: Pointer<Hash = <Self::Store as ContentAddressable>::Hash>;

    fn begin(&self) -> Snapshot;
    fn read(&self, snapshot: &Snapshot, path: &Path) -> Option<Value>;
    fn write(&mut self, path: &Path, value: Value);
    fn commit(&self) -> Result<(), CasFailure>;
}
```

The protocol is:

1. **Begin.**
   Read the pointer to obtain the current root hash.
   Resolve to a snapshot.
2. **Read.**
   Traverse the content-addressed structure from the root to read values.
3. **Write.**
   Accumulate mutations in memory.
4. **Commit.**
   Compute a new root hash incorporating all mutations.
   CAS the pointer from old root to new root.

On CAS failure (concurrent writer advanced the pointer), the caller retries from step 1.
This is optimistic concurrency with no locks held during computation.

**Single-pointer property.**
The entire state tree is rooted at one pointer.
One CAS per transaction, regardless of how many primitives are modified.
This was identified in the original document as the key insight that collapses the ref-scalability concern.

## 5. Merge Contract

When two forks of the state tree must be reconciled, the merge dispatcher walks both trees, reads the type marker at each node, and delegates to the appropriate strategy.

```text
trait MergeStrategy<P> {
    fn merge(base: &P, left: &P, right: &P) -> MergeResult<P>;
}
```

Where `MergeResult` is one of: `Resolved(P)`, `Conflict(ConflictRecord)`.

Built-in strategies for the two primitives:

- **Chain:** causal interleave.
  Entries from both forks are combined in causal order.
  Configurable: if causal order is indeterminate, break ties by hash (deterministic) or timestamp (non-deterministic but human-friendly).
- **Ledger:** key-by-key three-way merge.
  Auto-merge for disjoint keys.
  Configurable conflict policy for same-key mutations: last-writer-wins (requires a clock), both-sides-preserved (produce a conflict ledger), or caller-provided function.

Applications can provide custom `MergeStrategy` implementations for domain-specific types.
The dispatch mechanism is:

1. Read the type marker at the current node.
2. Look up the registered `MergeStrategy` for that marker.
3. If found, delegate.
   If not found, fall back to a default byte-level three-way merge or report an unresolvable conflict.

## 6. What the Crate Contains

```text
ea/
  src/
    lib.rs              — Re-exports
    content.rs          — ContentAddressable trait + laws
    pointer.rs          — Pointer trait + laws
    chain.rs            — Chain trait + merge strategy trait
    ledger.rs           — Ledger trait + merge strategy trait
    transaction.rs      — Transaction protocol trait
    merge.rs            — MergeStrategy trait + dispatcher contract
    annotations.rs      — Policy annotation types
    composition.rs      — Recursive type marker + traversal
```

Estimated size: 500–1500 lines of Rust.
No dependencies beyond `core`/`alloc`.
The crate is `no_std` compatible.

## 7. What the Crate Does Not Contain

- **No I/O.**
  No file system, no network, no git operations.
- **No serialization.**
  No wire format, no on-disk layout.
  Backends choose their own.
- **No storage backend.**
  Git, prolly trees, SQLite, in-memory — all are external crates.
- **No query language.**
  Read-path indexing and queries are consumer concerns.
- **No schema enforcement.**
  Type checking beyond the primitive markers is the application's responsibility.
- **No clock implementation.**
  Lamport timestamps, vector clocks, and wall clocks are injected by the application or backend.

## 8. Backend Implementor's Contract

A valid `ea` backend must:

1. Implement `ContentAddressable` satisfying the three laws (determinism, round-trip, referential transparency).
2. Implement `Pointer` satisfying atomicity, linearizability, and consistency.
3. Provide concrete representations for chain entries, ledger snapshots, type markers, and the root state tree.
4. Pass the `ea` property test suite, which verifies the laws and protocol invariants against the concrete implementation.

A backend is free to make any performance trade-offs (tree format, chunking strategy, compression, caching) as long as the laws hold.

### 8.1 Expected Backends

| Crate | Backend | Notes |
|---|---|---|
| `ea-gix` | Git via gitoxide | Trees as git tree objects, chains as commit chains, pointer as ref. See original design document for detailed architecture. |
| `ea-prolly` | Prolly trees | Better scaling for large flat collections. Content-defined chunking. |
| `ea-mem` | In-memory HashMap | For testing and ephemeral workloads. |
| `ea-sqlite` | SQLite | Content-addressed rows. Pointer as a row in a metadata table with `UPDATE ... WHERE hash = ?` for CAS. |

## 9. Relationship to Existing Work

### 9.1 Irmin

Irmin is the closest existing project.
It provides a content-addressable, mergeable, branchable store with customizable types and backends.
Irmin differs from `ea` in that it is a concrete library (OCaml, with built-in backends for git, filesystem, and in-memory), not an abstract contract.
Its API surface is substantially larger, and its merge system is defined in terms of OCaml functors, making it difficult to port to other languages.
`ea` can be understood as "the abstract kernel that Irmin implements, extracted as a language-agnostic set of trait definitions."

### 9.2 Noms / Dolt

Noms pioneered content-addressed versioned databases with prolly trees.
Dolt commercialized the approach with a SQL interface.
Both are complete database systems with custom storage engines.
`ea` operates one layer below: it defines the traits that a system like Noms would implement, without prescribing the storage format or query interface.

### 9.3 Merkle-CRDTs

The Merkle-CRDT literature (Protocol Labs, 2020) formalizes embedding CRDT payloads in Merkle DAG nodes.
`ea`'s merge contract is compatible with this: a `MergeStrategy` implementation can encode CRDT semantics.
The difference is that Merkle-CRDTs are defined over IPFS-style DAGs with content identifiers (CIDs), while `ea` is backend-agnostic and does not require IPFS or any specific DAG format.

### 9.4 Local-First Software

The local-first movement (Kleppmann et al., Ink & Switch) has produced significant research on offline-capable, user-owned data with automatic conflict resolution.
The community is currently fragmented across incompatible implementations (Automerge, Yjs, cr-sqlite, Electric SQL, Zero).
`ea` does not directly compete with these — it operates at the storage kernel level beneath them.
A local-first framework could use an `ea` backend as its persistence and sync layer, gaining content-addressed deduplication, Merkle-verifiable history, and backend portability.

## 10. Risks

### 10.1 Trait Expressiveness

The two-primitive model (chain + ledger) is a bet that all useful content-addressed data structures can be expressed as compositions of append-only logs and mutable keyed maps.
If an application requires a primitive that does not decompose into these two (e.g., a graph with bidirectional edges as a first-class concept), the trait set must be extended.
Mitigation: the trait system is open for extension.
New primitive traits can be added without breaking existing backends, as long as they are expressed in terms of `ContentAddressable` and `Pointer`.

### 10.2 Merge Strategy Completeness

The built-in merge strategies (causal interleave for chains, key-by-key for ledgers) may not be sufficient for all domain-specific merge needs.
The custom `MergeStrategy` escape hatch addresses this, but shifts complexity to the application author.
If most applications need custom strategies, the built-in ones provide little value and the library is "just traits."

### 10.3 Adoption Without a Flagship Backend

A crate of abstract traits with no concrete backend is difficult to evaluate.
Developers cannot `cargo run` anything.
The `ea-gix` (git) and `ea-mem` (in-memory) backends must ship simultaneously with the core crate, or adoption will not happen.

### 10.4 Premature Abstraction

The trait definitions are derived from a single design trajectory (the git-mirdain ecosystem).
They have not been validated against diverse backend implementations or application domains.
The risk is that the abstractions encode assumptions specific to git's object model that do not generalize.
Mitigation: implement at least two structurally different backends (`ea-gix` and `ea-prolly` or `ea-sqlite`) before stabilizing the trait definitions.

### 10.5 Performance Opacity

Abstract traits hide performance characteristics.
A transaction that is O(log N) on a git backend might be O(N) on a naive in-memory backend.
Application authors writing backend-agnostic code cannot reason about performance without knowing the backend.
This is a fundamental tension in any abstraction library and has no clean resolution beyond good documentation of backend-specific performance profiles.

## 11. Development Plan (Revised)

### Phase 0: Trait Definitions (1–2 weeks)

Define `ContentAddressable`, `Pointer`, `Chain`, `Ledger`, `Transaction`, `MergeStrategy`, and `PolicyAnnotation` as Rust traits with associated types.
Write the algebraic laws as doc comments and as property test generators (using `proptest` or `quickcheck`) that any backend can run to verify compliance.
Ship as `ea` crate, `no_std`, zero dependencies.

### Phase 1: In-Memory Backend (1 week)

Implement `ea-mem`: `ContentAddressable` as `HashMap<Hash, Value>`, `Pointer` as `AtomicPtr` or `Mutex<Option<Hash>>`.
This backend exists for testing and for library authors to develop against without I/O overhead.
Ship simultaneously with Phase 0.

### Phase 2: Git Backend (3–4 weeks)

Implement `ea-gix`: `ContentAddressable` as gix object store, `Pointer` as gix ref, `Chain` as git commit chains, `Ledger` as git tree objects.
Transaction as read-tree → mutate-in-memory → write-tree → commit → CAS ref.
This validates that the trait definitions are expressive enough for a real content-addressed backend with structural sharing.
See original design document for detailed architecture.

### Phase 3: Merge Implementation (3–5 weeks)

Implement the merge dispatcher and built-in strategies for chain and ledger.
Property test: generate random forked state trees, merge, verify that all auto-mergeable cases produce correct results and all real conflicts are detected.
Fuzz test: random concurrent transaction sequences against `ea-mem`, verify no invariant violations.

### Phase 4: Second Backend (2–3 weeks)

Implement `ea-prolly` or `ea-sqlite`.
This is the critical validation step: if the trait definitions require modification to accommodate a structurally different backend, make those modifications before stabilizing.
If they accommodate the second backend without changes, confidence in the abstraction increases.

### Phase 5: Documentation and Stabilization (1–2 weeks)

Crate-level documentation.
Tutorial: "Implement a backend for `ea` in 100 lines."
Tutorial: "Build a versioned key-value store on `ea-gix`."
Specification: formal statement of laws and protocol invariants.
Stability policy for 0.1 release.

---

**Total estimated timeline: 11–17 weeks to 0.1 release of `ea` + `ea-mem` + `ea-gix` + one additional backend.**

The critical path is Phase 3 (merge) and Phase 4 (second backend validation).
Everything else is straightforward trait definition and plumbing.
