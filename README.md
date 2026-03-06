#  `git-rewrite`

*Porcelain for rewriting repository history, trees, and (eventually) blobs.*

<!-- rumdl-disable MD013 -->
[![CI](https://github.com/git-ents/git-filter/actions/workflows/CI.yml/badge.svg)](https://github.com/git-ents/git-filter/actions/workflows/CI.yml)
[![CD](https://github.com/git-ents/git-filter/actions/workflows/CD.yml/badge.svg)](https://github.com/git-ents/git-filter/actions/workflows/CD.yml)
<!-- rumdl-enable MD013 -->

> [!CAUTION]
> This project is being actively developed!
> Despite this, semantic versioning rules will be respected.
> Expect frequent updates.

## About

To support a more expansive usage of the Git object database — as is the goal for other projects within the [`git-ents`](https://github.com/git-ents) organization — new tooling is needed.
This project provides a command that allows users to rewrite repository history as safely as possible.

You may see the terms *porcelain* and *plumbing* used across this project.
These are [borrowed from Git itself](https://git-scm.com/book/en/v2/Git-Internals-Plumbing-and-Porcelain): porcelain refers to user-facing commands, and plumbing refers to the lower-level libraries and commands they are built on.

## Crates

| Crate | Description | API |
|---|---|----|
| [`git-rewrite`](crates/git-rewrite/) | Rewrite repository history and trees via a `git`-style CLI | Porcelain |
| [`git-filter-tree`](crates/git-filter-tree/) | Filter Git tree objects by glob pattern, as a library or CLI | Plumbing |
