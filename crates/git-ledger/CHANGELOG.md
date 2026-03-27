# Changelog

## [0.1.0-alpha.2](https://github.com/git-ents/git-data/compare/git-ledger-v0.1.0-alpha.1...git-ledger-v0.1.0-alpha.2) (2026-03-27)


### Features

* Add IdStrategy::CommitOid for commit-OID-keyed entity refs ([31c8dc3](https://github.com/git-ents/git-data/commit/31c8dc3ff376f0a88a78fdf444053c95ae0336da)), closes [#6](https://github.com/git-ents/git-data/issues/6)

## 0.1.0-alpha.1 (2026-03-26)


### Features

* Add --file flag and optional --set values to git-ledger ([800e3cf](https://github.com/git-ents/git-data/commit/800e3cfc55ce37a93244214bb94a34d9e140d104))
* Add --file flag to create and update commands ([800e3cf](https://github.com/git-ents/git-data/commit/800e3cfc55ce37a93244214bb94a34d9e140d104))
* Add git-chain ([2a2cfea](https://github.com/git-ents/git-data/commit/2a2cfeaa9a78ee3d4a764008c14c9acb90672594))
* Add git-ledger ([2a2cfea](https://github.com/git-ents/git-data/commit/2a2cfeaa9a78ee3d4a764008c14c9acb90672594))
* Add git-ledger crate ([c6d47d0](https://github.com/git-ents/git-data/commit/c6d47d077742d622fc9024fdacb5f67e9a9c38c1))
* Add log CLI subcommand to git-ledger ([8badb72](https://github.com/git-ents/git-data/commit/8badb722fd8ba27a6f1e15b375e53983682f4494))
* Add man page generation to all crates and fix CI/CD workflows ([0f3d0ef](https://github.com/git-ents/git-data/commit/0f3d0ef8c49793de9b0a751daf345f8b25a4be85))
* Add man page generation to git-ledger and git-chain ([0f3d0ef](https://github.com/git-ents/git-data/commit/0f3d0ef8c49793de9b0a751daf345f8b25a4be85))
* Support --set key with empty blob default ([800e3cf](https://github.com/git-ents/git-data/commit/800e3cfc55ce37a93244214bb94a34d9e140d104))


### Bug Fixes

* Apply clippy suggestions (search_is_some in tests, fmt in main.rs) ([6f83928](https://github.com/git-ents/git-data/commit/6f83928928c114a0cfd54a2b9d98732c64a461a9))
* Broaden CD tag regex to match all three crate prefixes ([0f3d0ef](https://github.com/git-ents/git-data/commit/0f3d0ef8c49793de9b0a751daf345f8b25a4be85))
* CI man job uses checkout@v4 and generates for all crates ([0f3d0ef](https://github.com/git-ents/git-data/commit/0f3d0ef8c49793de9b0a751daf345f8b25a4be85))
* Handle arbitrary-depth nested fields in ledger tree operations ([55991b9](https://github.com/git-ents/git-data/commit/55991b9610eef10d79c8796756442a74cc7efa38))
* Move project to new repository ([4697dfa](https://github.com/git-ents/git-data/commit/4697dfa55a70721ec63c87da208e31a1fbbfe061))
* Nested delete, create consistency, and Box::leak in git-ledger ([c00e676](https://github.com/git-ents/git-data/commit/c00e67675717a448003452e627eab5e31096224e))


### Miscellaneous Chores

* Move project to new repository ([d961a8c](https://github.com/git-ents/git-data/commit/d961a8cc0cf8459b790b4d614bd27c0e4d24cd15))
* Pin release ([5c1728a](https://github.com/git-ents/git-data/commit/5c1728a684724ed6507b9f9f06bf563f21db796e))
