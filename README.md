# 📋 `git-data`

*Porcelain for adding metadata to any object without rewriting history.*

<!-- rumdl-disable MD013 -->
[![CI](https://github.com/git-ents/git-data/actions/workflows/CI.yml/badge.svg)](https://github.com/git-ents/git-data/actions/workflows/CI.yml) [![CD](https://github.com/git-ents/git-data/actions/workflows/CD.yml/badge.svg)](https://github.com/git-ents/git-data/actions/workflows/CD.yml)
<!-- rumdl-enable MD013 -->

> [!CAUTION]
> This project is in active development.
> There are surely bugs and misbehaviors that have not yet been discovered.
> Please file a [new issue] for any misbehaviors you find!

[new issue]: https://github.com/git-ents/git-data/issues/new

## Overview

Git's object store is tragically underutilized.
This project adds various abstractions for storing metadata on objects, storing tree structures with distinct lifetimes, and storing durable links between references.

## Crates

| Crate | Description |
| --- | --- |
| [`git-metadata`](crates/git-metadata) | Annotations on any Git object. |
| [`git-ledger`](crates/git-ledger) | Versioned records stored as refs. |
| [`git-chain`](crates/git-chain) | Append-only event chains stored as commit history. |
