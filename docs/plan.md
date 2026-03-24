+++
title = "git-data"
subtitle = "Implementation Plan"
date = 2026-03-21
+++

# Implementation Plan

## Current State

All three crates are implemented with CLIs and published in the workspace.

- **git-metadata** (v0.2.1) — OID-keyed metadata (list, get, add, remove, copy, prune) with CLI.
  Relation operations (link, unlink, linked, is_linked) are implemented.
- **git-ledger** (v0.1.0) — Versioned records stored as refs with CLI (`git ledger`).
  Supports create, read, update, list, and history operations.
- **git-chain** (v0.1.0) — Append-only event chains stored as commit history with CLI (`git chain`).
  Supports append and chain walking operations.

Workspace wiring is complete: all three crates are members of the workspace `Cargo.toml`, `git2` and `tempfile` versions are shared via workspace dependencies, and CI covers all crates.

---

## Completed Phases

### Phase 1 — git-metadata: Relation Operations ✓

Relation operations are implemented in `git-metadata`.

#### Data model

Three-level tree under any metadata ref:

```text
<ref> → commit → tree
  <key>/
    <relation>/
      <target>          # blob: empty or optional metadata
```

Keys use `:` as an internal delimiter (`issue:42`, `commit:abc123`).
Git prohibits `:` in ref names but allows it in tree entry names.

#### API surface (on `MetadataIndex` trait)

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

#### Implementation notes

- `link` and `unlink` write both directions in a single commit (one tree mutation, one `git2::Commit`).
  This is the atomicity guarantee.
- Tree path: `insert_path_into_tree` already exists; reuse for `<key>/<rel>/<target>`.
- Empty blob (`e69de29`) is the default metadata value; reuse `git2::Repository::blob`.
- Fanout is not applied to relation keys — keys are human-readable short strings.
- Concurrency: two writers linking disjoint pairs touch disjoint tree paths; three-way merge resolves them.
  Conflict = same link written simultaneously → reject and retry (same pattern as existing metadata writes).

#### CLI additions to `git-metadata`

```text
git metadata link   <a> <b> --forward <label> --reverse <label> [--ref <ref>]
git metadata unlink <a> <b> --forward <label> --reverse <label> [--ref <ref>]
git metadata linked <key>   [--relation <label>]                 [--ref <ref>]
```

### Phase 2 — git-ledger ✓

**Crate:** `crates/git-ledger/` **Type:** library + CLI (`git-ledger`, invoked as `git ledger`) **Version:** 0.1.0

#### Ref structure

```text
refs/<namespace>/<id> → commit → tree
  <field>               # blob
  <subdir>/
    <field>             # blob
```

Each record is its own ref.
Two writers on different records never conflict.

#### Public API

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

#### CLI

```text
git ledger create <ref-prefix> [<id>] [--sequential | --content-hash] --set key=value ...
git ledger read   <ref>
git ledger update <ref> --set key=value ... --delete key ...
git ledger list   <ref-prefix>
```

### Phase 3 — git-chain ✓

**Crate:** `crates/git-chain/` **Type:** library + CLI (`git-chain`, invoked as `git chain`) **Version:** 0.1.0

#### Model

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

#### Public API

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

#### CLI

```text
git chain append <ref> [-m <message>] [--parent <commit>] [--payload <path>]...
git chain walk   <ref> [--thread <commit>]
```

### Phase 4 — Workspace Wiring ✓

1. `git-ledger` and `git-chain` added to `Cargo.toml` workspace members.
2. `git2` and `tempfile` versions shared via workspace `[dependencies]`.
3. CI matrix entries added for all crates.
4. Integration test helpers available per-crate.

---

## Sequencing (completed)

| Phase | Deliverable | Status |
|-------|-------------|--------|
| 1 | `git-metadata` relation ops + CLI | ✓ Done |
| 2 | `git-ledger` library + CLI | ✓ Done |
| 3 | `git-chain` library + CLI | ✓ Done |
| 4 | Workspace wiring | ✓ Done |

---

## Out of Scope

- Transport, push/fetch, ref advertisement.
- Merge strategy selection.
- Derived query caching.
- Ephemeral data handling.
- A shared internal crate (defer until duplication is observed).
