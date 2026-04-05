# git-store — Plumbing for Non-Text Porcelain

**Project:** `git-store` **Organization:** `git-ainur` **Status:** Draft — design document **Author:** Joey Carpinelli **Date:** April 2026 **References:**

- [ea-design-doc.md](https://github.com/git-ents/git-data/blob/feat/db/docs/design/ea-design-doc.md) — Original ea design (historical)
- [ea-revised-design-doc.md](https://github.com/git-ents/git-data/blob/feat/db/docs/design/ea-revised-design-doc.md) — Abstract kernel specification
- [ea-gix-design-doc.md](https://github.com/git-ents/git-data/blob/feat/db/docs/design/ea-gix-design-doc.md) — Git backend implementation

---

## 0. Problem

Git's object database is fully general.
Blobs, trees, and commits can represent any structured data.
But git's porcelain — `diff`, `merge`, `status`, `log`, `add`, `commit` — assumes text files in a working directory.
Every project that stores structured data in git (issue trackers, config management, build systems, scientific provenance, access control, package lockfiles) reinvents the same things: a ref layout convention, a tree structure, a serialization format, a merge strategy, and transaction logic.
These ad-hoc implementations are fragile, incompatible, and expensive to build.

The gap is between `git hash-object` / `git update-ref` (too low-level) and `git add` / `git commit` (assumes text files).
There is no plumbing layer for structured data.

`git-store` is that layer.

---

## 1. What git-store Is

A Rust library (`git-store`) and a set of CLI plumbing commands (`git db`) for transactional, typed, structured data operations over a standard git repository.
No new file formats.
No new directories.
No new protocols.
Everything is git objects and refs.

`git-store` is to structured data porcelains what `git hash-object` / `git mktree` / `git update-ref` are to text porcelains: the plumbing that higher-level tools build on.

A database created by `git-store` coexists in the same `.git` as source code.
Source lives in `refs/heads/`.
Structured data lives in `refs/db/<n>`.
They share the ODB, packfiles, and transport.
`git push` pushes both.
`git clone` fetches both.
If they reference the same blobs, deduplication is automatic.

---

## 2. Foundation Traits

Two traits define the storage contract.
All higher-level operations are expressed in terms of these.

### 2.1 ContentAddressable

```rust
pub trait ContentAddressable {
    type Hash: Eq + Clone + Hash;
    type Value;

    fn store(&self, value: &Self::Value) -> Result<Self::Hash>;
    fn retrieve(&self, hash: &Self::Hash) -> Result<Option<Self::Value>>;
    fn contains(&self, hash: &Self::Hash) -> Result<bool>;
}
```

**Laws (enforced by property tests in `git-store`):**

1. **Determinism.** `store(v)` always returns the same hash for the same value.
2. **Round-trip.** `retrieve(store(v))` returns `Some(v')` where `v' == v`.
3. **Referential transparency.**
   Two values with the same hash are semantically identical.

The git implementation: `Hash` = `gix::ObjectId`.
`Value` = git object (blob, tree, commit).
`store` = `write_object()`.
`retrieve` = `find_object()`.
The laws are guaranteed by the git specification.

### 2.2 Pointer

```rust
pub trait Pointer {
    type Hash: Eq + Clone;

    fn read(&self) -> Result<Option<Self::Hash>>;
    fn cas(
        &self,
        expected: Option<Self::Hash>,
        new: Option<Self::Hash>,
    ) -> Result<(), CasFailure>;
}
```

**Laws:**

1. **Atomicity.**
   A CAS either fully succeeds or fully fails.
2. **Linearizability.**
   Concurrent CAS operations are totally ordered.
3. **Consistency.**
   After a successful `cas(old, new)`, `read()` returns `new` absent further writes.

The git implementation: `Pointer` = git ref under `refs/db/<n>`.
CAS via gix ref transaction (lockfile, verify, update, rename).
Fallback to `git update-ref --stdin` for reftable or other backends gix doesn't yet support.

### 2.3 Closure Property

Any `ContentAddressable` store can store the complete serialized state of any other `ContentAddressable` store as a single value.
This is structural — it falls out of `Value` being general enough to contain arbitrary bytes.
The property guarantees interoperability: a git-store database can embed another git-store database, and backend bootstrapping is always available.

---

## 3. Primitives

Two primitives.
Everything else is a policy annotation on one of these.

### 3.1 Chain

An ordered, append-only log.

```rust
pub trait Chain {
    type Store: ContentAddressable;
    type Entry;

    fn head(&self) -> Result<Option<<Self::Store as ContentAddressable>::Hash>>;
    fn append(&mut self, entry: Self::Entry) -> Result<<Self::Store as ContentAddressable>::Hash>;
    fn log(&self) -> Result<impl Iterator<Item = Self::Entry>>;
}
```

Git implementation: a commit chain.
Each commit's tree carries the entry payload.
Parent pointer is the previous entry.
The chain head is the current commit OID.

Merge: causal interleave.
Entries from both forks combined in causal order (Lamport timestamp or content hash for deterministic tiebreak).
Duplicates (same content hash) collapsed.

CRDT equivalence: a chain is a G-Set (grow-only set) with a total order.
Two peers append independently; sync is union; result is deterministic.

### 3.2 Ledger

A keyed, mutable map.

```rust
pub trait Ledger {
    type Store: ContentAddressable;
    type Key;
    type Value;

    fn get(&self, key: &Self::Key) -> Result<Option<Self::Value>>;
    fn put(&mut self, key: Self::Key, value: Self::Value) -> Result<<Self::Store as ContentAddressable>::Hash>;
    fn delete(&mut self, key: &Self::Key) -> Result<<Self::Store as ContentAddressable>::Hash>;
    fn list(&self) -> Result<impl Iterator<Item = (Self::Key, Self::Value)>>;
}
```

Git implementation: a subtree in the state tree.
Keys are tree entry names.
Values are blobs or nested subtrees.

Merge: key-by-key three-way.
Disjoint keys auto-merge.
Same-key conflicts resolved by pluggable policy (last-writer-wins, preserve-both, custom function).

CRDT equivalence: with LWW-Register semantics per key, a ledger is a conflict-free OR-Map.

### 3.3 Policy Annotations

Other data structure types are not new primitives — they are chains or ledgers with constraints:

| Type | Realization |
|---|---|
| Derived index | Ledger + `derived` (maintained inline by writers; rebuilt after merge from primary data) |
| Immutable store | Ledger + `write-once` (key = content hash, no overwrite) |
| Local state | Chain or Ledger + `local-only` (excluded from push/fetch; lives under `refs/db-local/<n>`) |
| Conflict record | Ledger with keys `base`, `left`, `right` + `.db-type` = `conflict` |

`derived` indexes are not lazily invalidated and recomputed.
Writers maintain them atomically in the same transaction as the primary write.
The `derived` annotation exists to tell the merge dispatcher to rebuild the index from primary data after a merge rather than attempting to merge the index directly.

`local-only` ledgers and chains are stored under a separate local ref (`refs/db-local/<n>`) that is never advertised during `upload-pack`.
They use the same transaction machinery as shared data.

### 3.4 Recursive Composition

Primitive values can themselves be primitives.
A ledger value can be a chain.
A chain entry can contain a ledger.
Recursion bottoms out at opaque blobs (application-defined content).
A `.db-type` marker blob at each typed subtree root tells the merge dispatcher which strategy to apply.

---

## 4. Type Definitions via Facet

Rust consumers define git-store types using the [facet](https://github.com/facet-rs/facet) reflection library.
The Rust type definition is the authoritative source for serialization layout, merge strategy, and type registration.
No separate schema files.
No manual registry entries.

### 4.1 The GitDbType Trait

```rust
pub trait GitDbType: facet::Facet {
    /// The `.db-type` marker written to the state tree.
    /// Derived from the facet type name by default.
    const MARKER: &'static str;

    /// The merge strategy for this type.
    /// Derived from type structure and facet attributes by default.
    type MergeStrategy: MergeStrategy;
}
```

Implementations are derived, not hand-written:

```rust
#[derive(Facet, GitDbType)]
struct ForgeIssue {
    title: String,
    #[facet(merge = "lww")]
    state: IssueState,
    labels: LabelSet,
}
```

The derive macro:

1. Walks the facet `SHAPE` to produce the git tree layout (struct fields → named blobs,
   nested structs → subtrees).
2. Reads `#[facet(merge = "...")]` attributes to select per-field merge strategies.
3. Sets `MARKER` from the type name.
4. Writes `MARKER` and the strategy name to `.db/type-registry` on `db init`.

### 4.2 Merge Strategy Derivation

Default strategy selection from shape:

| Shape | Default strategy |
|---|---|
| Named struct | Ledger merge (field-by-field) |
| `Vec<T>` | Chain merge (causal interleave) |
| `Option<T>` | LWW |
| Scalar blob | LWW |

Override per field with `#[facet(merge = "...")]`.
The strategy name must be registered in the `StrategyMap` — either a built-in or an application-provided implementation.

### 4.3 CLI and Shell Consumers

The CLI cannot use Rust trait impls directly.
For CLI and shell porcelains, the `.db/type-registry` ledger (written at `db init` by the Rust consumer) provides string → strategy mappings that the merge dispatcher reads at runtime.
The Rust type definition is the source of truth; the registry blob is derived from it.

Shell porcelains that never call `db merge` do not need the registry at all.

---

## 5. Transaction

```rust
pub struct Db { /* gix::Repository + ref name */ }

impl Db {
    pub fn open(repo: &gix::Repository, name: &str) -> Result<Self>;
    pub fn init(repo: &gix::Repository, name: &str) -> Result<Self>;
    pub fn transaction(&self) -> Result<Tx>;
}

pub struct Tx { /* snapshot oid, in-memory mutation buffer */ }

impl Tx {
    // Ledger operations
    pub fn get(&self, path: &[&str]) -> Result<Option<Value>>;
    pub fn put(&mut self, path: &[&str], value: Value) -> Result<()>;
    pub fn delete(&mut self, path: &[&str]) -> Result<()>;
    pub fn list(&self, path: &[&str]) -> Result<Vec<String>>;

    // Chain operations
    pub fn append(&mut self, path: &[&str], entry: Value) -> Result<()>;
    pub fn log(&self, path: &[&str]) -> Result<Log>;

    // Commit — single ref CAS, atomic
    pub fn commit(self, meta: CommitMeta) -> Result<Oid, CasFailure>;
}
```

Protocol:

1. **Begin.**
   Read pointer → commit → root tree.
   Snapshot in memory.
2. **Read.**
   Path traversal through content-addressed trees. 4–5 object reads per lookup.
3. **Write.**
   Mutations accumulate in memory.
   No I/O until commit.
4. **Commit.**
   Write new tree objects bottom-up (only modified paths).
   Write commit (parent = old head).
   CAS the pointer.
   On failure, retry from step 1.

Single pointer per database.
One CAS per transaction.
Arbitrarily many mutations per transaction.
Structural sharing: unchanged subtrees referenced by existing OID.

**Writers maintain derived indexes in the same transaction as primary writes.**
There is no separate invalidation pass.
A transaction that creates an issue also updates `index/issues-by-display-id/`.
Atomicity is free — it is already a single CAS.

---

## 6. State Tree Layout

```text
refs/db/<n>  →  commit (transaction N)
                     └── tree (root state)
                         ├── .db-type             → blob: "db-root"
                         ├── .db/                  → self-hosted metadata
                         │   ├── .db-type          → blob: "ledger"
                         │   ├── schema-version    → blob: "0.1"
                         │   ├── type-registry/    → ledger: type marker → merge strategy name
                         │   └── annotations/      → ledger: path → policy
                         ├── <user-defined>/
                         │   ├── .db-type          → blob: "ledger" | "chain"
                         │   └── ...
                         └── ...

refs/db-local/<n>  →  commit (local-only state)
                           └── tree
                               └── <user-defined local data>/
```

The `.db/` subtree is a ledger like any other — self-hosted, versioned, auditable.
The first transaction on `db init` writes it, populated from registered `GitDbType` implementations.
No external config files.

`refs/db-local/<n>` follows the same tree conventions but is never advertised during `upload-pack`.
Local-only chains and ledgers live here.

Nested chains use embedded representation: entries are subtrees within the parent, named by sequence number.
History is recovered from the enclosing transaction chain.
This preserves git reachability for GC and clone.

---

## 7. Derived Indexes

Derived indexes are ledgers maintained atomically by writers, not lazily recomputed by readers.

**Rule:** any transaction that mutates primary data must also update all `derived` ledgers that index that data, in the same `Tx`.
The `commit()` call is a single CAS covering both.

**On merge:** the merge dispatcher skips `derived` ledgers (per their policy annotation) and instead rebuilds them from the merged primary data.
This is a single tree walk over the merged state, not a dependency graph traversal.

**OID-based cache validity:** the subtree OID is the revision identifier for any subtree.
A process-local in-memory cache may store `(input_subtree_oid → derived_value)`.
Cache validity is a hash comparison.
No framework required.
If the OID of `issues/` matches the last-seen OID, the cached issue list is valid.

This pattern — writer-maintained indexes, OID-keyed process cache — covers the practical needs of porcelains like forge without external dependencies.

---

## 8. Merge

```rust
pub fn merge(
    db: &Db,
    left: Oid,
    right: Oid,
    strategies: &StrategyMap,
) -> Result<MergeResult>;

pub trait MergeStrategy: Send + Sync {
    fn merge(
        &self,
        base: Option<Value>,
        left: Option<Value>,
        right: Option<Value>,
    ) -> Result<MergeResult>;
}

pub enum MergeResult {
    Clean(Oid),
    Conflicted(Oid, Vec<Conflict>),
}
```

Dispatcher walks base, left, right trees in parallel.
At each node:

1. Same OID in left and right → keep.
2. Changed in one fork only → take the change.
3. `derived` annotation → skip; rebuild from merged primary data after dispatch
   completes.
4. Changed in both → read `.db-type`, dispatch to registered strategy.

For Rust consumers, strategies are registered from `GitDbType` implementations at startup.
For CLI consumers, strategies are loaded from `.db/type-registry`.
The dispatcher does not distinguish between the two sources.

Recursive: if a conflicting entry is a typed subtree, the dispatcher recurses.
Conflicts at leaves propagate upward as conflict records.

---

## 9. Query Layer

git-store's tree structure enables a useful query layer without SQL or a separate query engine.

### 9.1 Structural Properties

| Property | What it enables |
|---|---|
| Hierarchical key layout | Path wildcard traversal |
| Blob values at known paths | Predicate filtering without full scan |
| Subtree OID stability | Result caching keyed on OID |
| Structural sharing | Incremental re-query over changed subtrees only |

### 9.2 Path Algebra

The natural query interface is path algebra, not SQL:

```text
issues/*/state                    -- enumerate all issues, get state blob
issues/*/[state="open"]           -- filter by blob value at known path
reviews/*/approvals/*/*           -- two-level wildcard
issues/*/[state="open"]/title     -- project after filter
```

These translate directly to: enumerate a subtree, fetch a blob at a relative path, filter, project.
No full object-store scan.
O(N × depth) where N is the size of the enumerated subtree.

### 9.3 Index-Aware Planning

A query planner that knows which paths carry `derived` ledger indexes can rewrite predicates:

```text
issues/*/[display-id="GH#42"]
  → rewrite to: index/issues-by-display-id/GH#42
  → O(depth) instead of O(N × depth)
```

The planner reads `derived` annotations from `.db/annotations/`.
No separate schema required.
Porcelains that maintain good indexes get fast queries automatically.

### 9.4 OID Cache Integration

A query over `issues/*/state` touches a predictable set of subtree OIDs.
If the OID of `issues/` has not changed since the last execution, the result is valid without re-reading any blobs.
The query layer tracks input OIDs per query and short-circuits on match.
This is structural, not a framework feature.

### 9.5 Limitations

Cross-path joins with no common key require either a purpose-built index or an in-memory join over two full subtree scans.
The query layer does not help here beyond what explicit indexes provide.
Porcelains that need joins should maintain join-supporting indexes at write time.

---

## 10. CLI Plumbing Commands

`git db` provides the CLI equivalent of every library operation, following git's convention of low-level plumbing commands that porcelains compose.

### 10.1 Database Lifecycle

```text
git db init <n>
    Create a new database. Writes .db/ metadata subtree, creates refs/db/<n>.
    Registers GitDbType implementations if called from a Rust binary; otherwise
    writes an empty type registry.

git db list
    List all databases in the repository (enumerate refs/db/*).

git db drop <n>
    Delete a database (remove refs/db/<n>, objects GC'd normally).
```

### 10.2 Transaction Commands

```text
git db tx begin <n>
    Start a transaction. Prints a transaction ID (the snapshot OID).
    Writes transaction state to .git/db-tx/<txid>.

git db tx get <txid> <path>
    Read a value. Prints blob content to stdout.

git db tx put <txid> <path> [--stdin | --file=<f> | <literal>]
    Stage a write.

git db tx delete <txid> <path>
    Stage a deletion.

git db tx append <txid> <path> [--stdin | --file=<f> | --tree <k=v>...]
    Stage a chain append. --tree accepts named blob pairs for structured entries.

git db tx log <txid> <path>
    Print chain entries to stdout.

git db tx list <txid> <path>
    List keys in a ledger.

git db tx commit <txid> [--message=<msg>] [--author=<a>]
    Commit the transaction. Atomic CAS. Exits 0 on success, 1 on contention.

git db tx abort <txid>
    Discard staged mutations.
```

Transaction state files in `.git/db-tx/` are local, ephemeral, and deleted on commit or abort.

### 10.3 History and Inspection

```text
git db log <n> [-n <count>]
    Print transaction history (commit log of refs/db/<n>).

git db show <n> [<path>]
    Print current state at path. Without path, prints root tree.

git db diff <n> <oid-a> <oid-b> [<path>]
    Diff two states. Output is path-level: added/modified/deleted keys.

git db cat <n> <oid>
    Print raw content of an object by OID.
```

### 10.4 Merge

```text
git db merge <n> <left-oid> <right-oid> [--strategy=<s>]
    Three-way merge. Derived ledgers rebuilt from merged primary data.
    Prints result OID and conflict summary.

git db conflicts <n> <oid>
    List unresolved conflicts in a merge result.

git db resolve <n> <oid> <path> [--take=left|right|base] [--value=<v>]
    Resolve a single conflict. Produces a new state OID.
```

### 10.5 Query

```text
git db query <n> <path-pattern> [--where <path>=<value>] [--select <path>]
    Path algebra query. Enumerates matching entries, applies predicates,
    projects selected subpaths. Uses index rewriting if applicable.

git db query <n> --explain <path-pattern> [--where <path>=<value>]
    Show query plan: which paths are scanned, which indexes are used.
```

### 10.6 Schema and Types

```text
git db type register <n> <type-name> [--merge-strategy=<s>]
    Register a type marker and its merge strategy in .db/type-registry.
    Normally written automatically by GitDbType::init(); use manually for
    shell-only porcelains.

git db type list <n>
    List registered types and strategies.

git db annotate <n> <path> <annotation>
    Set a policy annotation (derived, write-once, local-only) on a path.
```

---

## 11. Usage Example: forge

forge is a local-first issue and code-review tracker stored entirely in git.
It is the primary reference porcelain for git-store and drives all API design decisions.

### 11.1 State Tree Layout

forge uses a single database (`refs/db/forge`) with four top-level namespaces:

```text
refs/db/forge
└── tree
    ├── issues/              → ledger (keyed by content-hash OID of the title blob)
    │   └── <oid>/
    │       ├── title        → blob
    │       ├── state        → blob: "open" | "closed"
    │       ├── body         → blob (optional)
    │       ├── display-id   → blob: e.g. "GH#42" (set by sync adapter)
    │       ├── labels/      → ledger (name → empty blob; presence = set membership)
    │       └── assignees/   → ledger (contributor UUID → empty blob)
    ├── reviews/             → ledger (keyed by UUID v7)
    │   └── <uuid>/
    │       ├── title        → blob
    │       ├── state        → blob: "open" | "draft" | "closed" | "merged"
    │       ├── body         → blob (optional)
    │       ├── target/
    │       │   ├── head     → blob: commit OID
    │       │   ├── base     → blob: commit OID
    │       │   └── path     → blob: file path (absent = whole-tree review)
    │       ├── labels/      → ledger
    │       ├── assignees/   → ledger
    │       └── approvals/   → ledger
    │           └── <commit-oid>/<contributor-uuid> → empty blob
    ├── comments/            → ledger of chains (keyed by thread UUID)
    │   └── <thread-uuid>/   → chain
    │       └── <entry>/     → tree
    │           ├── body     → blob
    │           ├── anchor   → blob: "<oid>[:<start>-<end>]"
    │           ├── id       → blob: UUID v7
    │           ├── resolved → blob: "true" (absent = unresolved)
    │           ├── reply-to → blob: parent comment UUID (absent = top-level)
    │           └── replaces → blob: prior comment UUID (absent = original)
    ├── contributors/        → ledger (keyed by UUID v7)
    │   └── <uuid>/
    │       ├── handle       → blob
    │       ├── names/       → ledger
    │       ├── emails/      → ledger
    │       ├── keys/        → ledger
    │       └── roles/       → ledger
    ├── config/              → ledger
    │   └── providers/
    │       └── github/      → provider-specific config blobs
    └── index/               → derived ledger (annotation: derived)
        ├── issues-by-display-id/   → display-id string → issue OID
        ├── reviews-by-display-id/  → display-id string → review UUID
        └── comments-by-object/     → object OID → space-separated thread UUIDs
```

**GC note:** Any OID stored as blob content in the transaction tree is reachable from `refs/db/forge` and GC-safe. forge's prior `objects/` workaround subtree is unnecessary under git-store and must be removed when forge is migrated.

**Index maintenance:** All mutations to `issues/`, `reviews/`, and `comments/` update the corresponding `index/` entries in the same transaction.
There is no deferred rebuild.
After a merge, the merge dispatcher rebuilds `index/` from the merged primary data in a single tree walk.

**Chain entry structure:** Comment chain entries are trees, not flat blobs.
Each entry's tree contains body and metadata blobs.
`tx.append` accepts `Value::tree()`.

### 11.2 Rust Type Definitions

```rust
#[derive(Facet, GitDbType)]
#[db(path = "issues")]
struct ForgeIssue {
    title: String,
    #[facet(merge = "lww")]
    state: IssueState,
    body: Option<String>,
    #[facet(merge = "set")]
    labels: LabelSet,
    #[facet(merge = "set")]
    assignees: ContributorSet,
}

#[derive(Facet, GitDbType)]
#[db(path = "comments", primitive = "chain")]
struct ForgeComment {
    body: String,
    anchor: String,
    id: Uuid,
    resolved: bool,
    reply_to: Option<Uuid>,
    replaces: Option<Uuid>,
}
```

The derive macro produces `serialize`, `deserialize`, `MARKER`, and `MergeStrategy` impls.
No manual implementation required.

### 11.3 Porcelain Operations (Rust)

```rust
fn create_issue(db: &Db, title: &str, body: Option<&str>) -> Result<String> {
    let oid = git_hash_blob(title.as_bytes());
    let mut tx = db.transaction()?;
    tx.put(&["issues", &oid, "title"], Value::from(title))?;
    tx.put(&["issues", &oid, "state"], Value::from("open"))?;
    if let Some(b) = body {
        tx.put(&["issues", &oid, "body"], Value::from(b))?;
    }
    // Derived index maintained in same transaction.
    tx.commit(meta("open issue"))?;
    Ok(oid)
}

fn add_comment(
    db: &Db,
    thread_uuid: &str,
    anchor: &str,
    body: &str,
    reply_to: Option<&str>,
) -> Result<String> {
    let comment_id = uuid::Uuid::now_v7().to_string();
    let object_oid = anchor.split(':').next().unwrap();

    let mut entry = Value::tree();
    entry.insert("body", Value::from(body));
    entry.insert("anchor", Value::from(anchor));
    entry.insert("id", Value::from(comment_id.as_str()));
    if let Some(r) = reply_to {
        entry.insert("reply-to", Value::from(r));
    }

    let mut tx = db.transaction()?;
    tx.append(&["comments", thread_uuid], entry)?;

    // Derived index: maintained inline, same transaction.
    let current = tx.get(&["index", "comments-by-object", object_oid])?
        .map(|v| v.to_string())
        .unwrap_or_default();
    let updated = if current.is_empty() {
        thread_uuid.to_owned()
    } else {
        format!("{current} {thread_uuid}")
    };
    tx.put(&["index", "comments-by-object", object_oid], Value::from(updated.as_str()))?;

    tx.commit(meta("add comment"))?;
    Ok(comment_id)
}
```

---

## 12. What git-store Replaces

| Current approach | Problem | git-store equivalent |
|---|---|---|
| git-bug, git-appraise | Ad-hoc ref conventions, custom merge, can't share infrastructure | Porcelain on `git db` |
| YAML/JSON config in git | Text merge on structured data, broken merges | Ledger with key-level merge |
| Terraform state in git | No transactions, race conditions on concurrent apply | Transaction with CAS |
| DVC metadata | Custom sidecar format, separate sync | Chain + ledger in same repo |
| CODEOWNERS | Flat file, no history, no audit | Ledger with append-only audit chain |
| Package lockfiles | Constant merge conflicts on structured data | Ledger with package-name keys, auto-merge |
| git-notes | Single-key-per-object, no nesting, poor merge | Ledger with arbitrary structure |
| gitops state stores | Ad-hoc conventions, fragile CI scripts | Transaction-safe state with merge |

---

## 13. What git-store Does Not Do

- **No query language enforcement.**
  The query layer (§9) is additive.
  Reads are path lookups by default.
  Build a query layer on top if needed.
- **No schema enforcement.**
  Tree structure is by convention.
  Build a schema validator on top if needed.
- **No working directory.**
  There is no checkout.
  The state tree exists only as git objects.
- **No text diff/merge.**
  The text porcelain still handles source code. git-store handles structured data.
- **No new protocols.**
  Sync is `git push` / `git fetch`.
  Auth is whatever your git remote uses.
- **No hosted service.**
  git-store is plumbing.
  Hosted services are porcelain built on top.
- **No external database.**
  No SQLite, no external process.
  Everything is git objects and refs.

---

## 14. Relationship to Existing Work

**git plumbing.**
`git-store` composes `hash-object`, `mktree`, `write-tree`, `update-ref` into higher-level operations.
It does not replace or modify any existing git behavior.

**Irmin.**
Closest prior art.
OCaml library for mergeable, branchable, content-addressed stores.
`git-store` differs in being git-native, Rust, CLI-accessible, and reduced to two primitives.

**Noms / Dolt.**
Content-addressed versioned databases with prolly trees and SQL.
`git-store` operates at a lower layer — it provides primitives, without prescribing a query interface or storage format.

**facet.**
git-store uses facet for Rust type reflection.
The `GitDbType` derive macro produces serialization, merge strategy selection, and type registry entries from the facet `SHAPE`.
Shell consumers use the string registry written at `db init`.

**jj.**
jj could be implemented as a porcelain on `git-store` (operation log as a chain, change-id map as a ledger, branch pointers as ledger entries or real git refs).
Whether performance characteristics would be acceptable is an open question.

**Local-first / CRDTs.**
`git-store`'s chain is a G-Set and its ledger with LWW per key is a conflict-free OR-Map.
A local-first framework could use `git-store` as its persistence and sync layer.

---

## 15. Performance

**Reads:** 4–5 object lookups per path (ref → commit → root tree → subtree → entry).
Packfile memory-mapped.
Microseconds.

**Writes:** one tree object per modified subtree level + one commit + one CAS.
A transaction touching M disjoint keys in a tree of depth D: M×D tree writes + 1 commit + 1 CAS.
Typically under a kilobyte total.

**Derived index writes:** zero overhead beyond the primary write.
Index mutations are part of the same in-memory transaction buffer and committed in the same CAS.

**Sync:** push/fetch of one ref.
Bandwidth proportional to state delta, not total size.

**Scaling:** hierarchical key sharding for ledgers exceeding ~10k entries.
For read-heavy workloads requiring fast prefix scans, maintain a process-local in-memory map keyed by subtree OID.
Rebuild on OID change.
No external database required.

These are the performance characteristics of a small-to-medium structured data store.
`git-store` is not competing with SQLite on throughput.
It is competing with "ad-hoc YAML in a git repo" on correctness.

---

## 16. Development Plan

### Phase 0: Core Library (3–4 weeks)

`ContentAddressable` and `Pointer` implementations on gix.
`Tx` struct with get/put/delete/append/log/list/commit.
Single integration test: create a database, run a transaction, verify round-trip.
Property tests for foundation trait laws.

### Phase 1: Self-Hosted Metadata (1 week)

`.db/` subtree.
Schema version, type registry, annotation store.
First consumer of the ledger implementation.

### Phase 2: CLI Plumbing (2–3 weeks)

`git db init`, `git db tx begin/get/put/delete/append/log/list/commit/abort`, `git db show`, `git db log`, `git db diff`.
Shell-scriptable, unix-philosophy (stdin/stdout, exit codes).

### Phase 3: Merge (3–5 weeks)

Merge dispatcher.
Built-in chain and ledger strategies.
`derived` ledger rebuild after merge.
Conflict representation.
Recursive merge.
`git db merge`, `git db conflicts`, `git db resolve`.
Property tests and fuzz testing.

### Phase 4: Facet Integration (2 weeks)

`GitDbType` derive macro.
Serialization and deserialization from facet `SHAPE`.
Merge strategy derivation from shape and `#[facet(merge = "...")]` attributes.
Auto-population of `.db/type-registry` on `db init`.

### Phase 5: forge as Reference Porcelain (2–3 weeks)

Migrate forge's storage layer (`store.rs`, `issue.rs`, `review.rs`, `comment.rs`, `contributor.rs`) to use `git_db::Db` and `Tx`.
Remove `objects/` GC workaround.
Remove manual index rebuild logic; replace with inline index maintenance.
All existing integration tests in `crates/git-forge/tests/` must pass unchanged. forge MCP server operates correctly with no changes to its public tool API.

### Phase 6: Query Layer (2–3 weeks)

Path algebra evaluator.
Wildcard traversal, blob predicate filtering, subpath projection.
Index-aware query planner using `.db/annotations/`.
`git db query` and `git db query --explain`.
OID-keyed result caching.

### Phase 7: Documentation and Stabilization (1–2 weeks)

Crate docs.
Man pages for CLI commands.
Tutorial: "Build a porcelain on git-store."
Specification: tree layout, type markers, merge contracts, query algebra.

---

**Total estimated timeline: 15–22 weeks to 0.1 release.**

Critical path is Phase 3 (merge).
Minimum viable release (no facet integration, no query layer): Phases 0–3, approximately 6–8 weeks.
Sufficient for single-writer porcelains that sync via push/fetch without concurrent offline edits.
