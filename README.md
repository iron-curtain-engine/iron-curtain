# Iron Curtain

<p align="center">
  <img src="images/logo.png" alt="Iron Curtain logo" width="280">
</p>

<p align="center">
  <a href="https://github.com/iron-curtain-engine/iron-curtain/actions/workflows/ci.yml"><img src="https://github.com/iron-curtain-engine/iron-curtain/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/iron-curtain-engine/iron-curtain/actions/workflows/audit.yml"><img src="https://github.com/iron-curtain-engine/iron-curtain/actions/workflows/audit.yml/badge.svg" alt="Security Audit"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="License"></a>
</p>

<p align="center">
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.85%2B-orange.svg" alt="Rust"></a>
  &nbsp;&nbsp;
  <img src="https://img.shields.io/badge/LM-ready-blueviolet.svg" alt="LM Ready"><br>
  <img src="images/rust_inside.png" alt="Rust-based project" width="74">
  &nbsp;
  <img src="images/lm_ready.png" alt="LM Ready" width="74">
</p>

A modern open-source RTS engine in Rust, starting with Command & Conquer.

*Red Alert first. Tiberian Dawn alongside it. The rest of the C&C family follows later.*

## Status

Iron Curtain is in early development.

- Active milestone: `M1`
- Active focus: `G2` content-lab bootstrap on top of completed `G1.1`-`G1.3`
  content-pipeline foundations
- Current workspace crates: `ic-protocol`, `ic-cnc-content`, `ic-render`, `ic-game`
- A runnable content-lab client exists, but no playable game build exists yet

## Design And Local Rules

Canonical architecture, roadmap, and design rationale live in the
[Iron Curtain design-doc repository](https://github.com/iron-curtain-engine/iron-curtain-design-docs).
The hosted book is:

**<https://iron-curtain-engine.github.io/iron-curtain-design-docs/>**

For local implementation work in this repo, read:

- `AGENTS.md` for coding-session rules and architectural invariants
- `CODE-INDEX.md` for current-file routing and repo navigation

## Repo Family

Iron Curtain is one repository in the wider `iron-curtain-engine` family.
Sibling repos currently include:

| Repository | Role |
| --- | --- |
| [`iron-curtain-design-docs`](https://github.com/iron-curtain-engine/iron-curtain-design-docs) | Canonical architecture, roadmap, and design decisions |
| [`cnc-formats`](https://github.com/iron-curtain-engine/cnc-formats) | Clean-room C&C binary format parsers and conversion tooling |
| [`fixed-game-math`](https://github.com/iron-curtain-engine/fixed-game-math) | Deterministic fixed-point math crate |
| [`deterministic-rng`](https://github.com/iron-curtain-engine/deterministic-rng) | Platform-identical deterministic random number generator |

## Building

```bash
cargo build --workspace --locked
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

You can also run the repo-local CI wrapper:

```bash
./ci lint
./ci test
./ci all
```

Underlying host-native wrappers still exist when you want them directly:

```bash
./ci-local.sh lint
./ci-local.sh test
./ci-local.sh all
```

On Windows PowerShell:

```powershell
./ci.ps1 lint
./ci.ps1 test
./ci.ps1 all
```

Or directly through the Windows host wrapper:

```powershell
./ci-local.ps1 lint
./ci-local.ps1 test
./ci-local.ps1 all
```

`ci` is the stable top-level dispatcher. It forwards to the correct host-native
wrapper so humans and agents only need one repo command name to remember.
Use `lint` first to get the current platform green before paying for the test
run, then use `all` for the broader local policy suite.

When touching non-host targets, run the dedicated target validation directly
where the required SDK/toolchain is available:

```bash
cargo check --workspace --target wasm32-unknown-unknown --locked
cargo clippy --workspace --target wasm32-unknown-unknown --locked -- -D warnings
cargo check --workspace --target aarch64-linux-android --locked
cargo clippy --workspace --target aarch64-linux-android --locked -- -D warnings
cargo check --workspace --target aarch64-apple-ios --locked
cargo clippy --workspace --target aarch64-apple-ios --locked -- -D warnings
```

GitHub Actions is the authoritative cross-platform enforcement path. The CI
matrix now runs `cargo check`, `cargo clippy`, and `cargo test` on Ubuntu,
Windows, and macOS, and it also runs dedicated `wasm32-unknown-unknown`,
`aarch64-linux-android`, and `aarch64-apple-ios` workspace `check` + `clippy`
lanes so web/mobile regressions are caught before merge.

## First Visible Slice

The repo now includes a narrow runnable content-lab bootstrap:

```bash
cargo run -p ic-game --locked
```

Today this opens a borderless-fullscreen Bevy content lab, keeps the synthetic
RA-style demo scene as background context, and mirrors the currently selected
real resource onto the main 2D preview surface. It scans the locally
configured Red Alert / Remastered roots, mounts logical members from `.mix` /
`.meg` archives into one content graph, and validates real `.shp`, `.pal`,
`.aud`, `.wav`, `.wsa`, `.vqa`, `.ini`, `.eng`, `.lut`, `.vqp`, `.fnt`, and
`.tmp` resources through one GUI with:

- a scrollable thumbnail wall for visual resources
- filename captions and diagnostics panels
- an aspect-preserving focused preview/player pane
- selected-resource transport controls

The lab is a resource-validation tool, not a full map loader or gameplay loop
yet.

The default source roots are now host-native: Windows builds probe `C:\...`
developer locations, while WSL/Linux builds probe the corresponding `/mnt/c`
paths. You can override them with `IC_RA1_SAMPLE_DISC_ROOT`,
`IC_RA1_SAMPLE_RAR`, `IC_RA1_SAMPLE_PALETTES`, and `IC_REMASTERED_ROOT`.
The catalog scan now runs in the background after the window opens, so large
installs should show a visible loading state instead of appearing hung before
the first frame. Heavy movie preview preparation also runs on a background
worker so selecting a VQA does not freeze the main UI thread before playback
starts.

Current controls:

- `Arrow keys`: browse the visible gallery window
- `PageUp` / `PageDown`: jump by gallery page
- `Home` / `End`: jump to the first or last previewable entry
- `Q` / `E`: switch source roots
- `Space`: play/pause the selected preview
- `Enter`: restart the selected preview
- `,` / `.`: step animation/video frames
- `Esc` twice: exit the fullscreen content lab

On Windows builds, the content lab wires decoded preview audio into Bevy's
audio runtime through a direct PCM path, covering decoded WAV/AUD data and VQA
soundtracks without synthesizing temporary WAV files. Non-Windows CI still
validates the decode, waveform, and animation paths without requiring system
audio libraries.

## Current Crates

| Crate | Purpose |
| --- | --- |
| `ic-protocol` | Shared wire types for the future simulation/network boundary |
| `ic-cnc-content` | Iron Curtain-side Bevy integration for legacy C&C content loading, including `.mix` / `.meg` archive wrappers |
| `ic-render` | Render-side camera bootstrap and static-scene validation for the future RA viewport |
| `ic-game` | Runnable Bevy content lab that catalogs real local Red Alert / Remastered roots, mounts archive members, and validates classic art/audio/video/text resources through an aspect-preserving gallery plus focused preview/player |

Additional crates from the full architecture will be added as local
implementation reaches later milestones.

## Standalone Crates (MIT/Apache-2.0)

These general-purpose libraries live in separate repositories under permissive
licenses for reuse outside the engine (D076):

| Crate | Repository | Purpose |
| --- | --- | --- |
| `cnc-formats` | [cnc-formats](https://github.com/iron-curtain-engine/cnc-formats) | Clean-room C&C binary format parsers |
| `fixed-game-math` | [fixed-game-math](https://github.com/iron-curtain-engine/fixed-game-math) | Deterministic fixed-point arithmetic |
| `deterministic-rng` | [deterministic-rng](https://github.com/iron-curtain-engine/deterministic-rng) | Seedable platform-identical PRNG |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a PR.

All contributions require a Developer Certificate of Origin (DCO). Add
`Signed-off-by` to your commits with `git commit -s`.

## License

Engine source code is licensed under **GPL-3.0-or-later** with the project’s
modding exception. YAML, Lua, and WASM mods loaded through the engine’s data
interfaces are not treated as derivative works.

See [LICENSE](LICENSE) for the full text.

## Trademark Disclaimer

Red Alert, Tiberian Dawn, Command & Conquer, and C&C are trademarks of
Electronic Arts Inc. Iron Curtain is not affiliated with, endorsed by, or
sponsored by Electronic Arts. These names are used only to identify the games
and formats the engine is intended to interoperate with.
