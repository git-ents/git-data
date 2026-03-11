# 📋 `git-metadata`

*Porcelain for adding metadata to any object without rewriting history.*

<!-- rumdl-disable MD013 -->
[![CI](https://github.com/git-ents/git-metadata/actions/workflows/CI.yml/badge.svg)](https://github.com/git-ents/git-metadata/actions/workflows/CI.yml) [![CD](https://github.com/git-ents/git-metadata/actions/workflows/CD.yml/badge.svg)](https://github.com/git-ents/git-metadata/actions/workflows/CD.yml)
<!-- rumdl-enable MD013 -->

> [!CAUTION]
> This project is in active development.
> There are surely bugs and misbehaviors that have not yet been discovered.
> Please file a [new issue] for any misbehaviors you find!

[new issue]: https://github.com/git-ents/git-metadata/issues/new

## Overview

To support a more expansive usage of the Git object database — as is the goal for other projects within the [`git-ents`](https://github.com/git-ents) organization — new tooling is needed.
This project provides a command that allows users to associate arbitrary data to any object in Git's store.
The `metadata` command follows `notes` semantics.

[Notes] are a tragically underutilized feature of Git.
For more information about `git notes` entries, Tyler Cipriani's [blog post] is an excellent introduction, and some highly-motivating examples.
One such example is Google's open-source [`git-appraise`] project, which stores code review metadata as structured entries in a note blob.
While impressive, that design highlights a limitation of notes: structured data, or data that does not map cleanly onto UTF-8 text, is difficult to represent in a blob format.
The `git-metadata` project provides a structured alternative to the notes-blob design using Git trees objects.
Just like notes, metadata added to an object does not alter the object's history.

> [!TIP]
> Unlike notes, `metadata` is not added to `git log`.

[Notes]: https://git-scm.com/docs/git-notes
[blog post]: https://tylercipriani.com/blog/2022/11/19/git-notes-gits-coolest-most-unloved-feature/
[`git-appraise`]: https://github.com/google/git-appraise

## Usage

Given any blob (file), tree (folder), or commit, add metadata using `metadata add`.
By default, `add` assumes you're adding metadata to `HEAD`.
Alternatively, use the `--oid` option to specify a Git object identifier.
To remove metadata for a particular object, use `metadata remove` and provide glob patterns which represent entries in the metadata tree to be deleted.
Use the `--keep` option to instead specify patterns to keep.
For more information, see `git metadata --help`.

## Installation

### CLI

The `git-metadata` plumbing command can be installed with `cargo install`.

```shell
cargo install --locked git-metadata
```

If `~/.cargo/bin` is on your `PATH`, you can invoke the command with `git`.

```shell
git metadata -h
```

### Library

The `git-metadata` library can be added to your Rust project via `cargo add`.

```shell
cargo add git-metadata
```
