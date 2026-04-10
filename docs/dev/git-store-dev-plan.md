# git-store Development Plan

**Author:** Joey Carpinelli **Date:** April 2026 **Status:** Draft

---

## Scope

This plan covers the path from zero lines of code to a 0.1 release of `git-store`: a Rust library and CLI plumbing for transactional, typed, structured data over git objects and refs.

The plan is sequenced by dependency, not importance.
Each phase produces a testable artifact. forge migration is the validation gate — if forge can't be rebuilt on git-store without escape hatches, the primitives are wrong.

---

## Phase 0: Foundation Traits

**Duration:** 1–2 weeks **Depends on:** nothing **Output:** `git-store` crate with `ContentAddressable` and `Pointer` impls on gix

### Work

- Implement `ContentAddressable` over `gix::Repository` (blob/tree/commit read/write).
- Implement `Pointer` over gix ref transactions (read, CAS with expected OID).
- Fallback path for `Pointer`: shell out to `git update-ref --stdin` when gix doesn't support the ref backend (reftable).
- Property tests for the three `ContentAddressable` laws (determinism, round-trip, referential transparency).
- Property tests for the three `Pointer` laws (atomicity, linearizability, consistency).

### Done when

`cargo test` passes with a tmpdir git repo, exercising store/retrieve round-trips and concurrent CAS contention.

---

## Phase 1: Transaction

**Duration:** 2–3 weeks **Depends on:** Phase 0 **Output:** `Db`, `Tx` structs; get/put/delete/list/commit

### Work

- `Db::open` / `Db::init` — open or create a database at `refs/db/<name>`.
- `Tx` — snapshot current state on begin, accumulate mutations in memory, write new tree objects bottom-up on commit, CAS the ref.
- Structural sharing: only modified subtree paths produce new tree objects.
- Retry loop on CAS failure (configurable max retries).
- `get(path)`, `put(path, value)`, `delete(path)`, `list(path)` — path is `&[&str]`, traversal through nested trees.

### Done when

Integration test: init a db, open a transaction, write 10 keys across 3 subtree levels, commit, re-read all keys, verify round-trip.
Second test: two concurrent transactions to the same db, one wins, one retries.

---

## Phase 2: Chain Primitive

**Duration:** 1–2 weeks **Depends on:** Phase 1 **Output:** `append(path, entry)`, `log(path)` on `Tx`

### Work

- Chain representation: entries as sequentially-named subtrees within the parent tree.
  Entry N is a tree containing user-defined blobs.
- `append` adds the next entry to the in-memory buffer.
- `log` iterates entries in order by recovering history from the enclosing transaction commit chain.
- Chain entries are immutable once committed — append-only enforced by the API.

### Open question

Embedded representation (entries as subtrees in the state tree) vs. commit-chain representation (each entry is its own commit with a parent pointer).
The design doc specifies embedded.
Confirm this is sufficient for forge comments before committing — if comment threads grow large, the full-scan cost of embedded entries may matter.

### Done when

Integration test: append 100 entries to a chain, read them back in order, verify content and ordering.

---

## Phase 3: Self-Hosted Metadata

**Duration:** 1 week **Depends on:** Phase 1 **Output:** `.db/` subtree written on `db init`

### Work

- On `db init`, write the `.db/` subtree: `schema-version`, `type-registry/` (empty ledger), `annotations/` (empty ledger).
- `.db/` is a ledger like any other — no special-casing in the transaction path.
- Type registry maps path patterns to merge strategy names.
- Annotation store maps path patterns to policy flags (`derived`, `write-once`, `local-only`).

### Design decision: kill `.db-type` marker blobs

The merge dispatcher resolves types by path prefix lookup against the registry, not by reading marker blobs from the state tree.
The tree contains only user data.
The registry (populated from facet `SHAPE` at `db init` for Rust consumers, or manually via CLI for shell consumers) is the single source of type information.

### Done when

`db init` produces a valid state tree with `.db/` metadata.
Registry and annotation entries can be written and read through the normal `Tx` API.

---

## Phase 4: CLI Plumbing (Tier 1)

**Duration:** 2–3 weeks **Depends on:** Phases 1–3 **Output:** `git db` subcommands for basic operations

### Commands

```text
git db init <name>
git db list
git db drop <name>
git db tx begin <name>         → prints txid (snapshot OID)
git db tx get <txid> <path>
git db tx put <txid> <path> [--stdin | --file=<f> | <literal>]
git db tx delete <txid> <path>
git db tx append <txid> <path> [--stdin | --file=<f> | --tree <k=v>...]
git db tx log <txid> <path>
git db tx list <txid> <path>
git db tx commit <txid> [--message=<msg>] [--author=<a>]
git db tx abort <txid>
git db show <name> [<path>]
git db log <name> [-n <count>]
```

### Work

- Transaction state in `.git/db-tx/<txid>` — ephemeral, deleted on commit/abort.
- stdin/stdout conventions, exit code 0 on success, 1 on CAS contention.
- `--tree` flag on `append` accepts `key=value` pairs for structured chain entries.

### Done when

A shell script can create a db, run a transaction, write and read values, append to a chain, and inspect history — all through the CLI.

---

## Phase 5: Merge

**Duration:** 3–5 weeks **Depends on:** Phases 1–3 **Output:** `merge()` function, `MergeStrategy` trait, built-in strategies, conflict representation

This is the critical path.
Budget accordingly.

### Work

- `MergeStrategy` trait: `fn merge(base, left, right) -> MergeResult`.
- Merge dispatcher: parallel walk of base/left/right trees.
  At each node:
  - Same OID → keep.
  - Changed in one side only → take.
  - `derived` annotation → skip, rebuild after.
  - Changed in both → path-prefix lookup in registry → dispatch to strategy.
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
- Derived index rebuild: after merge dispatch completes, walk merged primary data, regenerate all `derived`-annotated subtrees.
- `StrategyMap`: registry-backed for CLI, trait-backed for Rust consumers.
  Dispatcher doesn't distinguish.

### Open question: derived index rebuild contract

The rebuild-after-merge path requires a function `primary_data → index_data`.
For Rust consumers this is a trait impl.
For CLI consumers, this is unspecified.
Options:

1. Shell consumers provide a rebuild script registered in `.db/annotations/`.
2. Derived indexes are Rust-only; CLI merge skips them and marks them stale.
3. The annotation includes enough metadata (source path, key extraction rule) to rebuild mechanically.

Option 3 is the most honest but may not generalize.
Decide before implementing.

### CLI additions

```text
git db merge <name> <left-oid> <right-oid> [--strategy=<s>]
git db conflicts <name> <oid>
git db resolve <name> <oid> <path> [--take=left|right|base] [--value=<v>]
```

### Done when

- Two-fork merge of a ledger with disjoint keys auto-merges cleanly.
- Two-fork merge of a ledger with same-key conflict produces a conflict record.
- Two-fork merge of a chain produces a causal interleave.
- Derived indexes are rebuilt from merged primary data.
- Property tests: merge(left, right) and merge(right, left) produce the same result (commutativity). merge(x, x) = x (idempotency).
- Fuzz testing on randomly generated state trees.

---

## Phase 6: Facet Integration

**Duration:** 2 weeks **Depends on:** Phases 3, 5 **Output:** `GitDbType` derive macro, automatic serialization/deserialization/strategy derivation

### Work

- `#[derive(GitDbType)]` walks facet `SHAPE` to produce:
  - Tree layout (struct fields → named blobs, nested structs → subtrees).
  - Path → strategy mapping from `#[facet(merge = "...")]` attributes.
  - `MARKER` string from type name.
- On `db init`, registered `GitDbType` impls write their path → strategy mappings to `.db/type-registry` and policy annotations to `.db/annotations/`.
- Default strategy derivation: named struct → ledger (field-by-field), `Vec<T>` → chain (causal interleave), `Option<T>` → LWW, scalar → LWW.
- `serialize` / `deserialize` between Rust types and git tree/blob structures.

### Done when

A `#[derive(GitDbType)]` struct can be written to and read from a `Tx` without manual tree construction.
Merge strategies are derived from the type definition and match hand-written equivalents.

---

## Phase 7: forge Migration

**Duration:** 2–3 weeks **Depends on:** Phases 1–6 **Output:** forge's storage layer rebuilt on `git_store::Db` and `Tx`

This is the validation gate.
If forge can't be cleanly expressed, the primitives need revision.

### Work

- Replace `store.rs`, `issue.rs`, `review.rs`, `comment.rs`, `contributor.rs` internals with `git_store` calls.
- Remove `objects/` GC workaround subtree (git-store's tree reachability handles GC).
- Remove manual index rebuild logic; replace with inline index maintenance in each transaction.
- Define `ForgeIssue`, `ForgeComment`, `ForgeReview`, `ForgeContributor` as `#[derive(GitDbType)]` structs.
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

**Duration:** 2–3 weeks **Depends on:** Phases 1, 3 **Output:** Path algebra evaluator, index-aware planner, `git db query`

### Work

- Path algebra: `issues/*/state`, `issues/*/[state="open"]`, `reviews/*/approvals/*/*`.
- Evaluator: subtree enumeration, blob predicate filtering, subpath projection.
- Index-aware planner: reads `derived` annotations from `.db/annotations/`, rewrites predicates to use index paths when available.
- OID-keyed result cache: track input subtree OIDs per query, short-circuit on match.
- `git db query <name> <pattern> [--where <path>=<value>] [--select <path>]`
- `git db query <name> --explain <pattern>` — show query plan.

### Done when

`git db query forge "issues/*/[state=\"open\"]/title"` returns open issue titles. `--explain` shows index rewrite when `index/issues-by-display-id/` exists.

---

## Phase 9: Documentation and Stabilization

**Duration:** 1–2 weeks **Depends on:** all prior phases **Output:** crate docs, man pages, tutorial, specification

### Work

- Rustdoc for all public types and traits.
- Man pages for every `git db` subcommand.
- Tutorial: "Build a porcelain on git-store" — walk through creating a minimal app (e.g., a todo list) from `db init` to merge.
- Specification document: tree layout, type registry format, merge contracts, query algebra syntax.
- Review all `// TODO` and `// HACK` comments; resolve or file issues.

---

## Timeline Summary

| Phase | Duration | Cumulative |
|-------|----------|------------|
| 0: Foundation Traits | 1–2 weeks | 1–2 weeks |
| 1: Transaction | 2–3 weeks | 3–5 weeks |
| 2: Chain Primitive | 1–2 weeks | 4–7 weeks |
| 3: Self-Hosted Metadata | 1 week | 5–8 weeks |
| 4: CLI Plumbing (Tier 1) | 2–3 weeks | 7–11 weeks |
| 5: Merge | 3–5 weeks | 10–16 weeks |
| 6: Facet Integration | 2 weeks | 12–18 weeks |
| 7: forge Migration | 2–3 weeks | 14–21 weeks |
| 8: Query Layer | 2–3 weeks | 16–24 weeks |
| 9: Docs & Stabilization | 1–2 weeks | 17–26 weeks |

**Minimum viable release (single-writer, no concurrent merge):** Phases 0–4. ~7–11 weeks.
**Full 0.1 release:** all phases. ~17–26 weeks.
**Critical path:** Phase 5 (merge).
Start design work and property test scaffolding during Phase 2.

---

## Open Questions (Decide Before or During Implementation)

1. **Chain representation.**
   Embedded subtrees (current design) vs. commit-chain per chain instance.
   Embedded is simpler but loses per-entry commit metadata.
   Decide during Phase 2 by benchmarking forge comment thread traversal.

2. **Derived index rebuild for CLI consumers.**
   No mechanism currently exists for shell porcelains to specify rebuild logic.
   Decide during Phase 5.
   See options in that phase.

3. **Chain merge strategy selection.**
   Causal interleave is the default.
   Preserve-DAG is needed for gin's op log.
   Confirm the strategy is selectable per-chain via the same path → strategy registry mechanism.
   This should fall out naturally but verify.

4. **Local-only refs.**
   `refs/db-local/<n>` is specified but not scheduled.
   Defer to post-0.1 unless forge or gin needs it during migration.

5. **Reftable support.**
   gix's reftable support may be incomplete.
   The `git update-ref --stdin` fallback in Phase 0 is the hedge.
   Test on a reftable-enabled repo early.

6. **gin as second consumer.**
   The strongest validation of primitive sufficiency is a second consumer with different needs.
   Schedule a spike (1 week) after Phase 7 to sketch gin's op log and change-ID map on git-store.
   If it doesn't fit, the primitives need revision before 0.1.
