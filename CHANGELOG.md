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
- Engine-side `.meg` archive wrapper and staging helpers in `ic-cnc-content`
- `G1.3` `.shp` / `.pal` parser-to-render handoff metadata for the future render crate
- Initial `ic-render` crate with camera bootstrap, classic isometric projection math, and static-scene validation
- Bootstrap indexed-to-RGBA sprite conversion helpers in `ic-render`
- Initial `ic-game` crate with a runnable Bevy window and first synthetic RA-style demo sprite
- First `ic-game` content-lab window with real local RA / Remastered source cataloging and keyboard-browsable file inventory
- First actual Red Alert resource preview path in `ic-game`, including SHP sprite rendering with resolved palettes and PAL swatch preview
- Mounted `.mix` / `.meg` archive members in the `ic-game` content lab so archive-contained resources appear as logical catalog entries
- Expanded `ic-game` into a broader content-validation lab with AUD/WAV playback surfaces, WSA/VQA animation preview, text/config inspection, and LUT/VQP/FNT/TMP diagnostic viewers
- Scrollable thumbnail gallery in `ic-game` so visible RA / Remastered resources now render as one on-screen wall with filename captions and selected-resource playback controls
- Aspect-preserving gallery sizing and a focused preview/player pane in `ic-game` so mixed RA resource types are no longer stretched to one generic rectangle
- Local design-gap note for the Remastered `.bk2` support boundary so future sessions treat Bink 2 as an explicit policy/backend decision instead of silently folding it into the clean-room baseline
- Local design-gap note for media-container and localization-packaging strategy so future sessions keep language/fallback policy in IC manifests instead of conflating it with the raw media container
- Native `mimalloc` allocator wiring for the `ic-game` executable, following the canonical desktop/mobile-vs-WASM target split from the performance design docs
- Dedicated `wasm32-unknown-unknown` GitHub Actions validation lane plus local contributor guidance for running the matching target checks directly
- Dedicated Android and iOS GitHub Actions compile/lint lanes (`aarch64-linux-android`, `aarch64-apple-ios`) plus local contributor guidance for the matching target checks

### Changed

- Renamed the engine-side content crate from `ra-formats` to `ic-cnc-content`
- Tightened the Bevy feature surface for the runnable client so Linux builds use the local X11 path without requiring Wayland development packages
- Evolved `ic-game` from a pure demo window into a content-lab bootstrap that scans configured source roots and displays their typed file inventory in-window
- Evolved the content-lab catalog model from loose-file-only browsing into a unified loose-file / archive-member content graph
- Split the oversized `ic-game` content-window module into focused catalog/state/preview files to stay aligned with the repo tree rules
- Split the content-lab preview path into pure decode logic and Bevy runtime control logic, with Windows-targeted Bevy audio playback and cross-platform decode/waveform validation
- Replaced the content-lab `AUD` / VQA audio WAV bridge with direct PCM preview playback on Windows, keeping decoded samples in-memory instead of synthesizing temporary WAV files
- Reworked the local CI wrappers into host-native `lint` / `test` / `all` entrypoints and expanded GitHub Actions `check` / `clippy` coverage across Ubuntu, Windows, and macOS so platform-gated code is validated on the correct host
- Added stable top-level `ci` dispatchers so local validation can start from one repo entrypoint instead of memorizing host-specific wrapper names
- Switched the content lab from oversized windowed startup to borderless fullscreen startup with a deliberate double-`Esc` exit gesture
- Reworked content-lab navigation around gallery browsing instead of the old text-only entry list, moving source switching to `Q/E` and dedicating arrow keys to thumbnail scrolling
- Reworked the content-lab preview runtime so selected media uses one persistent display surface, heavy movie preview preparation happens off the main thread, and Windows preview audio plays through a direct PCM path instead of temporary WAV synthesis
- Aligned local repo docs with the current content-lab runtime, design-gap notes, and canonical media-container policy wording

### Fixed

- Aligned Bevy asset-loader wrappers with Bevy `0.18`
- Corrected the content-lab bootstrap so it uses one explicit fullscreen UI hierarchy, chooses host-native RA/Remastered source paths by default, and prefers showcase assets like `TANYA1.VQA` on startup when they are available
- Moved content-root scanning behind a background worker so `ic-game` opens its fullscreen content-lab window immediately and shows a loading state while large RA / Remastered catalogs are built
- Fixed VQA preview presentation so movie frames render fully opaque instead of treating palette index `0` as transparent sprite data
