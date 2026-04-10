# git-store Development Plan

**Author:** Joey Carpinelli **Date:** April 2026 **Status:** Draft

---

## Scope

This plan covers the path from zero lines of code to a 0.1 release of `git-store`: a Rust library and CLI plumbing for transactional, typed, structured data over git objects and refs.

The plan is sequenced by dependency, not importance.
Each phase produces a testable artifact. forge migration is the validation gate — if forge can't be rebuilt on git-store without escape hatches, the primitives are wrong.

### Primitives

Three primitives.
Everything else is a policy or convention on top of these.

- **Ledger** — keyed mutable map.
  Entities, config, any record-by-ID access.
- **Chain** — ordered append-only log (DAG with one tip).
  Comments, activity feeds, op logs.
- **Metadata** — object → key → value annotations.
  Relations, type registry, policy flags.

No derived indexes.
Secondary key lookups (e.g., display-id → UUID) are local-only caches under `refs/db-local/`, rebuilt from a full scan when stale, never pushed.

Type information and merge strategy mappings live in `git-metadata` annotations on the store ref, not in a self-hosted `.db/` subtree.
The state tree is pure user data.

---

## Phase 0: Foundation Traits

**Duration:** 1–2 weeks **Depends on:** nothing **Output:** `git-store` crate with `ContentAddressable` and `Pointer` impls on gix

### Work

- Implement `ContentAddressable` over `gix::Repository` (blob/tree/commit read/write).
- Implement `Pointer` over gix ref transactions (read, CAS with expected OID).
- Property tests for the three `ContentAddressable` laws (determinism, round-trip, referential transparency).
- Property tests for the three `Pointer` laws (atomicity, linearizability, consistency).

### Done when

`cargo test` passes with a tmpdir git repo, exercising store/retrieve round-trips and concurrent CAS contention.

---

## Phase 1: Transaction

**Duration:** 2–3 weeks **Depends on:** Phase 0 **Output:** `Store`, `Tx` structs; get/put/delete/list/commit

### Work

- `Store::open` / `Store::init` — open or create a store at `refs/db/<n>`.
- `Tx` — snapshot current state on begin, accumulate mutations in memory, write new tree objects bottom-up on commit, CAS the ref.
- Structural sharing: only modified subtree paths produce new tree objects.
- Retry loop on CAS failure (configurable max retries).
- `get(path)`, `put(path, value)`, `delete(path)`, `list(path)` — path is `&[&str]`, traversal through nested trees.
- Error types: `thiserror`-based error enum in the library crate; CLI binary uses `anyhow`.

### Done when

Integration test: init a store, open a transaction, write 10 keys across 3 subtree levels, commit, re-read all keys, verify round-trip.
Second test: two concurrent transactions to the same store, one wins, one retries.

---

## Phase 2: Chain Primitive

**Duration:** 1–2 weeks **Depends on:** Phase 1 **Output:** `append(path, entry)`, `log(path)` on `Tx`

### Work

- Chain representation: entries as sequentially-named subtrees within the parent tree.
  Entry N is a tree containing user-defined blobs; blobs may be OIDs pointing to other ledgers or chains.
- `append` adds the next entry to the in-memory buffer.
- `log` iterates entries in order by recovering history from the enclosing transaction commit chain.
- Chain entries are immutable once committed — append-only enforced by the API.

### Done when

Integration test: append 100 entries to a chain, read them back in order, verify content and ordering.

---

## Phase 3: Metadata Integration

**Duration:** 1 week **Depends on:** Phase 1, existing `git-metadata` crate **Output:** type registry and policy annotations stored as `git-metadata` on the store ref

### Work

- On `Store::init`, write type and merge strategy mappings as `git-metadata` annotations on the store ref.
- Format: type name → glob pattern + merge strategy name.
- Relations are metadata annotations on objects within the store: `source object → relation name → target object`.
  Bidirectional consistency enforced at write time (both directions written in the same transaction).
- No `.db/` subtree.
  No marker blobs.
  The state tree is pure user data.

### Done when

Type registry entries can be written and read through `git-metadata`.
A relation between two objects can be written and queried from either side.

---

## Phase 4: CLI Plumbing

**Duration:** 2–3 weeks **Depends on:** Phases 1–3 **Output:** `git store` subcommands for basic operations

### Commands

```text
git store init <n>
git store list
git store drop <n>
git store tx begin <n>         → prints txid (snapshot OID)
git store tx get <txid> <path>
git store tx put <txid> <path> [--stdin | --file=<f> | <literal>]
git store tx delete <txid> <path>
git store tx append <txid> <path> [--stdin | --file=<f> | --tree <k=v>...]
git store tx log <txid> <path>
git store tx list <txid> <path>
git store tx commit <txid> [--message=<msg>] [--author=<a>]
git store tx abort <txid>
git store show <n> [<path>]
git store log <n> [-n <count>]
```

### Work

- Transaction state in `.git/store-tx/<txid>` — ephemeral, deleted on commit/abort.
- stdin/stdout conventions, exit code 0 on success, 1 on CAS contention.
- `--tree` flag on `append` accepts `key=value` pairs for structured chain entries.

### Done when

A shell script can create a store, run a transaction, write and read values, append to a chain, and inspect history — all through the CLI.

---

## Phase 5: Merge

**Duration:** 3–5 weeks **Depends on:** Phases 1–3 **Output:** `merge()` function, `MergeStrategy` trait, built-in strategies, conflict representation

This is the critical path.

### Work

- `MergeStrategy` trait: `fn merge(base, left, right) -> MergeResult`.
- Merge dispatcher: single-threaded three-way walk of base/left/right trees.
  At each node:
  - Same OID → keep.
  - Changed in one side only → take.
  - Changed in both → path-prefix lookup in metadata registry → dispatch to strategy.
- Built-in strategies:
  - **LWW** (last-writer-wins): take the side with the later timestamp.
    Tiebreak on OID.
  - **Set merge**: union of keys (presence-as-membership ledger).
  - **Causal interleave**: for chains.
    Linearize entries from both forks by Lamport timestamp, content-hash tiebreak, collapse duplicates.
  - **Preserve-DAG**: for chains where fork structure is meaningful (e.g., op log).
    Merge commit with both heads as parents, no linearization.
- Recursive merge: if a conflicting entry is a typed subtree, recurse.
- Conflict representation: conflict record at the leaf with `base`, `left`, `right` values.
- `StrategyMap`: metadata-backed for CLI, trait-backed for Rust consumers.
  Dispatcher doesn't distinguish.
- No derived index rebuild.
  That problem is gone.

### CLI additions

```text
git store merge <n> <left-oid> <right-oid> [--strategy=<s>]
git store conflicts <n> <oid>
git store resolve <n> <oid> <path> [--take=left|right|base] [--value=<v>]
```

### Done when

- Two-fork merge of a ledger with disjoint keys auto-merges cleanly.
- Two-fork merge of a ledger with same-key conflict produces a conflict record.
- Two-fork merge of a chain produces a causal interleave.
- Property tests: merge(left, right) and merge(right, left) produce the same result (commutativity). merge(x, x) = x (idempotency).
- Fuzz testing on randomly generated state trees.

---

## Phase 6: Facet Integration

**Duration:** 2 weeks **Depends on:** Phases 3, 5 **Output:** `GitStoreType` derive macro, automatic serialization/deserialization/strategy derivation

### Work

- `#[derive(GitStoreType)]` walks facet `SHAPE` to produce:
  - Tree layout (struct fields → named blobs, nested structs → subtrees).
  - Path → strategy mapping from `#[facet(merge = "...")]` attributes.
  - Relation declarations from `#[store(relations = ["blocks", "parent-of"])]`.
- On `Store::init`, registered `GitStoreType` impls write their mappings to `git-metadata` on the store ref.
- Default strategy derivation: named struct → ledger (field-by-field), `Vec<T>` → chain (causal interleave), `Option<T>` → LWW, scalar → LWW.
- `serialize` / `deserialize` between Rust types and git tree/blob structures.

### Done when

A `#[derive(GitStoreType)]` struct can be written to and read from a `Tx` without manual tree construction.
Merge strategies are derived from the type definition and match hand-written equivalents.

---

## Phase 7: forge Migration

**Duration:** 2–3 weeks **Depends on:** Phases 1–6 **Output:** forge's storage layer rebuilt on `git_store::Store` and `Tx`

This is the validation gate.
If forge can't be cleanly expressed, the primitives need revision.

### Work

- Replace `store.rs`, `issue.rs`, `review.rs`, `comment.rs`, `contributor.rs` internals with `git_store` calls.
- Remove `objects/` GC workaround subtree (git-store's tree reachability handles GC).
- Secondary key lookups (display-id → UUID) become local-only caches under `refs/db-local/`, rebuilt from full scan when missing.
- Issue relations (blocks, duplicates, parent-of) stored as `git-metadata` annotations, bidirectional, written atomically in the same transaction.
- Define `ForgeIssue`, `ForgeComment`, `ForgeReview`, `ForgeContributor` as `#[derive(GitStoreType)]` structs.
- All existing integration tests in `crates/git-forge/tests/` must pass unchanged.
- forge MCP server operates correctly with no changes to its public tool API.

### What to watch for

- Any place forge needs to escape the `Tx` API and touch raw git objects directly — that's a primitive gap.
- Any place the derive macro can't express forge's actual merge semantics — that's a facet integration gap.
- Comment chains: verify that embedded representation handles thread traversal without performance issues at ~1000 comments per thread.

### Done when

`cargo test` in `crates/git-forge/` passes.
No raw git plumbing calls remain in forge's storage layer.

---

## Phase 8: Query Layer

**Duration:** 2–3 weeks **Depends on:** Phase 1 **Output:** Path algebra evaluator, `git store query`

### Work

- Path algebra: `issues/*/state`, `issues/*/[state="open"]`, `reviews/*/approvals/*/*`.
- Evaluator: subtree enumeration, blob predicate filtering, subpath projection.
- OID-keyed result cache: track input subtree OIDs per query, short-circuit on match.
- `git store query <n> <pattern> [--where <path>=<value>] [--select <path>]`
- `git store query <n> --explain <pattern>` — show query plan.

No index-aware planning.
Queries scan the relevant subtree.
For forge-scale data this is fast enough.
If a consumer needs faster lookups, they maintain a local cache.

### Done when

`git store query forge "issues/*/[state=\"open\"]/title"` returns open issue titles.

---

## Phase 9: Documentation and Stabilization

**Duration:** 1–2 weeks **Depends on:** all prior phases **Output:** crate docs, man pages, tutorial, specification

### Work

- Rustdoc for all public types and traits.
- Man pages for every `git store` subcommand.
- Tutorial: "Build a porcelain on git-store" — walk through creating a minimal app from `Store::init` to merge.
- Specification document: tree layout, metadata conventions, merge contracts, query algebra syntax.
- Review all `// TODO` and `// HACK` comments; resolve or file issues.

---

## Timeline Summary

| Phase | Duration | Cumulative |
|-------|----------|------------|
| 0: Foundation Traits | 1–2 weeks | 1–2 weeks |
| 1: Transaction | 2–3 weeks | 3–5 weeks |
| 2: Chain Primitive | 1–2 weeks | 4–7 weeks |
| 3: Metadata Integration | 1 week | 5–8 weeks |
| 4: CLI Plumbing | 2–3 weeks | 7–11 weeks |
| 5: Merge | 3–5 weeks | 10–16 weeks |
| 6: Facet Integration | 2 weeks | 12–18 weeks |
| 7: forge Migration | 2–3 weeks | 14–21 weeks |
| 8: Query Layer | 2–3 weeks | 16–24 weeks |
| 9: Docs & Stabilization | 1–2 weeks | 17–26 weeks |

**With heavy agent use:** ~3–5 weeks calendar time.
Bottleneck is Phase 5 (merge) and design decisions during Phase 7 (forge migration).

**Minimum viable release (single-writer, no concurrent merge):** Phases 0–4. ~7–11 weeks human-solo, ~1–2 weeks with agents.

**Critical path:** Phase 5 (merge).
Start design work and property test scaffolding during Phase 2.

---

## Open Questions

1. **Chain representation.**
   Embedded subtrees vs. commit-chain per chain instance.
   Decide during Phase 2 by benchmarking forge comment thread traversal.

2. **Chain merge strategy selection.**
   Causal interleave is the default.
   Preserve-DAG is needed for gin's op log.
   Confirm the strategy is selectable per-chain via the same metadata registry mechanism.

3. **Local-only refs.**
   `refs/db-local/<n>` for secondary key caches.
   Needed during Phase 7 for forge display-id lookups.
   Simple — just a ref that's excluded from push/fetch.

4. **gin as second consumer.**
   Schedule a spike (1 week) after Phase 7 to sketch gin's op log and change-ID map on git-store.
   If it doesn't fit, the primitives need revision before 0.1.
