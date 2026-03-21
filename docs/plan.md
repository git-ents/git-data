+++
title = "git-data"
subtitle = "Implementation Plan"
date = 2026-03-21
+++

# Implementation Plan

## Current State

Only `git-metadata` (v0.2.1) exists.
It implements OID-keyed metadata (list, get, add, remove, copy, prune) with CLI.
Two crates are missing entirely.
Relation operations are unimplemented.

---

## Phase 1 — git-metadata: Relation Operations

**Goal:** implement the `Relation` feature described in the design doc.

### Data model

Three-level tree under any metadata ref:

```text
<ref> → commit → tree
  <key>/
    <relation>/
      <target>          # blob: empty or optional metadata
```

Keys use `:` as an internal delimiter (`issue:42`, `commit:abc123`).
Git prohibits `:` in ref names but allows it in tree entry names.

### New API surface (added to `MetadataIndex` trait)

```rust
fn link(
    &self,
    ref_name: &str,
    a: &str,
    b: &str,
    forward: &str,
    reverse: &str,
    meta: Option<&[u8]>,
) -> Result<Oid>;

fn unlink(
    &self,
    ref_name: &str,
    a: &str,
    b: &str,
    forward: &str,
    reverse: &str,
) -> Result<Oid>;

fn linked(
    &self,
    ref_name: &str,
    key: &str,
    relation: Option<&str>,
) -> Result<Vec<(String, String)>>; // (relation, target)

fn is_linked(
    &self,
    ref_name: &str,
    a: &str,
    b: &str,
    forward: &str,
) -> Result<bool>;
```

### Implementation notes

- `link` and `unlink` write both directions in a single commit (one tree mutation, one `git2::Commit`).
  This is the atomicity guarantee.
- Tree path: `insert_path_into_tree` already exists; reuse for `<key>/<rel>/<target>`.
- Empty blob (`e69de29`) is the default metadata value; reuse `git2::Repository::blob`.
- Fanout is not applied to relation keys — keys are human-readable short strings.
- Concurrency: two writers linking disjoint pairs touch disjoint tree paths; three-way merge resolves them.
  Conflict = same link written simultaneously → reject and retry (same pattern as existing metadata writes).

### CLI additions to `git-metadata`

```text
git metadata link   <a> <b> --forward <label> --reverse <label> [--ref <ref>]
git metadata unlink <a> <b> --forward <label> --reverse <label> [--ref <ref>]
git metadata linked <key>   [--relation <label>]                 [--ref <ref>]
```

---

## Phase 2 — git-ledger (new crate)

**Crate:** `crates/git-ledger/` **Type:** library + CLI (`git-ledger`, invoked as `git ledger`) **Version:** 0.1.0

### Ref structure

```text
refs/<namespace>/<id> → commit → tree
  <field>               # blob
  <subdir>/
    <field>             # blob
```

Each record is its own ref.
Two writers on different records never conflict.

### Public API

```rust
pub trait Ledger {
    fn create(
        &self,
        ref_prefix: &str,        // e.g. "refs/issues"
        strategy: &IdStrategy,
        fields: &[(&str, &[u8])],
        message: &str,
    ) -> Result<LedgerEntry>;

    fn read(&self, ref_name: &str) -> Result<LedgerEntry>;           // full ref

    fn update(
        &self,
        ref_name: &str,           // full ref
        mutations: &[Mutation],
        message: &str,
    ) -> Result<LedgerEntry>;

    fn list(&self, ref_prefix: &str) -> Result<Vec<String>>;          // IDs
    fn history(&self, ref_name: &str) -> Result<Vec<Oid>>;            // commit chain
}

pub enum IdStrategy<'a> {
    Sequential,                 // scan refs, increment
    ContentAddressed(&'a [u8]), // hash of caller-supplied bytes
    CallerProvided(&'a str),    // opaque string
}

pub enum Mutation<'a> {
    Set(&'a str, &'a [u8]),  // upsert a field
    Delete(&'a str),         // remove a field
}

pub struct LedgerEntry {
    pub id:     String,
    pub ref_:   String,
    pub commit: Oid,
    pub fields: Vec<(String, Vec<u8>)>,
}
```

### Implementation notes

**Sequential naming:**

- `list(ref_prefix)` performs a prefix scan via `Repository::references_glob`.
- Parse IDs as `u64`; max + 1 is the candidate.
- `create` writes the new ref; if another writer raced, rescan and retry.
  No counter ref required for correctness.

**Content-addressed naming:**

- `IdStrategy::ContentAddressed(bytes)` → hash using git's object hash
  algorithm via `git2`.

**Caller-provided naming:**

- Pass through directly; validate with `git check-ref-format` rules.

**Attestation:**

- Any `create` or `update` commit can be GPG/SSH signed.
  Expose a `sign: bool` option on `create`/`update`; delegate to `git2`'s signing support.

**Dependencies:** `git2`, `clap` (already in workspace), nothing else.

### CLI

```text
git ledger create <ref-prefix> [<id>] [--sequential | --content-hash] --set key=value ...
git ledger read   <ref>
git ledger update <ref> --set key=value ... --delete key ...
git ledger list   <ref-prefix>
```

History is available via `git log <ref>` directly.

`--sequential` is the default when no `<id>` is given.
`<id>` is caller-provided.
`--content-hash` hashes stdin.

---

## Phase 3 — git-chain (new crate)

**Crate:** `crates/git-chain/` **Type:** library + CLI (`git-chain`, invoked as `git chain`) **Version:** 0.1.0

### Model

A chain is a ref where each commit is an event.
The commit chain is the ordering.
There is no accumulated tree — each commit's tree holds only that entry's payload.
The consumer decides what goes in the tree vs. the commit message.

```text
<ref> → commit C
         ├─ parent1: commit B  (chronological)
         ├─ parent2: commit X  (optional second parent)
         ├─ message: <consumer-defined>
         └─ tree: <consumer-defined payload>
```

Entries are never edited.
Corrections are new appends.

### Public API

```rust
pub trait Chain {
    fn append(
        &self,
        ref_name: &str,
        message: &str,
        tree: Oid,              // caller builds the tree
        parent: Option<Oid>,    // second parent
    ) -> Result<ChainEntry>;

    fn walk(
        &self,
        ref_name: &str,
        thread: Option<Oid>,    // None = full chain, Some = thread root
    ) -> Result<Vec<ChainEntry>>;
}

pub struct ChainEntry {
    pub commit:  Oid,
    pub message: String,
    pub tree:    Oid,
}
```

### Implementation notes

**Append:** create a commit whose first parent is the current ref tip.
The caller provides the tree OID.
If `parent` is set, it becomes the second parent.

**Walk (full chain):** follow first-parent links from tip to root.
Each commit yields one `ChainEntry`.
Returns reverse-chronological order.

**Walk (threaded):** starting from `thread` root, find all commits in the chain whose second parent is that commit.
Then recursively find replies to those.
Returns the full thread tree rooted at that commit.

**Concurrency:** two concurrent appenders diverge the ref.
On next write, a merge commit with two first-parents re-converges.
This is correct — the chain is a DAG event log.
DAG order with commit timestamps is sufficient for reconstruction.

**Attestation:** same signing option as ledger.

**Dependencies:** `git2`, `clap` (already in workspace), nothing else.

### CLI

```text
git chain append <ref> [-m <message>] [--parent <commit>] [--payload <path>]...
git chain walk   <ref> [--thread <commit>]
```

`-m` sets the commit message.
`--payload` adds a file or directory to the commit's tree (repeatable).
`--parent` sets the second parent.

---

## Phase 4 — Workspace Wiring

1. Add `git-ledger` and `git-chain` to `Cargo.toml` workspace members.
2. Share `git2` and `tempfile` versions via workspace `[dependencies]`.
3. Add CI matrix entries for new crates.
4. Add integration test helpers shared across crates (or inline per-crate).

---

## Sequencing

| Phase | Deliverable | Depends on |
|-------|-------------|------------|
| 1 | `git-metadata` relation ops + CLI | — |
| 2 | `git-ledger` library | Phase 4 (workspace) |
| 3 | `git-chain` library | Phase 4 (workspace) |
| 4 | Workspace wiring | — (can run in parallel with 1) |

Phases 1 and 4 can start immediately.
Phases 2 and 3 are independent of each other and can be implemented in parallel after 4 is done.

---

## Out of Scope

- Transport, push/fetch, ref advertisement.
- Merge strategy selection.
- Derived query caching.
- Ephemeral data handling.
- A shared internal crate (defer until duplication is observed).
