# Changelog

## [0.3.0-rc.1](https://github.com/git-ents/git-data/compare/git-metadata-v0.2.1...git-metadata-v0.3.0-rc.1) (2026-03-26)


### Features

* Add git-chain ([2a2cfea](https://github.com/git-ents/git-data/commit/2a2cfeaa9a78ee3d4a764008c14c9acb90672594))
* Add git-ledger ([2a2cfea](https://github.com/git-ents/git-data/commit/2a2cfeaa9a78ee3d4a764008c14c9acb90672594))
* Implement relation operations in git-metadata ([7725be8](https://github.com/git-ents/git-data/commit/7725be82c332da618ab0bfc6d6d39d9f46ee064b))


### Bug Fixes

* Apply clippy suggestions (search_is_some in tests, fmt in main.rs) ([6f83928](https://github.com/git-ents/git-data/commit/6f83928928c114a0cfd54a2b9d98732c64a461a9))
* Handle slash in link keys and batch prune commits in git-metadata ([a37fb3e](https://github.com/git-ents/git-data/commit/a37fb3eb1430d869e77be663681f8223876c004d))

## [0.2.1](https://github.com/git-ents/git-data/compare/git-metadata-v0.2.0...git-metadata-v0.2.1) (2026-03-20)

### Miscellaneous Chores

* Move project to new repository ([d961a8c](https://github.com/git-ents/git-data/commit/d961a8cc0cf8459b790b4d614bd27c0e4d24cd15))
* Pin release ([5c1728a](https://github.com/git-ents/git-data/commit/5c1728a684724ed6507b9f9f06bf563f21db796e))

## 0.1.0 (2026-03-11)

### ⚠ BREAKING CHANGES

* metadata_remove no longer takes MetadataOptions

### Features

* Add executor (exe) and CLI implementation ([10d9c07](https://github.com/git-ents/git-data/commit/10d9c0749bdfa7abfe0c4f14af528e4c91a43e51))
* Add initial trait method and accompanying implementation ([c4ed9f8](https://github.com/git-ents/git-data/commit/c4ed9f85753443685fbaa63b13ffb1501f4d92d1))
* Add man generation to CLI ([275207f](https://github.com/git-ents/git-data/commit/275207f1b6c7b7aad9bdda1ccfb3b18f2d0fc2ac))
* Add metadata trait method definitions ([30ef2eb](https://github.com/git-ents/git-data/commit/30ef2eb67582dcceae1a88f6819802370dd00947))
* Add metadata trait method implementations ([30ef2eb](https://github.com/git-ents/git-data/commit/30ef2eb67582dcceae1a88f6819802370dd00947))
* Auto-detect fanout depth on read and remove ([bd86065](https://github.com/git-ents/git-data/commit/bd860650b8471215ad5f131f28666d613648178c))
* Glob matching for remove patterns (*, **, prefix match) ([4cf0d90](https://github.com/git-ents/git-data/commit/4cf0d90c56bed7188db4511eb0132d475286435c))
* Path-based metadata API following git-notes semantics ([4cf0d90](https://github.com/git-ents/git-data/commit/4cf0d90c56bed7188db4511eb0132d475286435c))
* Resolve revisions (HEAD, refs, short OIDs) in all object arguments ([4cf0d90](https://github.com/git-ents/git-data/commit/4cf0d90c56bed7188db4511eb0132d475286435c))

### Bug Fixes

* Auto-detect fanout depth and correct shard level semantics ([bd86065](https://github.com/git-ents/git-data/commit/bd860650b8471215ad5f131f28666d613648178c))
* Correct release-please manifest package key ([88f4ec1](https://github.com/git-ents/git-data/commit/88f4ec128b8c7a6544a612ae5a673e9462be7b8b))
* Remove cargo-workspace plugin for single-crate repo ([1d19a7d](https://github.com/git-ents/git-data/commit/1d19a7de740e6d637f7bc7751604d3deab244643))
* Shard-level default changed from 2 to 1 (git-notes compatible) ([bd86065](https://github.com/git-ents/git-data/commit/bd860650b8471215ad5f131f28666d613648178c))
