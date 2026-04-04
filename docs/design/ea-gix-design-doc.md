# ea-gix — Git Backend for eä

**Project:** `ea-gix` **Organization:** `git-ainur` **Status:** Draft — backend design document **Author:** Joey Carpinelli **Date:** April 2026 **References:**

- [ea-design-doc.md](ea-design-doc.md) — Original monolithic design (historical; performance analysis and concern sections remain relevant)
- [ea-revised-design-doc.md](ea-revised-design-doc.md) — Abstract kernel specification (authoritative trait definitions)

---

## 0. Purpose

This document describes `ea-gix`, the canonical git backend for the `ea` trait library.
It maps each abstract trait from the revised design document onto concrete git primitives via gitoxide (`gix`).
Where the original design document described a single library combining abstract primitives with git-specific implementation, this document covers only the git-specific layer.
The abstract contracts are defined in the revised design document and are not repeated here.

This document supersedes the original design document's implementation sections (Sections 2–5, Phase 0–2 of the development plan).
The original document's concern analysis (Sections 6–7) and performance discussion (Section 5) remain applicable to this backend and are referenced rather than duplicated.

## 0.1 Storage Model: There Is No `.ea` Directory

An `ea-gix` database is a git repository.
Not "backed by" a git repository — it *is* one.
`git init` creates it.
The object database, refs, packfiles, and transport are all standard git.
`ea-gix` writes objects to the ODB and maintains pointers under `refs/ea/*`.
It creates no new directories, no new file formats, and requires no new tools to install.

This means an `ea-gix` database can coexist in the same `.git` as a normal source code repository.
Source code lives in `refs/heads/`.
Structured ea state lives in `refs/ea/`.
Both share the same ODB and the same packfiles.
`git push` can push both.
`git clone` fetches both.
If ea state references the same blobs as the source tree (e.g., a forge storing issue metadata alongside the code it tracks), they are deduplicated automatically — stored once in the ODB.

Initialization is `git init` (or using an existing repository).
The first `ea` transaction writes the `.ea/` metadata subtree and the `refs/ea/` pointer into the repository.
There is no migration, no sidecar database, no separate sync channel.
Any tool that speaks git can inspect, back up, and transport ea state, even if it has no knowledge of `ea`.

The local-only auxiliary files — the WAL (Section 5) and the SQLite read cache (Section 6) — live outside the git object store, in `.git/ea-wal` and `.git/ea-cache.db` respectively.
These are ephemeral and rebuildable.
They are not synced, not versioned, and can be deleted at any time without data loss.

## 1. Trait Mappings

### 1.1 ContentAddressable → Git Object Database

The git object database (ODB) is a content-addressed store keyed by SHA-1 or SHA-256 depending on repository configuration.

| Trait element | Git realization |
|---|---|
| `Hash` | `gix::ObjectId` (20 or 32 bytes) |
| `Value` | Git object: blob, tree, or commit |
| `store(value)` | `gix::Repository::write_object()` — hashes and persists |
| `retrieve(hash)` | `gix::Repository::find_object()` — lookup by OID |
| `contains(hash)` | `gix::Repository::has_object()` |

**Law compliance:**

- Determinism: git hashes are computed from object type, size, and content.
  Same content always produces the same OID.
- Round-trip: objects are stored verbatim and retrieved verbatim.
- Referential transparency: two objects with the same OID are byte-identical by construction.

All three laws are guaranteed by the git specification, not by gitoxide specifically.
Any conforming git implementation satisfies them.

### 1.2 Pointer → Git Ref

A git ref is a named mutable pointer to an OID.

| Trait element | Git realization |
|---|---|
| `Hash` | `gix::ObjectId` |
| `read()` | `gix::Repository::find_reference()` → peel to OID |
| `cas(old, new)` | `gix::refs::transaction` — lock ref, verify current value matches `old`, update to `new`, commit |

**CAS implementation detail:** gitoxide's ref transaction acquires a lockfile on the ref, reads the current value, compares against the expected value, and writes the new value atomically by renaming the lockfile.
This provides the atomicity and linearizability required by the `Pointer` trait.
On CAS failure (current value ≠ expected), the lockfile is dropped and `CasFailure` is returned.

Alternatively, for environments where gitoxide's ref transaction is insufficient, `git update-ref --stdin` provides the same semantics via the `start` / `verify` / `update` / `commit` protocol. `ea-gix` prefers the native gix path but can fall back to subprocess invocation if needed (e.g., if targeting a git version with ref backends that gix does not yet support, such as reftable).

### 1.3 Chain → Git Commit Chain

A chain is an ordered, append-only log.
In git, this maps directly to a commit chain: each commit has a parent pointer (the previous entry), a tree (the entry payload), and metadata (author, timestamp, message).

| Chain operation | Git realization |
|---|---|
| `head()` | Read the pointer ref → commit OID |
| `append(entry)` | Write entry as a tree object. Create a commit with parent = current head, tree = entry. CAS the pointer from old head to new commit. |
| `entries()` | Walk the commit chain via parent pointers (reverse chronological). `gix::traverse::commit::Simple` provides this. |

**Entry representation:** Each chain entry is a git tree.
The tree contains:

- The entry payload (application-defined blobs and subtrees).
- An `.ea-type` blob containing the type marker for this node (value: `chain-entry`).

The chain itself is not a separate object — it is the commit history reachable from the pointer.
The pointer ref *is* the chain.

### 1.4 Ledger → Git Tree

A ledger is a keyed mutable map.
In git, a tree object maps names (keys) to OIDs (values).

| Ledger operation | Git realization |
|---|---|
| `get(key)` | Traverse from root tree to the subtree representing this ledger, read the entry named `key`. |
| `put(key, value)` | Write the value as a blob or subtree. Create a new tree with the entry added/replaced. Propagate new tree hashes up to the root. |
| `delete(key)` | Create a new tree without the entry. Propagate up. |
| `entries()` | Enumerate tree entries via `gix::object::tree::Tree::iter()`. |

**Structural sharing:** Modifying one key in a ledger creates new tree objects only along the path from the modified entry to the root.
All sibling subtrees are referenced by their existing OIDs.
This is git's fundamental structural sharing property and requires no special handling — it falls out of how tree objects work.

**Key encoding:** Ledger keys are encoded as tree entry names.
Git tree entry names are arbitrary byte strings (excluding `/` and null).
Keys containing these characters must be escaped or hashed.
For hierarchical keys, nested subtrees represent path segments naturally.

### 1.5 Recursive Composition → Nested Trees

A ledger value that is itself a chain: the tree entry for that key is a subtree containing an `.ea-type` marker (`chain`) and the chain's entries as numbered subtrees.
The chain's history is embedded in the enclosing transaction chain (see Section 2, nested chain representation).

A chain entry that is itself a ledger: the commit's tree is a ledger tree, with `.ea-type` marker `ledger`.

The merge dispatcher reads `.ea-type` at each level to determine the strategy.
This is a depth-first tree walk during three-way merge.

## 2. State Tree Layout

The entire `ea-gix` state is a single git tree, referenced by a single git ref (the pointer), via a single git commit (the latest transaction).
The tree layout:

```text
<root tree>
├── .ea-type              → blob: "ea-root"
├── .ea/                  → self-hosted metadata (see Section 8.1)
│   ├── .ea-type          → blob: "ledger"
│   ├── schema-version    → blob: "0.1"
│   ├── type-registry/    → subtree (ledger: type-name → merge-strategy-id)
│   └── annotations/      → subtree (ledger: path → policy annotations)
├── <ledger-name>/
│   ├── .ea-type          → blob: "ledger"
│   ├── <key>             → blob (leaf value) or tree (nested primitive)
│   └── <key>/
│       ├── .ea-type      → blob: "chain"
│       └── ...
├── <chain-name>/
│   ├── .ea-type          → blob: "chain"
│   └── (entries are in the commit history, not the tree)
└── ...
```

**Nested chain representation: embedded (Option A).**

The root-level transaction chain uses git's commit history directly (each transaction is a commit).
Chains nested *within* the state tree are embedded: entries are subtrees named by sequence number.
Appending means writing a new tree with the entry added.
History of a nested chain is recovered by diffing the enclosing transaction commits — the root chain records the before and after state of every nested primitive on every transaction.

An alternative design (Option B) would give each nested chain its own independent commit history in the ODB, referenced by a head-OID blob in the parent tree.
This was rejected for three reasons:

1. **GC safety.**
   Git's reachability walk follows tree and commit pointers, not OIDs embedded in blob content.
   Nested chain commits under Option B are invisible to `git gc` and would be pruned unless a custom keep-alive mechanism is maintained.
   This is a correctness hazard, not merely a performance concern.
2. **Clone and backup.**
   `git clone` fetches objects reachable from refs via the standard graph walk.
   Option B objects are not reachable through that walk, breaking backup and replication without a custom transport layer or a ref-per-chain (which reintroduces the ref scalability problem the single-ref design solved).
3. **Marginal benefit.**
   The incremental computation model (see Section 7) requires knowing *whether* a subtree changed, not independent traversal of its history.
   Subtree change detection under Option A is an OID comparison on the parent tree entry — O(1) per subtree.
   The independent traversal that Option B enables is rarely needed and is recoverable from the root transaction chain when it is.

## 3. Transaction Implementation

A transaction in `ea-gix`:

```text
1. Read the pointer ref → commit OID → root tree OID.
2. Parse the root tree into an in-memory representation.
3. Apply mutations:
   a. For each read: traverse tree by path, resolve OIDs via ODB.
   b. For each write: record the mutation in memory.
4. Flush:
   a. For each modified subtree (bottom-up), write a new tree object to ODB.
   b. Unchanged subtrees are referenced by existing OID (structural sharing).
   c. Write a new root tree.
   d. Write a new commit (parent = old head commit, tree = new root tree).
5. CAS the pointer ref from old commit OID to new commit OID.
6. On CAS failure: discard written objects (they are unreferenced and will be GC'd), retry from step 1.
```

**Object creation on retry:** Failed transactions leave orphaned objects in the ODB.
Git's garbage collector (`git gc`) prunes unreferenced objects.
For high-contention workloads, frequent retries could create substantial garbage.
Mitigation: batch transactions to reduce contention, or run `git gc` periodically.
This is a known trade-off in any optimistic concurrency system backed by an append-only store.

**In-memory buffering:** For the periodic flush mode described in the original design document (Section 5.3), mutations accumulate in an in-memory tree representation.
A lightweight WAL file records mutations between flushes for crash recovery.
On flush, the WAL is replayed into the in-memory tree, the tree is written to git, and the WAL is truncated.

The WAL is not a git object.
It is a local file outside the git repository (e.g., in `.git/ea-wal` or a configurable path).
It is ephemeral by design — it exists only to survive crashes between flushes.

## 4. Merge Implementation

Three-way merge between two forked states (base, left, right) where base is their common ancestor commit.

### 4.1 Dispatcher

```text
1. Resolve base, left, right root trees.
2. Walk all three trees in parallel, entry by entry.
3. For each entry present in any tree:
   a. If unchanged in both forks (same OID in left and right): keep as-is.
   b. If changed in only one fork: take the changed version.
   c. If changed in both forks:
      i.   Read .ea-type from the entry's subtree.
      ii.  Look up the registered MergeStrategy for that type.
      iii. Delegate to the strategy.
      iv.  If the strategy returns Resolved: write the result.
      v.   If the strategy returns Conflict: write a conflict record.
4. Write the merged root tree. Create a merge commit with two parents.
5. CAS the pointer.
```

### 4.2 Chain Merge (Causal Interleave)

When two forks have both appended entries to a nested chain:

1. Identify entries present in left but not base (left-appended).
2. Identify entries present in right but not base (right-appended).
3. Interleave by causal metadata (timestamp from the enclosing transaction commit).
4. Produce a merged chain containing base entries + interleaved new entries.

No conflicts are possible in chain merge unless entries are structurally identical (same content hash), in which case they are deduplicated.

### 4.3 Ledger Merge (Key-by-Key)

For each key in the union of base, left, right:

| base | left | right | result |
|---|---|---|---|
| — | — | — | (unreachable) |
| — | V | — | V (left added) |
| — | — | V | V (right added) |
| — | V₁ | V₂ | Conflict if V₁ ≠ V₂; V if V₁ = V₂ |
| B | B | B | B (unchanged) |
| B | L | B | L (left modified) |
| B | B | R | R (right modified) |
| B | L | R | Conflict if L ≠ R; L if L = R |
| B | — | B | — (left deleted) |
| B | B | — | — (right deleted) |
| B | — | R | Conflict (left deleted, right modified) |
| B | L | — | Conflict (left modified, right deleted) |
| B | — | — | — (both deleted) |

Conflict resolution is delegated to the registered policy for the ledger's type marker.
Default: preserve both sides as a conflict ledger (a ledger with keys `base`, `left`, `right` pointing at the three values, with `.ea-type` = `conflict`).

### 4.4 Recursive Merge

When a conflicting entry is itself a typed subtree (a nested ledger or chain), the dispatcher recurses.
Merge is depth-first: resolve children before parents.
A conflict at a leaf propagates upward as a conflict record, not as a failure of the entire merge.

## 5. Performance Characteristics

The original design document's performance analysis (Section 5) applies directly.
Summary for this backend:

**Reads:** 4–5 ODB lookups per path traversal (ref → commit → root tree → subtree → entry).
Git memory-maps packfiles; in practice, these are pointer chases.
Multiple reads sharing a path prefix share lookups.

**Writes:** One tree object per modified subtree along the path to root, plus one commit object, plus one ref CAS.
For a state tree of depth D with M mutations in disjoint subtrees: M×D tree writes + 1 commit + 1 CAS.

**Sync:** Push/fetch of one ref.
Pack negotiation transmits only missing objects.
Bandwidth is proportional to state delta, not total state size.

**Scaling concern: tree breadth.**
Git tree objects are flat lists of entries, scanned linearly.
A ledger with 100k keys in a single tree level degrades to O(N) per lookup.
Mitigation: hierarchical key sharding (e.g., first two hex characters of key hash as a subtree prefix), reducing per-level breadth to ~256 entries.
This is a backend-level optimization transparent to the `ea` traits.

**Scaling concern: packfile bloat.**
Each transaction writes a new root tree and path trees even if most entries are unchanged.
Git's delta compression in packfiles mitigates this — identical subtree OIDs mean the pack only stores the changed tree objects plus deltas.
For write-heavy workloads, periodic repacking is necessary.
`git gc --auto` handles this, but scheduling may need tuning for high-frequency flush intervals.

**Scaling concern: history depth.**
Long transaction chains (millions of commits) are cheap to store but expensive to traverse linearly.
`git log` performance on deep linear histories is well-studied and manageable.
Shallow clone (`--depth N`) bounds sync cost.
For read-path queries, the optional SQLite cache (see Section 6) avoids history traversal entirely.

## 6. Optional Read Cache

For workloads requiring fast key lookup, prefix scan, or filtered enumeration, `ea-gix` can maintain a local SQLite index materialized from the current state tree.

**Rebuild:** On each transaction commit, diff the old and new root trees (efficient — only walk changed subtrees).
Apply the diff to the SQLite index.
Cost is proportional to the transaction's mutations, not the total state size.

**Invalidation:** The SQLite index stores the root tree OID it was built from.
On startup, compare with the current pointer's root tree.
If they differ (e.g., after a `git fetch` advanced the ref), rebuild incrementally from the common ancestor.

**Scope:** The read cache is local, ephemeral, and rebuildable.
It is not synced.
It is not part of the git repository.
It lives alongside the WAL in `.git/ea-cache.db` or a configurable path.

This is explicitly not part of the `ea` trait contract — it is an `ea-gix`-specific optimization.
Other backends (e.g., `ea-sqlite`) may not need it because their storage layer already supports indexed queries.

## 7. Incremental Computation

Derived state — metadata indexes, caches, computed summaries — should not be rebuilt from scratch on every transaction. `ea-gix` supports a demand-driven incremental computation model inspired by Salsa (Rust's incremental computation framework for compilers).

### 7.1 Core Insight

Under Option A (embedded nested primitives), every subtree in the state tree has a git OID that changes if and only if the subtree's content changed.
This OID is a content-addressed revision identifier — functionally identical to Salsa's revision counter, but derived from content rather than assigned sequentially.

To determine whether a derived value is stale, compare the OIDs of its inputs against the OIDs recorded when it was last computed.
If all input OIDs match: the derived value is still valid.
If any differ: recompute.

### 7.2 Dependency Table

A dependency table maps derived values to their input paths and the OIDs those paths held at last computation time.

```text
derived_key → [(input_path, last_seen_oid), ...]
```

The dependency table is stored in the SQLite read cache (Section 6), not in the git tree.
It is local, ephemeral, and rebuildable — if the cache is lost, all derived values are treated as stale and recomputed on next access.

### 7.3 Invalidation Protocol

On each transaction commit:

1. Diff the old and new root trees.
   This yields a set of changed paths (the subtrees whose OIDs differ).
2. For each changed path, scan the dependency table for derived values that list that path as an input.
3. Mark those derived values as stale.

On read of a stale derived value:

1. Read the current OIDs of all input paths.
2. Recompute the derived value from the current inputs.
3. Store the result as a content-addressed object in the ODB.
4. Update the dependency table with the new input OIDs and result OID.

This is demand-driven: derived values are not recomputed until read.
For workloads where some derived state is read rarely, this avoids wasted computation.
For workloads where derived state must be fresh at all times, an eager mode can recompute all stale derived values at transaction commit time.

### 7.4 Composition with Merge

After a merge, the dependency table is fully invalidated (all derived values are stale).
This is conservative but correct — merge can change any subtree.
Incremental rebuild then proceeds on demand, recomputing only the derived values that are actually read after the merge.

A more precise strategy would diff the merge result against each parent and only invalidate derived values whose inputs changed.
This is an optimization for a later phase.

## 8. Self-Hosting and Bootstrapping

### 8.1 Self-Hosted Metadata

`ea-gix` requires bookkeeping state: schema version, registered type markers, merge strategy bindings, dependency table structure, and cache configuration.
Rather than storing this in an ad-hoc format (config files, magic blobs), `ea-gix` stores its own metadata as `ea` primitives within the state tree.

```text
<root tree>
├── .ea-type              → blob: "ea-root"
├── .ea/
│   ├── .ea-type          → blob: "ledger"
│   ├── schema-version    → blob: "0.1"
│   ├── type-registry     → subtree (ledger: type-name → merge-strategy-id)
│   ├── annotations       → subtree (ledger: path → policy annotations)
│   └── dependency-hints  → subtree (ledger: derived-key → input-paths)
├── <user ledgers and chains>
└── ...
```

The `.ea/` subtree is a ledger like any other.
It participates in transactions, gets structural sharing, survives merge, and has full history in the transaction chain.
The library's own bookkeeping is versioned, auditable, and syncable — the same properties the library provides to consumers.

**Bootstrapping sequence:** As described in Section 0.1, initialization requires only an existing git repository (`git init` or any pre-existing repo).
The first `ea` transaction writes the `.ea/` subtree and creates the `refs/ea/` pointer.
This is the only transaction that does not read prior state (there is none).
All subsequent transactions, including user-defined state, coexist with the `.ea/` metadata in the same root tree.
There is no separate initialization step, no migration, and no special tooling.

### 8.2 Backend Bootstrapping

New `ea` backends can be developed using `ea-gix` as scaffolding.
The pattern follows compiler bootstrapping:

1. **Stage 0:** Implement the new backend's bookkeeping (chunk indexes, boundary metadata, internal catalogs) as `ea` primitives stored on `ea-gix`.
   The new backend's own internal state is a ledger in a git repository.
   This is correct but slow — the new backend's performance is bounded by git's.

2. **Stage 1:** Verify the new backend against `ea`'s property test suite.
   All laws must hold.
   All merge tests must pass.
   The new backend is functionally correct, with its internals managed by git.

3. **Stage 2:** Rewrite the new backend's internal bookkeeping natively (e.g., `ea-prolly` stores its chunk index in its own prolly tree rather than in a git tree).
   Verify that outputs are identical to Stage 1 for the same inputs.

4. **Stage 3:** The new backend is fully self-contained.
   The git scaffolding is removed.

This is not a runtime architecture — production systems do not run `ea-prolly` on top of `ea-gix`.
It is a development and verification strategy that reduces the risk of introducing bugs in new backends by providing a reference implementation for all internal state at every stage.

The self-hosted metadata pattern (Section 8.1) makes this practical: the internal state that needs to be bootstrapped is already expressed as `ea` primitives, so porting it from one backend to another is a matter of swapping the `ContentAddressable` and `Pointer` implementations, not rewriting the logic.

## 9. gix Dependency

`ea-gix` depends on `gix` (gitoxide), not `git2` (libgit2 bindings).
Rationale:

- Pure Rust.
  No C FFI, no build-time C compilation, no soundness boundary.
- Actively maintained by Sebastian Thiel with explicit library-first design.
- Modular: `ea-gix` can depend on only the subcrates it needs (`gix-object`, `gix-ref`, `gix-odb`, `gix-traverse`) rather than the full `gix` facade.
- `no_std` potential for some subcrates, aligning with `ea` core's `no_std` design.

**Risk:** gitoxide does not yet expose a high-level multi-ref transaction API equivalent to libgit2's `git2::Transaction`.
For the `ea-gix` use case, this is not a blocker — we need single-ref CAS, which is supported via `gix-ref`'s lockfile-based transaction.
Multi-ref atomicity is not required because the state tree is rooted at a single ref.

**Minimum gix version:** To be determined during Phase 2 implementation. `ea-gix` will pin to a minimum version and document which `gix` subcrates and features are required.

## 10. Ref Namespace

`ea-gix` stores its pointer ref(s) under a dedicated namespace to avoid collisions with normal git refs (see Section 0.1 for the coexistence model).

```text
refs/ea/<n>          — state tree pointers (one per ea database in the repo)
refs/ea/meta/cache      — (optional) cache invalidation marker
```

The `refs/ea/` prefix is unlikely to collide with existing git conventions (`refs/heads/`, `refs/tags/`, `refs/remotes/`, `refs/notes/`).
Push/fetch refspecs can be configured to include or exclude `refs/ea/*` as needed.

A single git repository can host multiple independent ea databases by using distinct names under `refs/ea/`.
For example, a forge repository might use `refs/ea/forge` for issue/review state and `refs/ea/ci` for build metadata.
Each is an independent state tree with its own transaction chain, sharing the underlying ODB for object deduplication.

## 11. Development Plan

### Phase 0: Scaffold (1 week)

Create `ea-gix` crate.
Add `gix-object`, `gix-ref`, `gix-odb` dependencies.
Implement `ContentAddressable` for `gix::Repository` and `Pointer` for a gix ref.
Run `ea` core's property test suite against the implementations.
No typed primitives yet — just the foundation traits.

### Phase 1: Self-Hosted Metadata (1 week)

Implement the `.ea/` subtree (Section 8.1).
Bootstrap sequence: initialize repository with the metadata ledger as the first transaction.
Schema version, type registry, annotation store.
This is the first consumer of the `Ledger` implementation and validates the dogfooding model before any user-facing features exist.

### Phase 2: Primitives (2–3 weeks)

Implement `Chain` as commit chains and `Ledger` as tree objects.
State tree layout per Section 2 (Option A for nested chains).
Type markers (`.ea-type` blobs) registered in the self-hosted type registry.
Integration tests: create a state tree with nested chains and ledgers, apply transactions, verify structural sharing (count objects in ODB, confirm unchanged subtrees are not duplicated).

### Phase 3: Transaction Protocol (1–2 weeks)

Implement the full transaction loop per Section 3.
In-memory mutation buffer.
Tree write-back (bottom-up).
CAS commit.
Retry on contention.
Concurrency test: spawn N threads applying concurrent transactions, verify no lost updates and no invariant violations.

### Phase 4: Merge (3–4 weeks)

Implement the merge dispatcher per Section 4.
Built-in chain and ledger strategies.
Conflict representation.
Recursive merge for nested typed subtrees.
Property tests: generate random forked state trees, merge, verify correctness.
Fuzz: random concurrent transaction + merge sequences.

### Phase 5: WAL and Periodic Flush (1 week)

Append-only WAL file.
Configurable flush interval.
Crash recovery: replay WAL on startup.
Benchmark: compare per-mutation flush vs. batched flush at 10ms, 100ms, 1s intervals.

### Phase 6: Read Cache and Incremental Computation (3 weeks)

SQLite index.
Incremental rebuild from tree diffs.
Startup validation against current pointer.
Dependency table (Section 7.2).
Invalidation protocol on transaction commit.
Demand-driven recomputation of stale derived values.
Benchmark: key lookup latency with and without cache at 1k, 10k, 100k, 1M state entries.
Benchmark: derived value recomputation cost vs. full rebuild at various state tree sizes.

### Phase 7: Key Sharding (1 week)

Hierarchical key sharding for large ledgers.
Configurable shard depth and prefix derivation.
Benchmark: ledger operations at 100k+ entries with and without sharding.

### Phase 8: Documentation (1 week)

Crate-level docs.
Tutorial: "Store and retrieve typed state in a git repository using `ea-gix`."
Tutorial: "Define a derived value with incremental recomputation."
Performance guide: when to enable the read cache, how to tune flush intervals, when to shard keys.
Bootstrapping guide for new backend authors (Section 8.2).
Migration notes for users of the git-data primitives from the git-mirdain ecosystem.

---

**Total estimated timeline: 14–18 weeks to 0.1 release.**

The critical path is Phase 4 (merge).
Phases 6 and 7 (read cache, incremental computation, sharding) are performance optimizations that can be deferred past 0.1 if necessary, reducing the minimum viable timeline to approximately 9–12 weeks.
