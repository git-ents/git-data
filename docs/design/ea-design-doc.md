# eä — A Content-Addressed State Transition Kernel Over Git

**Project:** `ea` **Organization:** `git-ainur` **Status:** Draft — exploratory design document **Author:** Joey Carpinelli **Date:** April 2026

---

## 1. Thesis

A transaction is a pure function from one content hash to another.
Git already provides a content-addressed object store, named mutable pointers, and a Merkle DAG.
These are sufficient to implement a generalized database kernel where every state transition is a git commit, every checkpoint is a tree, and replication is `git fetch`.

`ea` extracts this primitive and exposes it as a small Rust library.
It does not compete with SQLite on throughput.
It serves the class of problems where history, auditability, offline-first sync, and structural merge are requirements rather than afterthoughts.

## 2. Core Model

### 2.1 State

A **state** is a git tree object.
The tree's Merkle structure provides content addressing, structural sharing (unchanged subtrees are never duplicated), and O(path-depth) access to any entry.

### 2.2 Transition

A **transition** is a git commit: it records `hash(input_state) → hash(output_state)` as a parent-tree pair.
The commit message and trailers carry metadata about the transition (author, timestamp, type, policy).

Properties inherited from git:

- **Deduplication.**
  Identical transitions (same input, same output) collapse to the same OID.
- **Verifiability.**
  Replay the transition chain from any checkpoint and compare output hashes.
- **Composability.**
  The output of transition A is a valid input to transition B if and only if the tree hashes match.
- **History.**
  The full transition chain is `git log`.

### 2.3 Pointer

A **pointer** is a git ref.
It names the current head of a transition chain.
Advancing the pointer is an atomic compare-and-swap (CAS).
This is the only mutable operation in the system.

### 2.4 Transaction

A **transaction** is:

1. Read the current pointer → resolve to a commit → resolve to a root tree.
2. Traverse the tree to read relevant state (4–5 object reads per path).
3. Compute mutations in memory.
4. Write a new tree incorporating all mutations (unchanged subtrees are shared by OID).
5. Write a commit (parent = old head, tree = new tree).
6. CAS the pointer from old head to new commit.

If the CAS fails (concurrent writer advanced the pointer), retry from step 1.
This is optimistic concurrency — no locks held during computation.

One ref.
One CAS per transaction.
Arbitrarily many mutations per transaction.

## 3. Typed Primitives

`ea` provides composable typed data structures laid out as subtrees within the state tree.
Each primitive is a convention on tree structure, not a new storage format.

### 3.1 Chain

An append-only ordered log.
Encoded as a subtree where entries are named by sequence number or UUIDv7.
New entries are appended; existing entries are never modified or deleted.

**Merge strategy:** Concurrent appends from divergent forks are interleaved by causal order (timestamp or vector clock in entry metadata).
Duplicate entries (same content hash) are collapsed.

**Use cases:** Operation logs, audit trails, event streams.

### 3.2 Ledger

A mutable keyed store.
Encoded as a subtree where entries are named by key (caller-provided, sequential, or content-hash).
Entries can be created, updated, and deleted.

**Merge strategy:** Different-key mutations auto-merge.
Same-key mutations are conflicts resolved by policy: last-writer-wins (Lamport timestamp), or escalation to a conflict object (a ledger containing both sides plus the common base).

**Use cases:** Entity state, configuration, mutable records.

### 3.3 Metadata

A derived index mapping one set of keys to another.
Always rebuildable from authoritative primitives (chains and ledgers).

**Merge strategy:** Never conflicts.
Rebuilt from the merged state of its inputs.

**Use cases:** Reverse indexes, cross-references, caches.

### 3.4 Source

Immutable content-addressed data.
A blob or tree stored by its OID.
No update semantics — new content produces a new OID.

**Merge strategy:** No conflicts possible.
Both sides' content exists after merge.

**Use cases:** File content, snapshots, artifacts.

### 3.5 Ephemeral

Local-only transient state.
Stored in a designated subtree that is excluded from push/fetch by convention.
Crash recovery discards it.

**Merge strategy:** Not synced, not merged.

**Use cases:** Working copy tracking, in-progress computations, local caches.

## 4. Recursive Composition

Primitives compose by nesting.
A ledger entry's value can be another ledger, a chain, or any typed subtree.
A chain can append entries that are themselves full state trees containing ledgers and metadata.

This means:

- A chain-of-ledgers records ordered history of mutable state.
- A ledger-of-chains is a keyed collection of independent event streams.
- Conflict objects are ledgers whose entries represent sides of a conflict, which may themselves contain nested conflicts — recursion bottoms out at source (concrete git objects).

Each subtree carries a type marker (a small blob at a conventional path within the subtree, e.g., `.ea-type`) that tells the merge machinery which strategy to dispatch.

## 5. Performance Characteristics

### 5.1 Reads

A single lookup: ref → commit → root tree → subtree → ... → entry.
Typically 4–5 object reads.
Git memory-maps packfiles, so these are pointer chases in practice.

Multiple reads sharing a path prefix pay only for the divergent suffix.
Worst case (N reads in disjoint subtrees): ~5N.
Best case (N reads in the same subtree): ~5 + N.

### 5.2 Writes

Writes are batched in memory and flushed as a single transaction.
The flush creates: one new tree object per modified subtree path, one commit object, one ref CAS.
Unchanged subtrees are shared by OID — a transaction touching 3 entries in a million-entry state tree writes approximately 3×depth tree objects plus the commit.

### 5.3 Periodic Flush Mode

For high-frequency mutation workloads, `ea` supports an in-memory buffer with periodic flush to git.
Between flushes, a lightweight append-only WAL file provides crash recovery.
On flush: WAL entries are applied, a git transaction is written, the WAL is truncated.
This amortizes git object creation overhead across many mutations.

### 5.4 Sync

Replication is `git push`/`git fetch` of a single ref.
Bandwidth is proportional to the delta between the two peers' states, not the total state size, because git's pack protocol negotiates common ancestors and transmits only missing objects.

### 5.5 Scaling Limits

- **State tree breadth.**
  Git trees can hold ~4 billion entries.
  Practical limit is packfile delta efficiency degrading with very large flat directories.
  Recommend hierarchical key namespacing (sharding by prefix) for collections exceeding ~10k entries.
- **History depth.**
  Long transition chains are cheap to store (commit objects are small) but expensive to traverse linearly.
  Shallow clone support bounds sync cost.
  Local index cache bounds query cost.
- **Object size.**
  Individual blobs up to ~4GB (git limitation).
  Packfile delta compression works poorly on large binary blobs.
  Recommend chunking or external storage with OID references for values exceeding ~10MB.

## 6. Concerns: Porcelain Authors (gin, jj, git)

This section addresses why a porcelain author building a tool like gin, jj, or any git-based version control system might choose *not* to adopt `ea`.

### 6.1 Abstraction Overhead

A porcelain author who understands git internals may view `ea` as an unnecessary indirection.
Writing tree objects, commits, and CAS ref updates directly is straightforward.
The typed primitives (chain, ledger, metadata) impose structural conventions that may not match the porcelain's preferred data layout.
If the porcelain only needs one or two patterns (e.g., an operation log and a change-id map), adopting a library that also handles arbitrary recursive composition is paying for generality that goes unused.

### 6.2 Performance Control

Version control porcelains are latency-sensitive.
Users expect sub-100ms response times for common operations.
`ea`'s typed tree traversal adds indirection that a hand-tuned porcelain can avoid.
For example, jj maintains a custom in-memory index for commit graph queries that would be slower if routed through `ea`'s generic tree model.
A porcelain author may reasonably conclude that owning the storage layer is necessary for competitive performance.

### 6.3 Merge Semantics Are Domain-Specific

`ea` provides default merge strategies per primitive type, but version control merge is deeply domain-specific.
File-level three-way merge, conflict materialization, rebase semantics, and commit rewriting all require logic that `ea`'s generic type-aware merge cannot provide.
A porcelain would need to override most of `ea`'s merge machinery anyway, reducing the library to a thin tree-manipulation wrapper.

### 6.4 Adoption Risk

A porcelain that depends on `ea` inherits `ea`'s design decisions, performance characteristics, and maintenance trajectory.
If `ea` makes a breaking change to its tree layout conventions or type marker format, every consumer must migrate.
A porcelain author with a stable, shipping product may prefer the reliability of a hand-built storage layer over the convenience of a shared library with a smaller user base and shorter track record.

### 6.5 Existing Alternatives

Porcelain authors targeting git already have `git2` (libgit2 bindings) and `gix` (gitoxide) as mature, well-tested libraries for object manipulation.
These provide the low-level primitives (`ea` would itself depend on one of them) without imposing structural conventions.
The incremental value of `ea` over raw `git2`/`gix` may not justify the dependency for a porcelain that already knows exactly what data layout it wants.

## 7. Concerns: Professional Data Providers

This section addresses why a professional data provider — a company managing datasets, data pipelines, or data products — might choose *not* to adopt `ea`.

### 7.1 Query Performance

Professional data workloads require indexed queries: range scans, joins, aggregations, filtered projections.
`ea` provides O(path-depth) key lookup and O(N) scan within a subtree.
There is no query planner, no secondary indexes beyond what the consumer builds in the metadata primitive, and no query language.
A data provider would need to build a full query layer on top of `ea`, at which point the question becomes why not use a database that already has one.

### 7.2 Schema and Validation

`ea` has no schema enforcement.
Tree structure is by convention, enforced (if at all) by the consumer.
A data provider managing regulated or contracted datasets needs schema validation, type checking, migration tooling, and access control at the data layer.
`ea` provides none of these.
The consumer must build a schema-on-write layer, a migration system, and an authorization model, all without the benefit of existing database ecosystem tooling.

### 7.3 Scale

Professional data providers routinely manage datasets in the terabyte-to-petabyte range.
Git's object store was designed for source code repositories that rarely exceed single-digit gigabytes.
Packfile repacking at scale is slow and memory-intensive.
Ref negotiation for large object sets is expensive.
Shallow clone and partial clone help but are designed for source code patterns (many small files, deep history), not bulk data patterns (large blobs, wide tables, append-heavy).
A data provider would need to carefully validate that git's storage engine performs acceptably for their specific data shape and volume, and for most cases, it will not.

### 7.4 Ecosystem Integration

Data providers operate within ecosystems: Spark, Snowflake, BigQuery, dbt, Airflow, Kafka, Parquet, Arrow.
These tools do not speak git.
Integrating `ea` into a data pipeline requires building connectors, serialization bridges, and format converters.
The versioning and lineage benefits of `ea` may not justify the integration cost when competing tools (Delta Lake, Apache Iceberg, LakeFS) provide git-like versioning natively within the data ecosystem.

### 7.5 Replication Topology

Git's push/fetch model is peer-to-peer with optional blessed remotes.
Professional data providers typically need hub-and-spoke distribution, access-controlled read replicas, incremental materialized views for downstream consumers, and SLA-backed availability.
Git's replication model can be forced into these patterns but it is not designed for them.
A data provider would need a custom distribution layer, erasing much of the "replication is just `git fetch`" simplicity.

### 7.6 Auditability vs. Compliance

`ea`'s git-backed history provides technical auditability: every state transition is recorded and verifiable.
But regulatory compliance (SOX, GDPR, HIPAA) requires more: access logs, role-based permissions, data retention policies, right-to-erasure.
Git's append-only history is actively hostile to right-to-erasure requirements.
A data provider in a regulated industry would need to solve these problems outside of `ea`, and the git-based history that is `ea`'s primary selling point becomes a liability rather than an asset.

## 8. Development Plan

### Phase 0: Spike (1–2 weeks)

Validate the core transaction loop in ~500 lines of Rust.
Hard dependency: `gix` (gitoxide) for object store operations.
Deliverables:

- `State`: read and write git tree objects representing typed state.
- `Transition`: create a commit from old state to new state.
- `Pointer`: CAS ref update.
- A single integration test: create a state, apply a transition, verify the output hash, advance the pointer.

No typed primitives.
No merge.
No flush batching.
Just the kernel.

### Phase 1: Typed Primitives (2–4 weeks)

Implement chain, ledger, metadata, source, and ephemeral as tree layout conventions.

- Type marker blobs (`.ea-type`) at each typed subtree root.
- Read/write APIs for each primitive: chain append, ledger get/put/delete, metadata rebuild, source store/retrieve.
- Property tests: round-trip serialization, structural sharing verification (mutations to one subtree do not duplicate sibling subtrees).

### Phase 2: Composition and Recursion (2–3 weeks)

Enable primitives to nest arbitrarily.

- A ledger entry can point at a chain, another ledger, or a source OID.
- A chain entry can contain a full typed state tree.
- Recursive traversal for reads and writes.
- Property tests: deeply nested structures survive round-trip, structural sharing holds at all depths.

### Phase 3: Merge (3–5 weeks)

Type-aware three-way merge.

- Merge dispatcher: read `.ea-type` markers, select strategy per subtree.
- Implement per-primitive merge strategies (see Section 3).
- Conflict representation: conflict ledger with base/left/right entries.
- Recursive conflict detection and representation.
- Property tests: all auto-mergeable cases produce correct output, all real conflicts produce well-formed conflict objects.
  Fuzz test merge with random concurrent mutations.

### Phase 4: Periodic Flush and WAL (1–2 weeks)

In-memory mutation buffer with periodic git flush.

- Append-only WAL file for crash recovery between flushes.
- Configurable flush interval (time-based and mutation-count-based).
- Recovery: on startup, replay WAL, flush, truncate.
- Benchmark: throughput comparison between per-mutation flush and batched flush at various intervals.

### Phase 5: Read Cache (2–3 weeks)

Optional local SQLite index for fast queries without tree traversal.

- Materialized from the current state tree on demand.
- Invalidated and rebuilt on each transaction commit (incremental rebuild for the changed subtrees only).
- Query API: key lookup, prefix scan, type-filtered enumeration.
- Benchmark: read latency with and without cache at various state tree sizes.

### Phase 6: Documentation and Stabilization (2–3 weeks)

- Crate documentation with examples for each primitive and composition pattern.
- Specification document: tree layout conventions, type marker format, merge strategy contracts.
- Stability guarantees: define what is covered by semver for 0.1 release.
- Integration example: a minimal key-value store built on `ea` demonstrating the full transaction loop, typed primitives, merge, and sync.

---

**Total estimated timeline: 13–22 weeks to 0.1 crate release.**

This estimate assumes a single developer working part-time.
The critical path is Phase 3 (merge), which contains the only genuinely novel engineering in the project.
All other phases are well-understood git plumbing wrapped in a typed Rust API.
