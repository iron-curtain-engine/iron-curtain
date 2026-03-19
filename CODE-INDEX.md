# CODE-INDEX.md — Iron Curtain Engine

> Source code navigation index for humans and LLMs.
> Canonical design authority: `https://github.com/iron-curtain-engine/iron-curtain-design-docs` @ `HEAD`

## How To Use This Index

- Start with **Current Repo State** so you know which crates and repo files
  actually exist locally.
- Use **Task Routing** to jump to the right file set for the job in front of you.
- Read the subsystem entry before editing a crate.
- If this index and the code disagree, update this file in the same change set.

## Current Repo State

- Active milestone(s): `M1`
- Active `G*` step(s): `G2` primary, with completed `G1.1`–`G1.3` feeding it and `G3` planned next
- Current implemented Rust crates:
  - `crates/ic-protocol`
  - `crates/ic-cnc-content`
  - `crates/ic-render`
  - `crates/ic-game`
- Current repo-ops files now maintained locally:
  - `AGENTS.md`
  - `CODE-INDEX.md`
  - `README.md`
  - `CONTRIBUTING.md`
  - `CHANGELOG.md`
  - `ci`
  - `ci.ps1`
  - `ci.cmd`
  - `ci-local.sh`
  - `ci-local.ps1`
  - `.github/workflows/ci.yml`
  - `.github/workflows/audit.yml`
  - `.github/workflows/dco.yml`
- Current placeholder directories with no substantive implementation yet:
  - `tests/`
  - `assets/`
  - `docs/design-gap-requests/`
  - `docs/implementation-notes/`
- Planned crates such as `ic-sim`, `ic-ui`, `ic-net`, and others
  are design-level architecture only right now. They do not exist in this repo
  yet and must not be treated as editable local code paths.

## Task Routing

| If you need to... | Start here | Then read | Avoid touching first |
| --- | --- | --- | --- |
| Add or change shared wire types | `crates/ic-protocol/src/lib.rs` | `crates/ic-protocol/src/tests.rs`, design docs D006/D008/D012 | inventing sim or net behavior locally |
| Add or change legacy C&C asset loading | `crates/ic-cnc-content/src/lib.rs` | format module `mod.rs` + `tests.rs`, design docs D003/D076 | modifying `cnc-formats` parsing rules in this repo |
| Add or change owned-source probe contracts | `crates/ic-cnc-content/src/source/mod.rs` | `crates/ic-cnc-content/src/source/tests.rs`, D069, `formats/backup-screenshot-import.md` | jumping straight to platform-specific probing without a normalized snapshot |
| Add or change importer staging for `.mix` archives | `crates/ic-cnc-content/src/mix/mod.rs` | `crates/ic-cnc-content/src/mix/tests.rs`, `05-FORMATS.md`, `execution-ladders.md` `G1.2` | re-implementing archive parsing instead of using `cnc-formats` |
| Add or change Remastered archive wrapping | `crates/ic-cnc-content/src/meg/mod.rs` | `crates/ic-cnc-content/src/meg/tests.rs`, sibling `cnc-formats` MEG parser docs/tests | bypassing the engine wrapper and talking to `cnc-formats` directly from `ic-game` |
| Add or change parser-to-render handoff for `.shp` / `.pal` | `crates/ic-cnc-content/src/shp/mod.rs` | `crates/ic-cnc-content/src/shp/tests.rs`, `crates/ic-cnc-content/src/pal/mod.rs`, `crates/ic-cnc-content/src/pal/tests.rs`, `execution-ladders.md` `G1.3` | making the future render crate depend on raw parser internals |
| Add or change archive-mounted content browsing | `crates/ic-game/src/content_window/catalog.rs` | `crates/ic-game/src/content_window/gallery.rs`, `crates/ic-game/src/content_window/preview_decode.rs`, `crates/ic-game/src/content_window/preview.rs`, `crates/ic-game/src/content_window/preview_audio.rs`, `crates/ic-game/src/content_window/tests.rs`, `crates/ic-cnc-content/src/mix/mod.rs`, `crates/ic-cnc-content/src/meg/mod.rs` | creating a second byte-loading path that only works for archive members or only for loose files |
| Add or change render-side camera math or static-scene validation | `crates/ic-render/src/lib.rs` | `crates/ic-render/src/camera/mod.rs`, `crates/ic-render/src/scene/mod.rs`, matching `tests.rs`, D017, D041, `tracker/checklists.md` `G2` | coupling early render code to missing sim crates or re-parsing format files |
| Add or change the runnable game-client bootstrap | `crates/ic-game/src/lib.rs` | `crates/ic-game/src/content_window/mod.rs`, `crates/ic-game/src/content_window/catalog.rs`, `crates/ic-game/src/content_window/state.rs`, `crates/ic-game/src/content_window/gallery.rs`, `crates/ic-game/src/content_window/preview_decode.rs`, `crates/ic-game/src/content_window/preview.rs`, `crates/ic-game/src/content_window/preview_audio.rs`, `crates/ic-game/src/content_window/tests.rs`, `crates/ic-game/src/demo.rs`, `crates/ic-render/src/camera/mod.rs`, `crates/ic-render/src/scene/mod.rs` | inventing gameplay or map-loading behavior that the current `G2` slice does not have yet |
| Learn how Bevy is used here | `crates/ic-cnc-content/src/lib.rs` | format loaders under `crates/ic-cnc-content/src/*/mod.rs`, matching tests | assuming full engine-runtime Bevy patterns already exist |
| Run repo validation with the stable entrypoint | `./ci lint`, `./ci test`, `./ci all` | `ci`, `ci-local.sh`, `ci-local.ps1`, `.github/workflows/ci.yml` | bypassing the repo dispatcher and forgetting which host wrapper proves which code path |
| Run host-native lint first | `ci-local.sh lint` or `ci-local.ps1 lint` | `.github/workflows/ci.yml`, `AGENTS.md` validation rules | treating Linux lint as proof for Windows-only code or vice versa |
| Run the full local validation flow | `ci-local.sh all` or `ci-local.ps1 all` | `.github/workflows/ci.yml`, `.github/workflows/audit.yml`, `deny.toml` | manually running partial checks and assuming parity |
| Update contributor-facing repo policy | `CONTRIBUTING.md` | `AGENTS.md`, `README.md`, `CODE-INDEX.md` | duplicating canonical design behavior here |
| Update the project overview / branding | `README.md` | `images/`, `CONTRIBUTING.md`, sibling repo READMEs | describing future local crates as if they already exist |
| Update implementation policy | `AGENTS.md` | remote design-doc `AGENTS.md`, `src/16-CODING-STANDARDS.md` | encoding one-off session history as permanent policy |
| Update repo navigation | `CODE-INDEX.md` | current tree under `crates/`, `.github/`, and top-level docs | documenting future crates as if they exist locally |
| Record a design gap or local verification note | `docs/design-gap-requests/` or `docs/implementation-notes/` | canonical design docs that motivated the note | silently diverging from the design docs |
| Start a brand-new subsystem from the design docs | `Cargo.toml` workspace + new crate directory | `AGENTS.md`, canonical crate-graph docs, `CODE-INDEX.md` | adding references to nonexistent code paths first |

## Repository Map (Actual Top Level)

| Path | Role | Notes |
| --- | --- | --- |
| `AGENTS.md` | Local implementation rules | Must stay aligned with canonical design docs and local workflow requirements |
| `CODE-INDEX.md` | Navigation index | Describes the repo that exists today, not only the target architecture |
| `README.md` | Contributor-facing overview | Branding, repo-family context, status, build commands |
| `CONTRIBUTING.md` | Contribution guide | DCO, local CI, design-doc relationship |
| `CHANGELOG.md` | Release-history skeleton | Tracks notable repo changes once the project advances |
| `ci` | Stable Unix-like validation dispatcher | Preferred top-level repo entrypoint for agents and Unix-like shells |
| `ci.ps1` | Stable PowerShell validation dispatcher | Preferred top-level repo entrypoint for Windows PowerShell |
| `ci.cmd` | Stable Command Prompt launcher | Thin wrapper that forwards to `ci.ps1` |
| `ci-local.sh` | Unix-like local CI wrapper | Host-native lint/test/all entrypoint for Unix-like environments |
| `ci-local.ps1` | PowerShell local CI wrapper | Host-native lint/test/all entrypoint for Windows environments |
| `Cargo.toml` | Workspace manifest | Declares current members and shared dependency policy |
| `deny.toml` | `cargo-deny` policy | License and dependency source policy for this GPL engine repo |
| `.github/workflows/ci.yml` | Main CI workflow | Check/clippy/test matrix on Ubuntu, Windows, and macOS plus fmt, docs, MSRV, license |
| `.github/workflows/audit.yml` | Security audit workflow | Scheduled and PR-triggered `cargo audit` |
| `.github/workflows/dco.yml` | DCO enforcement | Requires `Signed-off-by` on PR commits |
| `crates/ic-protocol/` | Shared protocol crate | Boundary types used by future sim/net work |
| `crates/ic-cnc-content/` | Bevy-facing content integration crate | Wraps `cnc-formats` with engine-specific asset loading behavior for loose formats and archive containers |
| `crates/ic-render/` | Render bootstrap crate | Owns render-side camera resources and static-scene validation for the future viewport |
| `crates/ic-game/` | Runnable game-client bootstrap crate | Opens the content lab window, catalogs local RA/Remastered roots, mounts archive members, and validates actual art/audio/video/text resources through an aspect-preserving thumbnail gallery plus focused preview/player and diagnostics panels |
| `docs/` | Local implementation notes | Currently placeholder directories only |
| `tests/` | Future integration test home | Currently placeholder only |
| `assets/` | Future test/sample assets | Currently placeholder only |
| `images/` | Branding/media assets | Logo, LM-ready, and Rust-inside imagery used by the README |

## Implemented Subsystems

### `ic-protocol`

- **Path:** `crates/ic-protocol/`
- **Primary responsibility:** shared serializable types at the future sim/net boundary
- **Owns:** `PlayerId`, `TickNumber`, `SubTickTimestamp`, `PlayerOrder`, `TimestampedOrder`, `TickOrders`, `MessageLane`, `FromClient<T>`, `FromServer<T>`
- **Does not own:** gameplay logic, networking transport, rendering, file IO
- **Key files to read first:** `src/lib.rs`, `src/tests.rs`
- **Tests / verification entry points:** YAML round-trip tests in `src/tests.rs`
- **Common change risks:** accidental protocol drift, bare integer IDs, adding fields without considering serialization compatibility
- **Related design decisions (`Dxxx`):** D006, D008, D012
- **Related execution steps (`G*`):** prepares for later `G6` and `G17`, even though those crates are not local yet
- **Search hints:** `PlayerOrder`, `TickOrders`, `MessageLane`, `FromClient`, `FromServer`

### `ic-cnc-content`

- **Path:** `crates/ic-cnc-content/`
- **Primary responsibility:** Iron Curtain-side integration for C&C-family content loading
- **Owns:** Bevy `Plugin`, Bevy `Asset` wrappers, Bevy `AssetLoader`s, importer-facing `.mix` / `.meg` staging helpers, parser-to-render handoff metadata for `.shp` / `.pal`, IC-specific compatibility decisions such as explicit `.miniyaml` loading
- **Does not own:** clean-room binary parsing rules, rendering, playback, game logic
- **Key files to read first:** `src/lib.rs`, then `src/source/mod.rs`, `src/mix/mod.rs`, `src/meg/mod.rs`, `src/shp/mod.rs`, `src/pal/mod.rs`, `src/aud/mod.rs`, `src/vqa/mod.rs`, `src/miniyaml/mod.rs`
- **Tests / verification entry points:** `src/tests.rs`, `src/source/tests.rs`, and each format module's `tests.rs`
- **Common change risks:** wrapper drift from `cnc-formats`, stale Bevy API assumptions, accidental over-claiming of file extensions, losing educational comments around Bevy concepts, letting source-probe schemas diverge from D069/D068 expectations, or making later importer/render crates rediscover format metadata instead of using the explicit handoff surfaces
- **Related design decisions (`Dxxx`):** D003, D023, D025, D027, D075, D076
- **Related execution steps (`G*`):** `G1`
- **Search hints:** `IcCncContentPlugin`, `AssetLoader`, `MixArchive`, `ShpSprite`, `Palette`, `AudAudio`, `VqaVideo`, `MiniYamlAsset`

### `ic-render`

- **Path:** `crates/ic-render/`
- **Primary responsibility:** render-side camera math and parser-to-render scene validation
- **Owns:** `IcRenderPlugin`, `GameCamera`, `ScreenToWorld`, `ClassicIsometricCameraModel`, `RenderLayer`, static-scene sprite validation built on `ic-cnc-content` handoff metadata
- **Does not own:** sim state, gameplay logic, original format parsing, final map loading, or the full sprite/material/render-graph implementation yet
- **Key files to read first:** `src/lib.rs`, then `src/camera/mod.rs`, `src/scene/mod.rs`
- **Tests / verification entry points:** `src/tests.rs`, `src/camera/tests.rs`, `src/scene/tests.rs`
- **Common change risks:** inventing sim-facing contracts before `ic-sim` exists locally, breaking the classic isometric projection math, drifting from the canonical RA draw-layer order, or making `ic-render` duplicate parser work already handled by `ic-cnc-content`
- **Related design decisions (`Dxxx`):** D017, D018, D041, D048
- **Related execution steps (`G*`):** `G2`
- **Search hints:** `IcRenderPlugin`, `GameCamera`, `ClassicIsometricCameraModel`, `ScreenToWorld`, `RenderLayer`, `StaticRenderScene`

### `ic-game`

- **Path:** `crates/ic-game/`
- **Primary responsibility:** runnable Bevy content lab for the first real-data viewport proof
- **Owns:** window/plugin setup, the synthetic SHP/PAL-backed background demo scene, the local content-source catalog, the mounted loose-file / `.mix` / `.meg` content graph, pure preview decoding for art/audio/video/text resources, the scrollable thumbnail gallery, the focused preview/player pane, Bevy-side transport controls, and the diagnostics panels that browse configured RA / Remastered roots
- **Does not own:** gameplay state, map loading, asset discovery, simulation, UI chrome, or the final palette-aware renderer
- **Key files to read first:** `src/lib.rs`, then `src/content_window/mod.rs`, `src/content_window/catalog.rs`, `src/content_window/state.rs`, `src/content_window/gallery.rs`, `src/content_window/preview_decode.rs`, `src/content_window/preview_audio.rs`, `src/content_window/preview.rs`, `src/content_window/tests.rs`, `src/demo.rs`, `src/tests.rs`, `src/main.rs`
- **Tests / verification entry points:** `src/tests.rs`, `src/content_window/tests.rs`
- **Common change risks:** pulling in Bevy platform features that do not match local CI environments, bypassing the `ic-cnc-content`/`ic-render` handoff with ad-hoc image loading, splitting pure decode logic from Bevy runtime logic incorrectly, splitting loose-file and archive-member handling into separate incompatible codepaths, distorting source aspect ratios in the gallery/inspector, letting gallery state drift from the selected preview runtime, or describing the content lab as a full game loop when it is only a visibility/browsing proof
- **Related design decisions (`Dxxx`):** D017, D041, D076
- **Related execution steps (`G*`):** `G2`
- **Search hints:** `run_content_window_client`, `ContentLabState`, `ContentCatalog`, `ContentEntryLocation`, `ContentGalleryTracker`, `ContainedImageSize`, `PreparedContentPreview`, `PcmAudioSource`, `ContentPreviewTracker`, `setup_content_gallery_ui`, `BootstrapDemoScene`, `setup_demo_scene`

## Repo Operations

### Local CI

- **Primary entry points:** `ci`, `ci.ps1`, `ci.cmd`, `ci-local.sh`, `ci-local.ps1`
- **Alignment target:** `.github/workflows/ci.yml`, with optional local `cargo deny` / `cargo audit` execution when those tools are installed
- **Current usage model:** use the stable `ci` dispatcher first, run `lint` on the host that owns the relevant `cfg(target_os)` code path, then `test`, then `all` for the wider policy suite
- **Common change risks:** letting local scripts drift from GitHub Actions, treating one host OS as proof for another, forgetting `--locked`, or changing MSRV in `Cargo.toml` without updating scripts and workflows
- **Search hints:** `MSRV`, `cargo deny`, `cargo audit`, `RUSTDOCFLAGS`

### GitHub Workflows

- **Primary files:** `.github/workflows/ci.yml`, `.github/workflows/audit.yml`, `.github/workflows/dco.yml`
- **Owns:** automated workspace validation, scheduled security audit, DCO commit-signoff enforcement
- **Does not own:** release packaging or crate publishing yet
- **Common change risks:** adding checks locally without CI parity, leaving platform-gated code covered on only one runner, using unrelated actions, or requiring tools not available on the selected runner

## Planned But Not Yet Implemented

These crates exist in the canonical design docs, not in the local workspace yet:

| Planned crate | Source of truth today |
| --- | --- |
| `ic-sim` | design docs architecture and determinism sections |
| `ic-ui` | design docs player-flow and UI decisions |
| `ic-audio` | design docs audio decisions and future integration notes |
| `ic-net` / `ic-server` | design docs netcode and relay decisions |
| `ic-script` | design docs modding and sandbox decisions |
| `ic-ai` | design docs AI decisions |
| `ic-llm` | design docs LLM decisions |
| `ic-editor` | design docs editor/SDK decisions |

When starting one of these crates locally, update:

- `Cargo.toml`
- `README.md`
- `CONTRIBUTING.md` if contributor workflow changes
- `AGENTS.md` if boundaries materially change
- `CODE-INDEX.md`

## Cross-Cutting Boundaries To Preserve

These matter now even if the downstream crates are not implemented yet.

1. `ic-protocol` remains the shared sim/net boundary crate.
2. `ic-cnc-content` stays an engine integration layer, not the clean-room parser implementation.
3. `ic-render` stays render-side: camera/view math and scene descriptors live here, not in `ic-cnc-content` or future sim crates.
4. `ic-game` is currently a thin executable/bootstrap crate: it should compose the other crates for visible proofs, not absorb their parser or render ownership.
5. Design-doc behavior changes are not settled locally without a design-gap or design-change update.
6. New code must follow the local documentation rules in `AGENTS.md`: test-first for behavior changes, context-rich docs, documented tests, and an LLM-friendly tree.
7. Repo-level automation should stay aligned across the Iron Curtain family when the policy is truly shared, but should not blindly copy library-specific release or publish workflows into the engine repo.

## Generated / Placeholder Areas

| Path | Current state | Edit policy |
| --- | --- | --- |
| `target/` | build output | do not commit; ignore for repo reasoning |
| `tests/` | placeholder only (`.gitkeep`) | add real integration tests here when they exist |
| `assets/` | placeholder only (`.gitkeep`) | add fixtures/samples here when they exist |
| `docs/design-gap-requests/` | placeholder only (`.gitkeep`) | add local design-gap notes here when needed |
| `docs/implementation-notes/` | placeholder only (`.gitkeep`) | add manual verification or local notes here when needed |

## Evidence Paths

- Workspace build: `cargo build --workspace`
- Workspace format check: `cargo fmt --all --check`
- Workspace tests: `cargo test --workspace --locked`
- Workspace clippy: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `ic-protocol` proofs: `crates/ic-protocol/src/tests.rs`
- `ic-cnc-content` proofs: `crates/ic-cnc-content/src/tests.rs`, `crates/ic-cnc-content/src/source/tests.rs`, and `crates/ic-cnc-content/src/*/tests.rs`
- `ic-render` proofs: `crates/ic-render/src/tests.rs`, `crates/ic-render/src/camera/tests.rs`, `crates/ic-render/src/scene/tests.rs`
- `ic-game` proofs: `crates/ic-game/src/tests.rs`, `crates/ic-game/src/content_window/tests.rs`
- Local repo automation: `ci`, `ci.ps1`, `ci.cmd`, `ci-local.sh`, `ci-local.ps1`, and `.github/workflows/*.yml`

## Maintenance Rules

- Update this file when crate names, file paths, repo-level workflow files, or ownership boundaries change.
- Prefer describing current local reality first and future architecture second.
- Do not list nonexistent files or crates as if they are already present.
- Keep task routing compact enough that a human or LLM can identify the right file set quickly.

## Execution Overlay Mapping

- **Milestone:** `M0`
- **Priority:** `P-Core`
- **Feature Cluster:** `M0.OPS.EXTERNAL_CODE_REPO_BOOTSTRAP_AND_NAVIGATION_TEMPLATES`
- **Depends on:** `M0.CORE.TRACKER_FOUNDATION`, `M0.CORE.DEP_GRAPH_SCHEMA`, `M0.OPS.MAINTENANCE_RULES`, `M0.QA.CI_PIPELINE_FOUNDATION`
