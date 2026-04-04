# git-db — Plumbing for Non-Text Porcelain

**Project:** `git-db` **Organization:** `git-ainur` **Status:** Draft — design document **Author:** Joey Carpinelli **Date:** April 2026 **References:**

- [ea-design-doc.md](ea-design-doc.md) — Original ea design (historical)
- [ea-revised-design-doc.md](ea-revised-design-doc.md) — Abstract kernel specification
- [ea-gix-design-doc.md](ea-gix-design-doc.md) — Git backend implementation

---

## 0. Problem

Git's object database is fully general.
Blobs, trees, and commits can represent any structured data.
But git's porcelain — `diff`, `merge`, `status`, `log`, `add`, `commit` — assumes text files in a working directory.
Every project that stores structured data in git (issue trackers, config management, build systems, scientific provenance, access control, package lockfiles) reinvents the same things: a ref layout convention, a tree structure, a serialization format, a merge strategy, and transaction logic.
These ad-hoc implementations are fragile, incompatible, and expensive to build.

The gap is between `git hash-object` / `git update-ref` (too low-level) and `git add` / `git commit` (assumes text files).
There is no plumbing layer for structured data.

`git-db` is that layer.

## 1. What git-db Is

A Rust library (`git-db`) and a set of CLI plumbing commands (`git db`) for transactional, typed, structured data operations over a standard git repository.
No new file formats.
No new directories.
No new protocols.
Everything is git objects and refs.

`git-db` is to structured data porcelains what `git hash-object` / `git mktree` / `git update-ref` are to text porcelains: the plumbing that higher-level tools build on.

A database created by `git-db` coexists in the same `.git` as source code.
Source lives in `refs/heads/`.
Structured data lives in `refs/db/<name>`.
They share the ODB, packfiles, and transport.
`git push` pushes both.
`git clone` fetches both.
If they reference the same blobs, deduplication is automatic.

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

**Laws (enforced by property tests in `git-db`):**

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
        new: Self::Hash,
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

The git implementation: `Pointer` = git ref under `refs/db/<name>`.
CAS via gix ref transaction (lockfile, verify, update, rename).
Fallback to `git update-ref --stdin` for reftable or other backends gix doesn't yet support.

### 2.3 Closure Property

Any `ContentAddressable` store can store the complete serialized state of any other `ContentAddressable` store as a single value.
This is structural, not a special feature — it falls out of `Value` being general enough to contain arbitrary bytes.
The property guarantees interoperability: a git-db database can embed another git-db database, and backend bootstrapping (implementing new backends using existing ones as scaffolding) is always available.

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
Two peers mutate independently; sync converges deterministically.

### 3.3 Policy Annotations

Other data structure types are not new primitives — they are chains or ledgers with constraints:

| Type | Realization |
|---|---|
| Metadata / index | Ledger + `derived` (rebuildable from other primitives, skipped during merge) |
| Immutable store | Ledger + `write-once` (key = content hash, no overwrite) |
| Ephemeral state | Chain or Ledger + `local-only` (excluded from push/fetch) |
| Conflict record | Ledger with keys `base`, `left`, `right` + `.db-type` = `conflict` |

### 3.4 Recursive Composition

Primitive values can themselves be primitives.
A ledger value can be a chain.
A chain entry can contain a ledger.
Recursion bottoms out at opaque blobs (application-defined content).
A `.db-type` marker blob at each typed subtree root tells the merge dispatcher which strategy to apply.

## 4. Transaction

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

## 5. State Tree Layout

```text
refs/db/<name>  →  commit (transaction N)
                     └── tree (root state)
                         ├── .db-type             → blob: "db-root"
                         ├── .db/                  → self-hosted metadata
                         │   ├── .db-type          → blob: "ledger"
                         │   ├── schema-version    → blob: "0.1"
                         │   ├── type-registry/    → ledger: type → merge strategy
                         │   └── annotations/      → ledger: path → policy
                         ├── <user-defined>/
                         │   ├── .db-type          → blob: "ledger" | "chain"
                         │   └── ...
                         └── ...
```

The `.db/` subtree is a ledger like any other — self-hosted, versioned, auditable.
The first transaction on `db init` writes it.
No external config files.

Nested chains use embedded representation (Option A): entries are subtrees within the parent, named by sequence number.
History is recovered from the enclosing transaction chain.
This preserves git reachability for GC and clone.

## 6. Merge

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

1. Same OID in left and right → keep (no change, or identical change).
2. Changed in one fork only → take the change.
3. Changed in both → read `.db-type`, dispatch to registered strategy.

Strategies are registered per type marker in `StrategyMap`.
Built-in strategies for chain (causal interleave) and ledger (key-by-key).
Applications provide custom strategies for domain-specific types.

Recursive: if a conflicting entry is a typed subtree, the dispatcher recurses.
Conflicts at leaves propagate upward as conflict records.

## 7. Incremental Computation

Derived state (indexes, caches, computed summaries) uses demand-driven invalidation keyed on subtree OIDs.

**Dependency table** (stored in optional local SQLite cache, not in git):

```text
derived_key → [(input_path, last_seen_oid), ...]
```

**On transaction commit:** diff old and new root trees → set of changed paths → scan dependency table → mark stale derived values.

**On read of stale derived value:** recompute from current inputs, cache result, update dependency table.

This is Salsa's red-green algorithm mapped onto Merkle structure.
The subtree OID is the revision identifier.
Cache invalidation is a hash comparison, not a tree walk.

## 8. CLI Plumbing Commands

`git db` provides the CLI equivalent of every library operation, following git's convention of low-level plumbing commands that porcelains compose.

### 8.1 Database Lifecycle

```text
git db init <name>
    Create a new database. Writes .db/ metadata subtree, creates refs/db/<name>.

git db list
    List all databases in the repository (enumerate refs/db/*).

git db drop <name>
    Delete a database (remove refs/db/<name>, objects GC'd normally).
```

### 8.2 Transaction Commands

```text
git db tx begin <name>
    Start a transaction. Prints a transaction ID (the snapshot OID).
    Writes transaction state to .git/db-tx/<txid>.

git db tx get <txid> <path>
    Read a value. Prints blob content to stdout.

git db tx put <txid> <path> [--stdin | --file=<f> | <literal>]
    Stage a write.

git db tx delete <txid> <path>
    Stage a deletion.

git db tx append <txid> <path> [--stdin | --file=<f> | <literal>]
    Stage a chain append.

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

### 8.3 History and Inspection

```text
git db log <name> [-n <count>]
    Print transaction history (commit log of refs/db/<name>).

git db show <name> [<path>]
    Print current state at path. Without path, prints root tree.

git db diff <name> <oid-a> <oid-b> [<path>]
    Diff two states. Output is path-level: added/modified/deleted keys.

git db cat <name> <oid>
    Print raw content of an object by OID.
```

### 8.4 Merge

```text
git db merge <name> <left-oid> <right-oid> [--strategy=<s>]
    Three-way merge. Prints result OID and conflict summary.
    Strategies loaded from .db/type-registry or --strategy flag.

git db conflicts <name> <oid>
    List unresolved conflicts in a merge result.

git db resolve <name> <oid> <path> [--take=left|right|base] [--value=<v>]
    Resolve a single conflict. Produces a new state OID.
```

### 8.5 Schema and Types

```text
git db type register <name> <type-name> [--merge-strategy=<s>]
    Register a type marker and its merge strategy.

git db type list <name>
    List registered types and strategies.

git db annotate <name> <path> <annotation>
    Set a policy annotation (derived, write-once, local-only) on a path.
```

## 9. Usage Example: forge

**forge** is a local-first issue and code-review tracker stored entirely in git.
It is the primary reference porcelain for git-db and drives all API design decisions.
Its data requirements are more complex than toy examples: threaded comments anchored to git objects, stateful reviews with per-commit approvals, and derived indexes for fast lookup.

### 9.1 State Tree Layout

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
    │       └── <entry>/     → tree (not a flat blob; see §9.2)
    │           ├── body     → blob (comment text)
    │           ├── anchor   → blob: "<oid>[:<start>-<end>]"
    │           ├── id       → blob: UUID v7 (stable comment identity across edits)
    │           ├── resolved → blob: "true" (absent = unresolved)
    │           ├── reply-to → blob: parent comment UUID (absent = top-level)
    │           └── replaces → blob: prior comment UUID (absent = original)
    ├── contributors/        → ledger (keyed by UUID v7)
    │   └── <uuid>/
    │       ├── handle       → blob
    │       ├── names/       → ledger
    │       ├── emails/      → ledger
    │       ├── keys/        → ledger (GPG/SSH key fingerprints)
    │       └── roles/       → ledger
    ├── config/              → ledger
    │   └── providers/
    │       └── github/      → provider-specific config blobs
    └── index/               → derived ledger (annotation: derived)
        ├── issues-by-display-id/   → display-id string → issue OID
        ├── reviews-by-display-id/  → display-id string → review UUID
        └── comments-by-object/     → object OID → space-separated thread UUIDs
```

**GC note:** In the current forge implementation, reviews manually track referenced blob OIDs under an `objects/` subtree to prevent garbage collection.
Under git-db this is unnecessary: any OID stored as a blob value in the transaction tree is reachable from `refs/db/forge` and therefore GC-safe.
The `objects/` workaround must be removed when forge is migrated.

**Chain entry structure:** Comment chain entries are trees, not flat blobs.
Each entry's tree contains the body blob and metadata blobs for anchor, identity, threading, and resolution.
This is the general pattern: chain entries are `Value::tree()`, and git-db's `tx.append` must accept a tree value, not only a byte payload.

### 9.2 Porcelain Operations (bash)

The forge porcelain storage layer in ~60 lines, built entirely on `git db`:

```bash
#!/bin/bash

forge_issue_new() {
    local title="$1" body="$2"
    local oid=$(echo -n "$title" | git hash-object --stdin)
    local txid=$(git db tx begin forge)
    echo "$title" | git db tx put "$txid" "issues/$oid/title" --stdin
    echo "open"   | git db tx put "$txid" "issues/$oid/state" --stdin
    [ -n "$body" ] && echo "$body" | git db tx put "$txid" "issues/$oid/body" --stdin
    git db tx commit "$txid" --message="open issue $oid"
    echo "$oid"
}

forge_issue_close() {
    local oid="$1"
    local txid=$(git db tx begin forge)
    echo "closed" | git db tx put "$txid" "issues/$oid/state" --stdin
    git db tx commit "$txid" --message="close issue $oid"
}

forge_comment_new() {
    local thread_uuid="$1" anchor="$2" body="$3" reply_to="$4"
    local comment_uuid=$(uuidgen)
    local object_oid="${anchor%%:*}"   # strip optional line range
    local txid=$(git db tx begin forge)
    # Entry is a tree of named blobs, not a flat blob.
    git db tx append "$txid" "comments/$thread_uuid" \
        --tree "body=$body" "anchor=$anchor" "id=$comment_uuid" \
               ${reply_to:+"reply-to=$reply_to"}
    # Maintain derived index inline (replaced by §7 incremental computation later).
    local current=$(git db tx get "$txid" "index/comments-by-object/$object_oid" 2>/dev/null || true)
    echo "${current:+$current }$thread_uuid" \
        | git db tx put "$txid" "index/comments-by-object/$object_oid" --stdin
    git db tx commit "$txid" --message="comment $comment_uuid on $object_oid"
    echo "$comment_uuid"
}

forge_review_approve() {
    local review_uuid="$1" commit_oid="$2" contributor_uuid="$3"
    local txid=$(git db tx begin forge)
    echo "" | git db tx put "$txid" \
        "reviews/$review_uuid/approvals/$commit_oid/$contributor_uuid" --stdin
    git db tx commit "$txid" --message="approve review $review_uuid at $commit_oid"
}

forge_issue_list() {
    git db show forge issues/ | while read oid; do
        local title=$(git db show forge "issues/$oid/title")
        local state=$(git db show forge "issues/$oid/state")
        local did=$(git db show forge "issues/$oid/display-id" 2>/dev/null || echo "$oid")
        printf "%s\t%s\t%s\n" "$did" "$state" "$title"
    done
}

forge_comments_for_object() {
    local object_oid="$1"
    local threads=$(git db show forge "index/comments-by-object/$object_oid")
    for thread_uuid in $threads; do
        git db tx log $(git db tx begin forge) "comments/$thread_uuid"
    done
}

forge_sync() {
    git push origin refs/db/forge
    git fetch origin refs/db/forge:refs/db/forge
}
```

This is the entire storage layer for a local-first issue and review tracker.
Full transaction history, atomic mutations, offline support, and sync via standard git push/fetch.
A developer building this never thinks about tree objects, ref CAS, or packfiles.

## 10. Usage from Rust

The same porcelain as a library.
This is how forge's `Store` is implemented on top of git-db.

```rust
use git_db::{Db, Value};

fn create_issue(db: &Db, title: &str, body: Option<&str>) -> Result<String> {
    // Issue OID is the SHA1 of the title blob — deterministic, content-addressed.
    let oid = git_hash_blob(title.as_bytes());
    let mut tx = db.transaction()?;
    tx.put(&["issues", &oid, "title"], Value::from(title))?;
    tx.put(&["issues", &oid, "state"], Value::from("open"))?;
    if let Some(b) = body {
        tx.put(&["issues", &oid, "body"], Value::from(b))?;
    }
    tx.commit(meta("open issue"))?;
    Ok(oid)
}

fn close_issue(db: &Db, oid: &str) -> Result<()> {
    let mut tx = db.transaction()?;
    tx.put(&["issues", oid, "state"], Value::from("closed"))?;
    tx.commit(meta("close issue"))?;
    Ok(())
}

fn add_comment(
    db: &Db,
    thread_uuid: &str,
    anchor: &str,        // "<blob-oid>[:<start>-<end>]"
    body: &str,
    reply_to: Option<&str>,
) -> Result<String> {
    let comment_id = uuid::Uuid::now_v7().to_string();
    let object_oid = anchor.split(':').next().unwrap(); // strip optional line range

    // Chain entry is a structured tree, not a flat blob.
    let mut entry = Value::tree();
    entry.insert("body", Value::from(body));
    entry.insert("anchor", Value::from(anchor));
    entry.insert("id", Value::from(comment_id.as_str()));
    if let Some(r) = reply_to {
        entry.insert("reply-to", Value::from(r));
    }

    let mut tx = db.transaction()?;
    tx.append(&["comments", thread_uuid], entry)?;

    // Maintain derived index inline.
    // Once §7 incremental computation is available, annotate index/ as `derived`
    // and register a rebuild callback instead of doing this manually.
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

fn approve_review(
    db: &Db,
    review_uuid: &str,
    commit_oid: &str,
    contributor_uuid: &str,
) -> Result<()> {
    let mut tx = db.transaction()?;
    // Presence-as-membership: the key existing is the fact; value is empty.
    tx.put(
        &["reviews", review_uuid, "approvals", commit_oid, contributor_uuid],
        Value::empty(),
    )?;
    tx.commit(meta("approve review"))?;
    Ok(())
}

fn list_issues(db: &Db) -> Result<Vec<Issue>> {
    let tx = db.transaction()?;
    let oids = tx.list(&["issues"])?;
    oids.iter().map(|oid| {
        Ok(Issue {
            oid: oid.clone(),
            title: tx.get(&["issues", oid, "title"])?.unwrap().to_string(),
            state: tx.get(&["issues", oid, "state"])?.unwrap().to_string(),
            display_id: tx.get(&["issues", oid, "display-id"])?
                .map(|v| v.to_string()),
        })
    }).collect()
}

fn comments_for_object(db: &Db, object_oid: &str) -> Result<Vec<Comment>> {
    let tx = db.transaction()?;
    let thread_list = tx.get(&["index", "comments-by-object", object_oid])?
        .map(|v| v.to_string())
        .unwrap_or_default();
    let mut comments = Vec::new();
    for thread_uuid in thread_list.split_whitespace() {
        for entry in tx.log(&["comments", thread_uuid])? {
            // Each entry is a Value::tree(); access fields by name.
            comments.push(Comment {
                id: entry.get("id").unwrap().to_string(),
                anchor: entry.get("anchor").unwrap().to_string(),
                body: entry.get("body").unwrap().to_string(),
                resolved: entry.get("resolved")
                    .map(|v| v.to_string() == "true")
                    .unwrap_or(false),
                reply_to: entry.get("reply-to").map(|v| v.to_string()),
                replaces: entry.get("replaces").map(|v| v.to_string()),
            });
        }
    }
    Ok(comments)
}
```

### Requirements surfaced by forge

These operations expose concrete requirements that drive git-db's API design:

1. **Structured chain entries.**
   `tx.append` must accept `Value::tree()`, not only a flat blob.
   Forge comment entries carry body, anchor, stable ID, threading, and resolution state as separate named blobs within the entry tree.

2. **Deep path put.**
   `tx.put(&["reviews", uuid, "approvals", commit_oid, contributor_uuid], ...)` requires creating intermediate tree nodes on demand.
   The transaction layer handles this; callers do not manage tree construction.

3. **`Value::empty()`.**
   Presence-as-membership (labels, assignees, approval entries) requires a valid empty blob value distinct from "key absent."

4. **`Value::tree()` with named field access.**
   `tx.log` returns an iterator of `Value`; for chain entries that are trees, callers must access fields by name (`entry.get("body")`).
   The `Value` type must support both blob and tree variants with a uniform access API.

5. **Automatic GC safety.**
   Any OID stored as blob content in the transaction tree is reachable from `refs/db/forge` and protected from GC.
   Forge's current manual `objects/` subtree (present in `reviews/<uuid>/objects/`) is unnecessary under git-db and must be removed when forge is migrated.

6. **Derived index annotation.**
   The `index/` subtree is annotated `derived`: excluded from merge, rebuildable from primary data. git-db's incremental computation (§7) should observe mutations to `issues/`, `reviews/`, and `comments/` and invoke registered rebuild callbacks.
   Until §7 is implemented, callers maintain the index inline (as shown above).

## 11. What git-db Replaces

| Current approach | Problem | git-db equivalent |
|---|---|---|
| git-bug, git-appraise | Ad-hoc ref conventions, custom merge, can't share infrastructure | Porcelain on `git db` |
| YAML/JSON config in git | Text merge on structured data, broken merges | Ledger with key-level merge |
| Terraform state in git | No transactions, race conditions on concurrent apply | Transaction with CAS |
| DVC metadata | Custom sidecar format, separate sync | Chain + ledger in same repo |
| CODEOWNERS | Flat file, no history, no audit | Ledger with append-only audit chain |
| Package lockfiles | Constant merge conflicts on structured data | Ledger with package-name keys, auto-merge |
| git-notes | Single-key-per-object, no nesting, poor merge | Ledger with arbitrary structure |
| gitops state stores | Ad-hoc conventions, fragile CI scripts | Transaction-safe state with merge |

## 12. What git-db Does Not Do

- **No query language.**
  Reads are path lookups, not planned queries.
  Build a query layer on top if you need one.
- **No schema enforcement.**
  Tree structure is by convention.
  Build a schema validator on top if you need one.
- **No working directory.**
  There is no checkout.
  The state tree exists only as git objects.
- **No text diff/merge.**
  The text porcelain (`git diff`, `git merge`) still handles source code. git-db handles structured data.
- **No new protocols.**
  Sync is `git push` / `git fetch`.
  Auth is whatever your git remote uses.
- **No hosted service.**
  git-db is plumbing.
  Hosted services (forges, CI, etc.) are porcelain built on top.

## 13. Relationship to Existing Work

**git plumbing.**
`git-db` is a strict superset of a subset of git plumbing.
It composes `hash-object`, `mktree`, `write-tree`, `update-ref` into higher-level operations.
It does not replace or modify any existing git behavior.

**Irmin.**
Closest prior art.
OCaml library for mergeable, branchable, content-addressed stores.
`git-db` differs in being git-native (no custom format), Rust, CLI-accessible, and reduced to two primitives rather than Irmin's larger API surface.

**Noms / Dolt.**
Content-addressed versioned databases with prolly trees and SQL.
`git-db` operates at a lower layer — it provides the primitives that a system like Noms would implement, without prescribing a query interface or storage format.

**DeltaDB (Zed).**
Operation-based version control with CRDTs for real-time collaboration.
DeltaDB targets character-level edit tracking for collaborative coding.
`git-db` targets general structured data.
DeltaDB is a product; `git-db` is plumbing.

**jj.**
Version control porcelain with operation log and conflict-as-data. jj could be implemented as a porcelain on `git-db` (operation log as a chain, change-id map as a ledger, branch pointers as ledger entries or real git refs).
Whether the performance characteristics would be acceptable is an open question.

**Local-first / CRDTs.**
`git-db`'s chain is a G-Set and its ledger with LWW per key is a conflict-free OR-Map.
A local-first framework could use `git-db` as its persistence and sync layer.
The community is fragmented across incompatible implementations; `git-db` offers a git-native option.

## 14. Performance

**Reads:** 4–5 object lookups per path (ref → commit → root tree → subtree → entry).
Packfile memory-mapped.
Microseconds.

**Writes:** one tree object per modified subtree level + one commit + one CAS.
A transaction touching M disjoint keys in a tree of depth D: M×D tree writes + 1 commit + 1 CAS.
Typically under a kilobyte total.

**Sync:** push/fetch of one ref.
Bandwidth proportional to state delta, not total size.

**Scaling:** hierarchical key sharding for ledgers exceeding ~10k entries.
Optional SQLite read cache for workloads requiring indexed queries or prefix scans.
Periodic flush mode with WAL for high-frequency mutation batching.

These are the performance characteristics of a small-to-medium structured data store.
`git-db` is not competing with SQLite on throughput.
It is competing with "ad-hoc YAML in a git repo" on correctness, and winning.

## 15. Development Plan

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
Conflict representation.
Recursive merge.
`git db merge`, `git db conflicts`, `git db resolve`.
Property tests and fuzz testing.

### Phase 4: forge as Reference Porcelain (2–3 weeks)

**forge** is the primary example porcelain.
Do not build a toy example — migrate forge itself. forge's full data model is specified in §9–10; those sections are the authoritative requirements for this phase.

Deliverables:

- Rewrite `crates/git-forge/src/store.rs` and its entity modules (`issue.rs`, `review.rs`, `comment.rs`, `contributor.rs`) to use `git_db::Db` and `Tx` instead of direct `git2` + `git-ledger` + `git-chain` calls.
- Remove `crates/git-forge/src/index.rs` manual rebuild logic; replace with inline index maintenance as shown in §10 (incremental computation from §7 is not required at this phase).
- Remove the `objects/` GC workaround subtree from reviews; GC safety is automatic under git-db structural reachability.
- All existing integration tests in `crates/git-forge/tests/` must pass unchanged.

Success criterion: the forge MCP server (`crates/forge-mcp`) operates correctly against the new storage layer with no changes to its public tool API.

### Phase 5: Incremental Computation (2–3 weeks)

Dependency table.
Demand-driven invalidation.
SQLite read cache with incremental rebuild.

### Phase 6: Documentation and Stabilization (1–2 weeks)

Crate docs.
Man pages for CLI commands.
Tutorial: "Build a porcelain on git-db."
Specification: tree layout, type markers, merge contracts.

---

**Total estimated timeline: 13–20 weeks to 0.1 release.**

Critical path is Phase 3 (merge).
Minimum viable release (no merge, no incremental computation): Phases 0–2, approximately 6–8 weeks.
This is enough for single-writer porcelains that sync via push/fetch and don't need concurrent offline edits.
