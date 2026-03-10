# CODE-INDEX.md — Iron Curtain Engine

> Source code navigation index for humans and LLMs.
> Canonical design authority: `https://github.com/iron-curtain-engine/iron-curtain-design-docs` @ `HEAD`

## How to Use This Index

- Start with the **Task Routing** section to find the right subsystem
- Read the **Subsystem Index** entry before editing any crate
- Follow the **Do Not Edit / Generated** notes
- Use the linked tests/profiles as proof paths for changes
- If this index and the code disagree, the code is stale or this index needs updating — report it

## Current Scope / Build Target

- Active milestone(s): `M1`
- Active `G*` step(s): `G1` (asset parsing), `G2` (isometric render), `G3` (unit animation)
- Current focus area(s): RA resource loading pipeline → Bevy rendering slice
- Known blockers (`Pxxx` / external): none

## Task Routing (Start Here For X)

| If you need to...                         | Start here                       | Then read                                                         | Avoid touching first           |
| ----------------------------------------- | -------------------------------- | ----------------------------------------------------------------- | ------------------------------ |
| Parse RA1 assets (.mix, .shp, .pal, .aud) | `crates/ra-formats/`             | format tests, `src/05-FORMATS.md`                                 | sim/net/render paths           |
| Implement deterministic sim behavior      | `crates/ic-sim/`                 | `ic-protocol/`, conformance tests                                 | render/UI/net paths            |
| Work on netcode / relay timing            | `crates/ic-net/`                 | `ic-protocol/`, `src/03-NETCODE.md`                               | `ic-sim` internals             |
| Add UI/HUD feature                        | `crates/ic-ui/`                  | `ic-render/`, `src/17-PLAYER-FLOW.md`                             | core sim/net paths             |
| Add renderer feature (sprites, map, fog)  | `crates/ic-render/`              | `ic-sim/` read-only state, Bevy docs                              | sim mutation, net internals    |
| Add audio/music/EVA                       | `crates/ic-audio/`               | `ra-formats/` for .aud parsing, Kira docs                         | sim/net/render internals       |
| Add Lua/WASM mod feature                  | `crates/ic-script/`              | `ic-sim/` trait surface, `src/04-MODDING.md`                      | sim internals beyond trait API |
| Add AI behavior                           | `crates/ic-ai/`                  | `ic-sim/` read view, `ic-protocol/` orders                        | net/render/UI paths            |
| Add LLM integration feature               | `crates/ic-llm/`                 | `ic-sim/`, `ic-script/`, `src/decisions/09f/D016-llm-missions.md` | core sim/net hot paths         |
| Fix pathfinding bug                       | `crates/ic-sim/src/pathfinding/` | conformance tests, map fixtures                                   | unrelated gameplay systems     |
| Add editor/SDK feature                    | `crates/ic-editor/`              | `ic-render/`, `ic-sim/`, design docs D038/D040                    | `ic-game` binary integration   |
| Resolve platform paths                    | `crates/ic-paths/`               | `src/architecture/install-layout.md`                              | everything else                |
| Add/modify shared wire types              | `crates/ic-protocol/`            | `ic-sim/` + `ic-net/` consumers                                   | — (changes propagate widely)   |
| Set up game binary / orchestration        | `crates/ic-game/`                | all dependent crates, `src/architecture/game-loop.md`             | —                              |

## Repository Map (Top-Level)

| Path                  | Role                                        | Notes                                                                                       |
| --------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `crates/ic-protocol/` | Shared wire types                           | Boundary crate between sim and net                                                          |
| `crates/ra-formats/`  | RA1 asset parsers                           | Standalone, no game dependencies                                                            |
| `crates/ic-paths/`    | Platform path resolution + credential store | Standalone, wraps `app-path` + `strict-path` + `keyring` + `aes-gcm` + `argon2` + `zeroize` |
| `crates/ic-sim/`      | Deterministic simulation                    | Pure, no I/O, no floats                                                                     |
| `crates/ic-render/`   | Bevy isometric renderer                     | Reads sim state (read-only)                                                                 |
| `crates/ic-ui/`       | Game UI chrome (Bevy UI)                    | Reads sim + render state                                                                    |
| `crates/ic-audio/`    | Sound/music/EVA (Kira)                      | Reads ra-formats for .aud                                                                   |
| `crates/ic-net/`      | Networking + relay server                   | RelayCore lib + relay binary                                                                |
| `crates/ic-script/`   | Lua + WASM mod runtimes                     | Sandboxed, capability-gated                                                                 |
| `crates/ic-ai/`       | Skirmish AI + LLM strategies                | Reads sim state via fog-filtered view; depends on ic-llm                                    |
| `crates/ic-llm/`      | LLM provider traits + infra                 | No ic-sim import; candle inference runtime (D047)                                           |
| `crates/ic-editor/`   | SDK editor tools                            | Separate binary from ic-game                                                                |
| `crates/ic-game/`     | Main game binary                            | Orchestrates all systems                                                                    |
| `tests/`              | Integration test suites                     | Conformance, replay, determinism                                                            |
| `assets/`             | Test fixtures and sample maps               | Not shipped — test corpus only                                                              |
| `docs/`               | Implementation notes                        | Local docs, design-gap requests                                                             |

## Subsystem Index (Canonical Entries)

### `ic-protocol`

- **Path:** `crates/ic-protocol/`
- **Primary responsibility:** Shared serializable types that cross the sim/net boundary
- **Does not own:** game logic, network transport, rendering
- **Public interfaces / trait seams:** `PlayerOrder`, `TimestampedOrder`, `TickOrders`, `MessageLane`, `FromClient<T>`, `FromServer<T>`
- **Key files to read first:** `src/lib.rs` (all public types)
- **Hot paths / perf-sensitive files:** `TimestampedOrder` serialization (wire format, delta compression)
- **Generated files:** none
- **Tests / verification entry points:** unit tests for serialization round-trips
- **Related design decisions (`Dxxx`):** D006, D008, D012
- **Related execution steps (`G*`):** G6 (sim tick loop), G17 (online netcode)
- **Common change risks:** changes propagate to both `ic-sim` and `ic-net` — coordinate carefully
- **Search hints:** `PlayerOrder`, `TimestampedOrder`, `TickOrders`, `MessageLane`

### `ra-formats`

- **Path:** `crates/ra-formats/`
- **Primary responsibility:** Parse RA1 file formats (`.mix`, `.shp`, `.pal`, `.aud`, `.vqa`, MiniYAML)
- **Does not own:** rendering, audio playback, game logic
- **Public interfaces / trait seams:** asset loader functions, `MixArchive`, `ShpFrame`, `Palette`, MiniYAML parser
- **Key files to read first:** `src/lib.rs`, `src/mix.rs`, `src/shp.rs`
- **Hot paths / perf-sensitive files:** `.mix` archive lookup (used during asset loading)
- **Generated files:** none
- **Tests / verification entry points:** format parsing tests against known-good RA1 corpus
- **Related design decisions (`Dxxx`):** D003, D023, D025, D027, D075, D076
- **Related execution steps (`G*`):** G1 (asset parsing)
- **Common change risks:** format regressions against real RA1 assets; MiniYAML parser compatibility with OpenRA YAML
- **Search hints:** `mix`, `shp`, `pal`, `aud`, `vqa`, `MiniYAML`, `palette`

### `ic-paths`

- **Path:** `crates/ic-paths/`
- **Primary responsibility:** Platform path resolution — XDG on Linux, APPDATA on Windows, portable mode via `portable.marker`. Wraps `app-path` (exe-relative resolution) and `strict-path` (path boundary enforcement for untrusted inputs). Also owns `CredentialStore` — three-tier credential protection (OS keyring / vault passphrase / session-only) with AES-256-GCM per-column encryption for sensitive SQLite data
- **Does not own:** file I/O beyond path construction, asset loading
- **Public interfaces / trait seams:** `AppDirs`, `PathMode::Platform` / `PathMode::Portable`, `AppDirs::save_boundary()` / `mod_boundary()` / `replay_cache_boundary()` (→ `PathBoundary`), `CredentialStore`, `CredentialBackend`
- **Key files to read first:** `src/lib.rs`, `src/credentials.rs`
- **Hot paths / perf-sensitive files:** none (called once at startup; credential ops are infrequent)
- **Generated files:** none
- **Tests / verification entry points:** unit tests for path resolution on each platform; credential round-trip encryption tests
- **Related design decisions (`Dxxx`):** D061, D047
- **Related execution steps (`G*`):** G1 (asset discovery)
- **Common change risks:** platform-specific path bugs; portable mode detection; OS keyring API differences across Linux DEs
- **Search hints:** `AppDirs`, `PathMode`, `portable.marker`, `XDG`, `APPDATA`, `CredentialStore`, `CredentialBackend`, `vault_meta`

### `ic-sim`

- **Path:** `crates/ic-sim/`
- **Primary responsibility:** Pure deterministic simulation — game state evolution, fixed-point math, ECS world, order application
- **Does not own:** rendering, networking, audio, UI, file I/O, system clock
- **Public interfaces / trait seams:** `Simulation`, `SimReadView`, `SimSnapshot`, `DeltaSnapshot`, `GameModule` trait, `FogProvider` trait, `OrderValidator` trait, `AiStrategy` trait, `WorldPos`, `CellPos`, `SimCoord`, `UnitTag`, `Health`, `Mobile`, `Armament`, `Building`, `Selectable`
- **Key files to read first:** `src/lib.rs` (public API), `src/simulation.rs`, `src/types.rs` (newtypes)
- **Hot paths / perf-sensitive files:** `src/simulation.rs` (tick loop — runs ~15x/sec at default Slower speed, up to 50x/sec at Fastest), `src/pathfinding/` (hundreds of queries/tick), `src/combat.rs` (weapon fire, hit detection), `src/movement.rs` (unit movement), `src/spatial/` (spatial indexing queries)
- **Generated files:** none
- **Tests / verification entry points:** determinism conformance tests (same input → same output across platforms), unit tests per system, replay-based regression tests
- **Related design decisions (`Dxxx`):** D009, D010, D012, D013, D015, D018, D022, D028, D029, D041, D045
- **Related execution steps (`G*`):** G6 (tick loop), G7 (pathfinding), G8 (combat), G9 (projectiles), G10 (death/destruction), G11 (victory/failure)
- **Common change risks:** **determinism regressions** (any non-deterministic operation breaks lockstep for all players), allocations in hot loops, hidden I/O, float usage, HashMap usage
- **Search hints:** `Simulation`, `apply_tick`, `SimReadView`, `GameModule`, `FogProvider`, `OrderValidator`, `WorldPos`, `SimCoord`, `fixed_point`

### `ic-render`

- **Path:** `crates/ic-render/`
- **Primary responsibility:** Bevy isometric map rendering, sprite animation, camera control, fog-of-war visualization, render mode toggles (D048)
- **Does not own:** game logic, sim state mutation, audio
- **Public interfaces / trait seams:** Bevy render systems, camera controller, sprite animation pipeline
- **Key files to read first:** `src/lib.rs`, `src/map.rs`, `src/sprites.rs`, `src/camera.rs`
- **Hot paths / perf-sensitive files:** sprite batching, fog rendering, terrain draw calls
- **Generated files:** none
- **Tests / verification entry points:** visual regression tests (screenshot comparison), render benchmark
- **Related design decisions (`Dxxx`):** D002, D003, D048
- **Related execution steps (`G*`):** G2 (isometric render), G3 (unit animation), G15 (RA "feel" pass)
- **Common change risks:** Bevy API changes, GPU compatibility, frame budget overruns
- **Search hints:** `bevy`, `sprite`, `isometric`, `camera`, `fog`, `render_mode`

### `ic-ui`

- **Path:** `crates/ic-ui/`
- **Primary responsibility:** Game UI chrome — sidebar, power bar, minimap, selection panel, menus, settings. Reads SQLite for player analytics (D034)
- **Does not own:** sim state mutation, rendering pipeline, audio
- **Public interfaces / trait seams:** UI systems, `PlayerOrder` emission from UI actions
- **Key files to read first:** `src/lib.rs`, `src/sidebar.rs`, `src/selection.rs`
- **Hot paths / perf-sensitive files:** minimap updates, selection rectangle drawing
- **Generated files:** none
- **Tests / verification entry points:** UI interaction tests, layout regression tests
- **Related design decisions (`Dxxx`):** D032, D033, D034, D036, D053, D065
- **Related execution steps (`G*`):** G4 (cursor/hit-test), G5 (selection), G12 (mission-end UX)
- **Common change risks:** UX drift from design docs, platform adaptation gaps (desktop vs touch)
- **Search hints:** `sidebar`, `selection`, `minimap`, `power_bar`, `menu`, `egui`

### `ic-audio`

- **Path:** `crates/ic-audio/`
- **Primary responsibility:** Sound effects, music jukebox, EVA voice notifications via Kira audio backend
- **Does not own:** .aud file parsing (that's `ra-formats`), sim logic, rendering
- **Public interfaces / trait seams:** `CombatAudioEvent`, `EvaNotification`, `MusicStateChange`, jukebox state machine
- **Key files to read first:** `src/lib.rs`, `src/jukebox.rs`, `src/eva.rs`
- **Hot paths / perf-sensitive files:** concurrent sound effect mixing (combat scenes with many units)
- **Generated files:** none
- **Tests / verification entry points:** audio event trigger tests, jukebox state machine tests
- **Related design decisions (`Dxxx`):** D003 (P003 resolved: Kira via `bevy_kira_audio`)
- **Related execution steps (`G*`):** G13 (EVA/VO), G15 (RA "feel" pass)
- **Common change risks:** audio latency, platform-specific backend issues
- **Search hints:** `kira`, `eva`, `jukebox`, `CombatAudioEvent`, `MusicStateChange`

### `ic-net`

- **Path:** `crates/ic-net/`
- **Primary responsibility:** `NetworkModel` implementations, `RelayCore` library, `ic-server` binary, timing normalization, delta compression, sub-tick fairness (D008), order rate control (D060)
- **Does not own:** sim state mutation, order validation logic (that's `ic-sim`), rendering
- **Public interfaces / trait seams:** `NetworkModel` trait, `RelayCore`, `Connection<S>` (typestate: `Disconnected` → `Handshaking` → `Authenticated` → `InGame` → `PostGame`), `ClientMetrics`, `TimingFeedback`, `OrderBudget`, `AckVector`
- **Key files to read first:** `src/lib.rs`, `src/relay_core.rs`, `src/network_model.rs`
- **Hot paths / perf-sensitive files:** order serialization/delta compression (TLV wire format), relay tick broadcast, timing normalization
- **Generated files:** none
- **Tests / verification entry points:** relay integration tests, timing fairness tests, delta compression round-trip tests, rate-limit boundary tests
- **Related design decisions (`Dxxx`):** D006, D007, D008, D011, D052, D055, D060, D072
- **Related execution steps (`G*`):** G17 (minimal online), G20 (multiplayer productization)
- **Common change risks:** trust boundary violations (relay must not leak fog-hidden state), fairness drift in timing normalization, timestamp handling mismatches between platforms
- **Search hints:** `NetworkModel`, `RelayCore`, `Connection`, `TimestampedOrder`, `delta`, `relay`, `lockstep`

### `ic-script`

- **Path:** `crates/ic-script/`
- **Primary responsibility:** Lua (`mlua`) and WASM (`wasmtime`) mod runtimes with deterministic sandboxing and capability-based API
- **Does not own:** sim internals beyond the trait API surface, asset loading
- **Public interfaces / trait seams:** `ScriptRuntime`, `LuaState`, `WasmInstance`, `WasmSandbox<S>` (typestate: `Loading` → `Ready` → `Executing` → `Terminated`), capability tokens
- **Key files to read first:** `src/lib.rs`, `src/lua.rs`, `src/wasm.rs`, `src/capabilities.rs`
- **Hot paths / perf-sensitive files:** Lua function calls per tick (mission scripts), WASM host call boundary
- **Generated files:** none
- **Tests / verification entry points:** sandbox escape tests, determinism tests (same script → same output), API surface tests
- **Related design decisions (`Dxxx`):** D004, D005, D023, D024, D025, D026
- **Related execution steps (`G*`):** G18 (campaign runtime), G19 (campaign completeness)
- **Common change risks:** sandbox escape vulnerabilities, determinism regressions in WASM (NaN canonicalization), API surface creep exposing fog-hidden data
- **Search hints:** `lua`, `wasm`, `mlua`, `wasmtime`, `ScriptRuntime`, `capability`, `sandbox`

### `ic-ai`

- **Path:** `crates/ic-ai/`
- **Primary responsibility:** Skirmish AI — `PersonalityDrivenAi`, economy/production/military managers, adaptive difficulty, LLM-enhanced AI strategies (`LlmOrchestratorAi`, `LlmPlayerAi` — D044). Depends on `ic-llm` for provider access. Reads SQLite (D034) for player analytics and personalization
- **Does not own:** sim state mutation (emits `PlayerOrder` through `AiStrategy` trait), rendering, networking
- **Public interfaces / trait seams:** `AiStrategy` trait, `PersonalityDrivenAi`, `AiDifficulty` enum, personality config
- **Key files to read first:** `src/lib.rs`, `src/personality.rs`, `src/strategy.rs`
- **Hot paths / perf-sensitive files:** `decide()` function — must complete within <0.5ms for 500 units (D043 budget)
- **Generated files:** none
- **Tests / verification entry points:** AI decision tests, performance benchmarks, replay-based AI regression tests
- **Related design decisions (`Dxxx`):** D041, D042, D043, D044, D045
- **Related execution steps (`G*`):** G16 (basic AI for M3 skirmish), G19 (campaign AI maturity)
- **Common change risks:** performance budget overruns, fog-filter bypass (AI must use `FogFilteredView`, not raw sim state)
- **Search hints:** `AiStrategy`, `PersonalityDrivenAi`, `EconomyManager`, `ProductionManager`, `MilitaryManager`

### `ic-llm`

- **Path:** `crates/ic-llm/`
- **Primary responsibility:** LLM provider abstraction — four-tier provider system (D047: IC Built-in CPU models, Cloud OAuth, API Key, Local External), `LlmProvider` trait, prompt infrastructure, skill library (D057), pure Rust CPU inference runtime. Does NOT import `ic-sim` or `ic-ai` — traits and infra only. Reads SQLite (D034) for model pack state and provider config.
- **Does not own:** sim logic, rendering, core gameplay, AI strategies (those live in `ic-ai` which depends on `ic-llm`)
- **Public interfaces / trait seams:** `LlmProvider` trait, `ProviderTier` enum, `PromptAssembler`, prompt strategy profiles, skill library (D057), pure Rust inference runtime (Tier 1)
- **Key files to read first:** `src/lib.rs`, `src/provider.rs`, `src/mission_gen.rs`
- **Hot paths / perf-sensitive files:** none (LLM calls are async, not frame-budget-sensitive)
- **Generated files:** none
- **Tests / verification entry points:** prompt template tests, validation pipeline tests, mock provider tests
- **Related design decisions (`Dxxx`):** D016, D044, D047, D057, D073
- **Related execution steps (`G*`):** M11 (ecosystem polish, optional AI/LLM)
- **Common change risks:** provider API changes, prompt injection vulnerabilities, cost overruns from unthrottled LLM calls
- **Search hints:** `llm`, `provider`, `mission_gen`, `skill_library`, `prompt`, `D016`, `D047`

### `ic-editor`

- **Path:** `crates/ic-editor/`
- **Primary responsibility:** SDK tools — scenario editor (D038), asset studio (D040), campaign editor. Separate binary from `ic-game`
- **Does not own:** runtime gameplay, multiplayer, shipping game binary
- **Public interfaces / trait seams:** editor UI systems, content authoring pipeline
- **Key files to read first:** `src/lib.rs`, `src/scenario.rs`, `src/asset_studio.rs`
- **Hot paths / perf-sensitive files:** map rendering in editor (shares `ic-render`)
- **Generated files:** none
- **Tests / verification entry points:** editor workflow tests, asset import/export round-trip tests
- **Related design decisions (`Dxxx`):** D038, D040
- **Related execution steps (`G*`):** G22 (full SDK editor), M9–M10
- **Common change risks:** drift from game runtime behavior (editor and game must agree on sim rules)
- **Search hints:** `editor`, `scenario`, `asset_studio`, `campaign_editor`

### `ic-game`

- **Path:** `crates/ic-game/`
- **Primary responsibility:** Main game client binary — Bevy app setup, ECS scheduling, system orchestration, ties sim + render + UI + audio + net together
- **Does not own:** individual subsystem logic (delegates to crate APIs)
- **Public interfaces / trait seams:** `GameLoop<N, I>` orchestrator, Bevy `App` builder, observer systems for audio/UI events
- **Key files to read first:** `src/main.rs`, `src/app.rs`, `src/game_loop.rs`
- **Hot paths / perf-sensitive files:** frame loop (Bevy schedule), sim tick dispatch
- **Generated files:** none
- **Tests / verification entry points:** integration tests (full game loop with `LocalNetwork` + `ReplayPlayback`)
- **Related design decisions (`Dxxx`):** D002, D006 (generic game loop), D054 (async architecture)
- **Related execution steps (`G*`):** G2+ (progressively integrates all systems)
- **Common change risks:** system ordering bugs in Bevy schedule, frame budget overruns
- **Search hints:** `GameLoop`, `bevy::App`, `main`, `schedule`, `observer`

## Cross-Cutting Boundaries (Must Respect)

These rules prevent accidental architecture violations. Breaking them is a blocking code review issue.

1. **`ic-sim` must not import `ic-net`, `ic-render`, `ic-ui`, `ic-audio`, or `ic-editor`** — sim is pure and isolated
2. **`ic-net` must not import `ic-sim`** — they share only `ic-protocol`
3. **`ic-game` must not import `ic-editor`** — separate binaries, shared libraries only
4. **`ic-sim` must not perform I/O** — no file reads, no network calls, no system clock
5. **`ic-sim` must not use `f32`/`f64`/`HashMap`/`HashSet`** — determinism invariant enforced by CI
6. **UI must not mutate authoritative sim state** — emit `PlayerOrder`, never write to `Simulation` directly
7. **Protocol types are the shared boundary** — do not duplicate wire structs across crates
8. **AI must use `FogFilteredView`** — never access raw sim state (prevents accidental maphack in AI)
9. **WASM mods use capability-gated API only** — host controls what data mods can see

## Generated / Vendored / Third-Party Areas

| Path                  | Type              | Edit policy                                              |
| --------------------- | ----------------- | -------------------------------------------------------- |
| `target/`             | Build output      | Do not commit, gitignored                                |
| `assets/test-corpus/` | RA1 test fixtures | Replace via test scripts, do not hand-edit binary assets |

## Implementation Evidence Paths

Where to attach proof when claiming progress:

- Unit tests: `cargo test --workspace`
- Integration tests: `tests/` directory, `cargo test --test <name>`
- Determinism conformance: `tests/determinism/` (replay-based, cross-platform)
- Replay/demo artifacts: `tests/fixtures/replays/`
- Perf profiles/flamegraphs: `benches/`, Tracy/Superluminal captures in `docs/profiles/`
- Manual verification notes: `docs/implementation-notes/`

## Design Gap Escalation (When Code and Docs Disagree)

If implementation reveals a conflict with canonical design docs:

1. Record the code path and failing assumption
2. Link the affected `Dxxx` / canonical doc path
3. Open a design-gap/design-change issue in the design-doc repo
4. Document the divergence locally in `docs/design-gap-requests/`
5. Mark local workaround as `implementation placeholder` or `blocked on Pxxx`

## Maintenance Rules

- Update this file in the same change set when:
  - code layout changes (new crate, moved files, renamed modules)
  - ownership boundaries move
  - new major subsystem is added
  - active milestone/G* focus changes materially
- Keep "Task Routing" and "Subsystem Index" current — these are the highest-value sections for agents and new contributors
- Review this index at each milestone boundary

## Execution Overlay Mapping

- **Milestone:** `M0`
- **Priority:** `P-Core`
- **Feature Cluster:** `M0.OPS.EXTERNAL_CODE_REPO_BOOTSTRAP_AND_NAVIGATION_TEMPLATES`
- **Depends on:** `M0.CORE.TRACKER_FOUNDATION`, `M0.CORE.DEP_GRAPH_SCHEMA`, `M0.OPS.MAINTENANCE_RULES`, `M0.QA.CI_PIPELINE_FOUNDATION`
