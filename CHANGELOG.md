# Changelog

All notable changes to this repository should be documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Until the project reaches `1.0`, versioning remains pre-stable and changes may
restructure APIs and workspace layout aggressively.

## [Unreleased]

### Added

- Workspace baseline for `ic-protocol` and `ic-cnc-content`
- Repo-local implementation rules in `AGENTS.md`
- Human/LLM navigation index in `CODE-INDEX.md`
- Local CI scripts and expanded GitHub Actions coverage
- `G1.1` source probe contract and source-manifest snapshot schema in `ic-cnc-content`
- `G1.2` importer-facing `.mix` staging helpers with duplicate-CRC-safe extraction
- `G1.3` `.shp` / `.pal` parser-to-render handoff metadata for the future render crate
- Initial `ic-render` crate with camera bootstrap, classic isometric projection math, and static-scene validation

### Changed

- Renamed the engine-side content crate from `ra-formats` to `ic-cnc-content`

### Fixed

- Aligned Bevy asset-loader wrappers with Bevy `0.18`
