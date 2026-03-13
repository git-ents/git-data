# Changelog

## [0.2.0](https://github.com/git-ents/git-metadata/compare/git-metadata-v0.1.0...git-metadata-v0.2.0) (2026-03-12)


### Features

* Add MetadataIndex::metadata ([fd7b179](https://github.com/git-ents/git-metadata/commit/fd7b1793465cfb167187c8d558dc61a35a3e1a28))
* Add MetadataIndex::metadata_commit ([fd7b179](https://github.com/git-ents/git-metadata/commit/fd7b1793465cfb167187c8d558dc61a35a3e1a28))

## 0.1.0 (2026-03-11)


### ⚠ BREAKING CHANGES

* metadata_remove no longer takes MetadataOptions

### Features

* Add executor (exe) and CLI implementation ([10d9c07](https://github.com/git-ents/git-metadata/commit/10d9c0749bdfa7abfe0c4f14af528e4c91a43e51))
* Add initial trait method and accompanying implementation ([c4ed9f8](https://github.com/git-ents/git-metadata/commit/c4ed9f85753443685fbaa63b13ffb1501f4d92d1))
* Add man generation to CLI ([275207f](https://github.com/git-ents/git-metadata/commit/275207f1b6c7b7aad9bdda1ccfb3b18f2d0fc2ac))
* Add metadata trait method definitions ([30ef2eb](https://github.com/git-ents/git-metadata/commit/30ef2eb67582dcceae1a88f6819802370dd00947))
* Add metadata trait method implementations ([30ef2eb](https://github.com/git-ents/git-metadata/commit/30ef2eb67582dcceae1a88f6819802370dd00947))
* Auto-detect fanout depth on read and remove ([bd86065](https://github.com/git-ents/git-metadata/commit/bd860650b8471215ad5f131f28666d613648178c))
* Glob matching for remove patterns (*, **, prefix match) ([4cf0d90](https://github.com/git-ents/git-metadata/commit/4cf0d90c56bed7188db4511eb0132d475286435c))
* Path-based metadata API following git-notes semantics ([4cf0d90](https://github.com/git-ents/git-metadata/commit/4cf0d90c56bed7188db4511eb0132d475286435c))
* Resolve revisions (HEAD, refs, short OIDs) in all object arguments ([4cf0d90](https://github.com/git-ents/git-metadata/commit/4cf0d90c56bed7188db4511eb0132d475286435c))


### Bug Fixes

* Auto-detect fanout depth and correct shard level semantics ([bd86065](https://github.com/git-ents/git-metadata/commit/bd860650b8471215ad5f131f28666d613648178c))
* Correct release-please manifest package key ([88f4ec1](https://github.com/git-ents/git-metadata/commit/88f4ec128b8c7a6544a612ae5a673e9462be7b8b))
* Remove cargo-workspace plugin for single-crate repo ([1d19a7d](https://github.com/git-ents/git-metadata/commit/1d19a7de740e6d637f7bc7751604d3deab244643))
* Shard-level default changed from 2 to 1 (git-notes compatible) ([bd86065](https://github.com/git-ents/git-metadata/commit/bd860650b8471215ad5f131f28666d613648178c))
