+++
title = "git-data"
subtitle = "Design Specification"
version = "0.2.0"
date = 2026-03-19
status = "Draft"
+++

# git-data

## Overview

git-data is a workspace of three crates that provide structured data primitives over Git refs.

Git has refs, commits, trees, and blobs.
It has no built-in concept of versioned records, shared indexes over objects, append-only event logs, or bidirectional relationships between entities.
These patterns recur in any system that uses Git as a data store.

git-data provides them as independent, composable libraries.

## Primitives

Three storage primitives, two constraints, one anti-primitive, one base layer.

**Storage primitives:**

- **Ledger** — one ref per record, tree as state, commits as versions.
- **Metadata** — one shared ref, tree fans out by key, concurrent writes auto-merge.
- **Chain** — one ref per stream, commits append events, entries are never edited.

**Constraints on primitives:**

- **Relation** — atomic bidirectional writes in a metadata tree.
  An integrity constraint, not a separate primitive.
- **Attestation** — commit signature as proof, parent links as provenance.
  Any ledger entry, metadata entry, or chain event can be an attestation.

**Anti-primitive:**

- **Ephemeral** — declared in source, scoped to execution, never stored in Git.
  Secrets are the canonical example.

**Base layer:**

- **Source** — blobs in the worktree, versioned on branches.
  Git itself.
  Policy, configuration, check definitions — anything read from a commit's tree.

## Crates

### git-ledger

Versioned records stored as refs.

A ledger entry is a standalone ref with its own lifecycle.
It is not metadata on any object — it is an independent record with a commit history as an audit log and tree-structured state.

**Ref structure:**

```text
refs/<namespace>/<id> → commit → tree
  <field>             # blob: field value
  <field>
  <subdir>/
    <field>
```

Each record is its own ref.
Two writers modifying different records never conflict.

**Naming strategies:**

The ID is a parameter.
Three strategies cover all known uses:

- **Sequential.**
  IDs are integers assigned by scanning `refs/<namespace>/` to find the highest existing ID and incrementing.
  The ref creation is the compare-and-swap: if another writer created the same ID, the push fails and the creator rescans and retries.
  No counter ref is required.
  The source of truth for "what IDs exist" is the refs themselves.
- **Content-addressed.**
  IDs are derived from the content — typically a hash of inputs.
  Two writers producing the same content produce the same ID, which is correct.
  Cache entries are the canonical example.
- **Caller-provided.**
  IDs are opaque strings chosen by the consumer.
  Contributors keyed by human-chosen identifiers, secret ACLs keyed by secret name.

At large scale (thousands of records), scanning all refs to find the max becomes expensive for sequential naming.
An optional counter ref can serve as an acceleration structure — a performance optimization, not a correctness requirement.
If the counter is lost or stale, a rescan rebuilds it.

**Operations:**

- `create(namespace, id_or_strategy, fields)` — create a new ref with an initial commit containing the given tree.
  For sequential naming, scan for the next available ID and retry on conflict.
- `read(namespace, id)` — read the current tree at a record's ref.
- `update(namespace, id, mutations)` — commit a new tree to the record's ref.
  The previous state is preserved in history.
- `list(namespace)` — prefix scan over `refs/<namespace>/` to enumerate records.
- `history(namespace, id)` — walk the commit chain on a record's ref.

**Namespace scoping:**

Namespaces partition records into independent groups, each with its own ref subtree:

```text
refs/<namespace>/<scope>/<id>
```

Scopes are fully independent: no cross-scope contention on ID assignment or record writes.

### git-metadata

Structured annotations and indexed lookups on a shared ref.

A metadata ref is a single ref whose tree fans out by key.
Multiple writers touching disjoint keys auto-merge via three-way tree merge.
This extends Git's notes (which map OIDs to blobs) to map arbitrary keys to trees, allowing multiple tools to attach named entries under the same key without conflict.

**Ref structure:**

```text
refs/metadata/<namespace> → commit → tree
  <key-prefix>/
    <key-suffix>/
      <entry-name>          # blob: arbitrary content
```

The two-level fanout by key prevents pathological tree sizes, matching the pattern Git uses internally for loose objects.
Fanout is applied when keys are OIDs or hashes.
For short human-readable keys, single-level paths are sufficient.

**Operations:**

- `metadata_add(key, path, content)` — write a blob at `<key>/<path>` in the metadata tree.
- `metadata_get(key, path)` — read a single entry.
- `metadata_show(key)` — list all entries for a key.
- `metadata_remove(key, path)` — delete an entry.

Every write is a new commit on the metadata ref.
The commit history is the audit log of all annotation changes.

**Concurrency:**

Two writers annotating different keys touch disjoint tree paths.
A three-way tree merge resolves these automatically.
Two writers annotating the same key at different paths also merge cleanly.
Conflict occurs only when two writers modify the same entry on the same key simultaneously — the correct resolution is rejection and retry.

**Relations:**

A relation is a bidirectional link between two keys, stored in a metadata tree with direction labels.
Both directions are written in a single commit, guaranteeing consistency without multi-ref atomicity.

Keys are single path segments using `:` as an internal delimiter.
Git refs cannot contain `:` (`git check-ref-format` rejects it), but tree entries can.
Any key derived from a ref path naturally avoids colons, making the delimiter unambiguous.

The tree structure is always three levels: `<key>/<relation>/<target>`.

```text
refs/<namespace> → commit → tree
  issue:42/
    parent/
      issue:10            # blob: empty or optional metadata
    fixes/
      commit:abc123       # blob: empty or optional metadata
  issue:10/
    child/
      issue:42            # blob: empty or optional metadata
  commit:abc123/
    fixed-by/
      issue:42            # blob: empty or optional metadata
```

When metadata is absent, the tree entry points to the empty blob (`e69de29...`).
Every metadata-free relation shares this single object.

**Relation operations:**

- `link(a, b, forward_label, reverse_label, metadata?)` — write both directions in one commit.
- `unlink(a, b, forward_label, reverse_label)` — remove both directions in one commit.
- `linked(key)` — list all keys linked to this key across all relations (read multiple subtrees).
- `linked(key, relation)` — list all keys linked to this key by a specific relation (single tree read).
- `is_linked(a, b)` — check existence (tree entry lookup).

**Concurrency for relations:**

Two writers linking disjoint key pairs touch disjoint tree paths.
A three-way tree merge resolves these automatically.
Conflict occurs only when two writers modify the same link simultaneously.

**Ref ownership:**

The `namespace` is caller-provided.
The library owns no ref namespace.
A consumer passes `"refs/forge/links"` or `"refs/metadata/approvals"` — the library does not care.

### git-chain

Append-only event streams stored as refs.

A chain is a ref where each commit represents an event.
Each commit's tree holds only that entry's payload — there is no accumulated state across commits.
The commit chain itself provides chronological ordering.

**Ref structure:**

```text
refs/<namespace>/<scope> → commit → tree
  <payload-name>        # blob: event content
  <payload-name>
```

Each commit's tree contains the payload files for that single event.

**Threading:**

Chains support optional threading via second parents.
A reply's second parent points at the commit that introduced the entry it replies to.
The first parent is always the previous chain tip (chronological order).
The second parent is the semantic parent (thread structure).

**Operations:**

- `append(namespace, scope, content)` — create a new commit adding an entry to the tree.
- `append_reply(namespace, scope, content, parent_commit)` — same, with a second parent for threading.
- `list(namespace, scope)` — read the current tree to enumerate all entries.
- `walk(namespace, scope)` — walk the commit chain for ordered history.
- `thread(namespace, scope, commit)` — follow second-parent links to reconstruct a thread.

Every append is a new commit.
The commit history is the definitive ordering.
The tree provides random access to current state.

**Concurrency:**

Two writers appending to the same chain both advance the tip.
A three-way tree merge resolves these when the entries have disjoint IDs, which they always do if IDs include a timestamp and author.
Merge commits on chain refs create non-linear history.
This is correct — the history is an event log, not a development narrative.
DAG order with timestamps is sufficient for reconstruction.

## Attestation

Any commit in any primitive can serve as an attestation.
The signature is the point — the commit attests that a specific actor produced a specific tree at a specific time.

In a ledger, an attestation means "a trusted actor set this state" (CI signed this build output).
In a metadata tree, an attestation means "a trusted actor made this annotation" (a runner recorded a check result).
In a chain, an attestation means "a trusted actor recorded this event" (an auditor logged a secret access).

**Provenance via parents:**

Attestation commits can use parent links to form a provenance DAG.
A build output commit whose parents are its input commits encodes the build graph in Git's native structure.
`git log` on the build graph shows the entire DAG.
`git verify-commit` on any node proves who built it.

## Ephemeral

Ephemeral data is declared in source but intentionally never persisted in Git.
It exists only for the duration of an execution context and is destroyed afterward.

The declaration is visible and reviewable (a check definition listing required secret names).
The value is provided by the executor at runtime, injected into a scoped environment (tmpfs mount, temporary directory), and destroyed on exit.

Ephemeral data is excluded from cache keys and content addressing.
Two executions with different ephemeral values but identical content-addressed inputs are considered equivalent.
This is correct — the ephemeral input enables a side effect (deployment, signing), not a deterministic build output.

git-data does not implement ephemeral handling.
It is documented here because it completes the theory: every piece of state in a Git-backed system is either source, a ledger entry, a metadata entry, a chain event, or ephemeral.
There is no sixth category.

## Coverage

Every feature across known consumers maps to these primitives:

| Feature | Primitive | Naming / Key | Attestation |
|---|---|---|---|
| Issues | ledger | sequential | no |
| Reviews | ledger | sequential | no |
| Releases | ledger | sequential | no |
| Contributors | ledger | caller-provided | no |
| Requirements | ledger | sequential | no |
| Build outputs | ledger | content hash | yes |
| Build plans | ledger | content hash | no |
| Secret ACLs | ledger | caller-provided | no |
| Approvals | metadata | OID / patch-id | yes |
| Check results | metadata | commit OID | yes |
| Entity links | metadata + relation | entity key | no |
| Traceability | metadata + relation | entity key | no |
| Merge queue | metadata | sequential position | no |
| Comments | chain | per-entity | no |
| Notifications | chain | per-user | no |
| Secret audit | chain | per-repo | yes |
| Policy | source | — | — |
| Check definitions | source | — | — |
| Config (env, kiln) | source | — | — |
| Secret declarations | source | — | — |
| Secret values | ephemeral | — | — |

## Layering

```text
git (objects, refs, transport)
├── git-ledger      (versioned records as refs)
├── git-metadata    (shared indexes and relations)
└── git-chain       (append-only event streams)
```

The three crates are independent.
None depends on another.
A consumer may use any combination.

The shared machinery — ref → commit → tree reads and writes, tree merging, commit signing — is either inlined or extracted to a shared internal crate if duplication warrants it.
This is a code organization decision, not an architectural one.

## What git-data Is Not

git-data is not a framework.
It imposes no schema, no workflow, no naming convention beyond ref structure.

git-data does not run hooks or enforce policy.
Consumers (forge, kiln, other tools) own domain logic.

git-data does not handle transport.
Push, fetch, and ref advertisement filtering are the consumer's responsibility.

git-data does not handle merge strategy selection.
It provides the primitives (tree reads, tree writes, atomic commits) that make auto-merge possible.
The consumer decides when and how to merge.

git-data does not cache derived queries.
Consumers maintain their own indexes.
Correctness never depends on a derived index.

## Workspace Layout

```text
git-data/
├── Cargo.toml              # workspace root
├── crates/
│   ├── git-ledger/
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── git-metadata/
│   │   ├── Cargo.toml
│   │   └── src/
│   └── git-chain/
│       ├── Cargo.toml
│       └── src/
```

Each crate publishes independently to crates.io.
The workspace shares test infrastructure, CI, and release tooling.

## CLI

All three crates ship CLIs:

- `git-metadata` (invoked as `git metadata`) — metadata annotation and relation operations.
- `git-ledger` (invoked as `git ledger`) — record creation, reading, updating, and listing.
- `git-chain` (invoked as `git chain`) — event appending and chain walking.
