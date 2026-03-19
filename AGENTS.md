# AGENTS.md — Iron Curtain Engine

> Local implementation rules for the IC engine/game code repository.
> Canonical design authority lives in the Iron Curtain design-doc repository.

## Maintaining This File

AGENTS.md is read by stateless agents with no memory of prior sessions.
Every rule must stand on its own without session context.

- **General, not reactive.** Do not add rules just to memorialize one past
  mistake. Only codify patterns likely to recur across many sessions.
- **Context-free.** No references to specific conversations, commit hashes,
  or session artifacts. A future agent must understand every rule in
  isolation.
- **Principles over anecdotes.** Prefer durable guidance over story-like
  explanations of why a rule was added.
- **No stale specifics.** If a rule names a concrete file, crate, or command,
  it must be because that item is structurally important, not because it was
  the subject of a one-time debate.

## Canonical Design Authority (Do Not Override Locally)

This repository implements the Iron Curtain design. The canonical design sources are:

- Design docs repo: `https://github.com/iron-curtain-engine/iron-curtain-design-docs`
- Design-doc baseline revision: `HEAD` (pin to a specific tag/commit at bootstrap time)

Primary canonical planning and design references:

- `src/18-PROJECT-TRACKER.md` — execution overlay, milestone ordering, "what next?"
- `src/tracking/milestone-dependency-map.md` — dependency DAG and feature-cluster ordering
- `src/09-DECISIONS.md` — decision index (`Dxxx`)
- `src/02-ARCHITECTURE.md` — crate structure, sim/net/render architecture, determinism invariants
- `src/03-NETCODE.md` — relay protocol, `NetworkModel` trait, sub-tick fairness, anti-cheat
- `src/04-MODDING.md` — YAML → Lua → WASM modding tiers, sandbox boundaries
- `src/06-SECURITY.md` — threat model, trust boundaries, anti-cheat mitigations
- `src/17-PLAYER-FLOW.md` — UI navigation, screen flow, platform adaptations
- `src/architecture/type-safety.md` — newtype policy, fixed-point math, typestate, verified wrappers
- `src/architecture/crate-graph.md` — crate dependency DAG, async architecture, IoBridge trait
- `src/LLM-INDEX.md` — retrieval routing for humans/LLMs
- `src/16-CODING-STANDARDS.md` — file structure, commenting, naming, error handling, testing
- `src/coding-standards/quality-review.md` — review checklist and code quality bar
- `src/14-METHODOLOGY.md` — work-unit discipline and evidence requirements

## Non-Negotiable Rule: No Silent Design Divergence

If implementation reveals a missing detail, contradiction, or infeasible design path:

- do **not** silently invent a new canonical behavior
- open a design-gap/design-change request (see escalation workflow below)
- document the divergence rationale locally in `docs/design-gap-requests/`
- mark local work as one of:
  - `implementation placeholder`
  - `proposal-only`
  - `blocked on Pxxx`

If a design change is accepted, update the design-doc repo (or link to the accepted issue/PR) before treating it as settled.

## Non-Negotiable Architectural Invariants

These invariants are settled design decisions. Violating them is a bug, not a tradeoff.

### Invariant 1: Simulation is Pure and Deterministic

- `ic-sim` performs **no I/O** — no file access, no network calls, no system clock reads
- **Fixed-point math only** — `i32`/`i64` with scale factor 1024 (P002 resolved). Never `f32`/`f64` in sim-facing code
- **No HashMap/HashSet** — non-deterministic iteration order breaks lockstep. Use `BTreeMap`/`BTreeSet`/`IndexMap`
- Same inputs → identical outputs on all platforms, all compilers, all OSes
- **Enforcement:** `clippy::disallowed_types` in CI catches `f32`, `f64`, `HashMap`, `HashSet` in `ic-sim`

Related decisions: D009, D010, D012, D013, D015

### Invariant 2: Network Model is Pluggable via Trait

- `GameLoop<N: NetworkModel, I: InputSource>` is generic over both network and input
- `ic-sim` has **zero imports** from `ic-net` (and vice versa) — they share only `ic-protocol`
- Swapping lockstep for rollback touches zero sim code
- Shipping implementations: `RelayLockstepNetwork`, `LocalNetwork` (testing), `ReplayPlayback`

Related decisions: D006, D007, D008

### Invariant 3: Modding is Tiered (YAML → Lua → WASM)

- Each tier is optional and sandboxed
- No C# runtime, no recompilation required
- YAML for data (80% of mods), Lua for scripting (missions, abilities), WASM for total conversions
- WASM sandbox uses capability-based API — mods cannot request data outside their fog-filtered view

Related decisions: D004, D005, D023, D024, D025, D026

### Invariant 4: Every ID is a Wrapped Newtype

- Never use bare integers for domain IDs (`PlayerId(u32)`, not `u32`)
- Crypto hashes only constructible via compute functions (`Fingerprint::compute()`)
- State machines use typestate pattern — invalid transitions are compile errors
- Post-verification data uses `Verified<T>` wrapper — only verification functions can construct it
- Network messages branded with direction: `FromClient<T>`, `FromServer<T>`

Related decisions: type-safety invariants in `src/architecture/type-safety.md`

### Invariant 5: UI Never Mutates Authoritative Sim State

- `ic-ui` reads sim state through `SimReadView` (fog-filtered, read-only)
- UI emits `PlayerOrder` values that flow through the order pipeline
- Sim applies validated orders during `apply_tick()` — never directly from UI

Related decisions: D012, D041

## Crate Workspace

| Crate            | Responsibility                                                                                 | Phase |
| ---------------- | ---------------------------------------------------------------------------------------------- | ----- |
| `ic-protocol`    | Shared serializable types (`PlayerOrder`, `TimestampedOrder`, `TickOrders`, `MessageLane`)     | 0     |
| `ic-cnc-content` | Iron Curtain-side C&C content integration (`.mix`, `.shp`, `.pal`, `.aud`, `.vqa`, MiniYAML)  | 0–1   |
| `ic-paths`       | Platform path resolution (XDG/APPDATA/portable mode)                                           | 1     |
| `ic-sim`         | Pure deterministic simulation (fixed-point, no I/O, no floats)                                 | 2     |
| `ic-render`      | Bevy isometric map/sprite renderer, camera, fog rendering                                      | 1     |
| `ic-ui`          | Game UI and chrome (Bevy UI), sidebar, power bar, selection, menus                             | 3–4   |
| `ic-audio`       | Sound, music, EVA via Kira backend                                                             | 3     |
| `ic-net`         | `NetworkModel` implementations, `RelayCore` library                                            | 5     |
| `ic-server`      | Unified server binary (D074): relay + optional headless sim for FogAuth/cross-engine           | 5     |
| `ic-script`      | Lua (`mlua`) and WASM (`wasmtime`) mod runtimes, deterministic sandbox                         | 4–5   |
| `ic-ai`          | Skirmish AI (`PersonalityDrivenAi`), adaptive difficulty, economy/production/military managers | 4–6   |
| `ic-llm`         | LLM integration for adaptive missions, briefings, coaching (D016, D044, D073)                  | 6+    |
| `ic-editor`      | SDK: scenario editor, asset studio, campaign editor (D038, D040)                               | 6a–6b |
| `ic-game`        | Main game client binary — Bevy ECS orchestration, ties all systems together                    | 2+    |

**Critical crate boundaries:**

- `ic-sim` never imports `ic-net`, `ic-render`, `ic-ui`, `ic-audio`, `ic-editor`
- `ic-net` library never imports `ic-sim`
- `ic-server` is a top-level binary (like `ic-game`) that depends on `ic-net` for RelayCore and optionally `ic-sim` for FogAuth/relay-headless (D074)
- `ic-sim` and `ic-net` share only `ic-protocol`
- `ic-game` never imports `ic-editor` (separate binaries, shared libraries)
- `ic-sim` never reads/writes SQLite directly

## Implementation Overlay Discipline (Required)

Every feature implemented in this repo must reference the execution overlay.

Required in implementation issues/PRs:

- `Milestone:` `M0–M11`
- `Execution Step:` `G*`
- `Priority:` `P-Core` / `P-Differentiator` / `P-Creator` / `P-Scale` / `P-Optional`
- `Dependencies:` relevant `Dxxx`, cluster IDs, `Pxxx` blockers
- `Evidence planned:` tests/demo/replay/profile/ops notes

Do not implement features out of sequence unless the dependency map says they can run in parallel.

### Milestone Summary

| Milestone | Objective                                                      | Key G-Steps |
| --------- | -------------------------------------------------------------- | ----------- |
| M0        | Design baseline & tracker setup                                | —           |
| M1        | Resource fidelity + visual rendering slice                     | G1–G3       |
| M2        | Deterministic simulation core + combat slice                   | G4–G10      |
| M3        | Local playable skirmish (single machine, dummy AI)             | G11–G16     |
| M4        | Minimal online skirmish                                        | G17         |
| M5        | Campaign runtime vertical slice                                | G18         |
| M6        | Campaign completeness + skirmish AI maturity                   | G19         |
| M7        | Multiplayer productization (browser, ranked, trust, spectator) | G20         |
| M8        | Creator foundation (CLI, minimal Workshop, profiles)           | G21         |
| M9        | Full SDK editor + Workshop + OpenRA export                     | G22         |
| M10       | Campaign editor + game modes + RA1 export                      | —           |
| M11       | Ecosystem polish, optional AI/LLM, platform expansion          | —           |

## Source Code Navigation Index (Required)

This repo must maintain a code navigation file for humans and LLMs:

- `CODE-INDEX.md` (required filename)

See the filled-in template in the design docs at `src/tracking/ic-engine-code-index.md` for the initial version to copy.

Update `CODE-INDEX.md` in the same change set when code layout changes.

## Coding Session Discipline (Required)

These rules govern how implementation work is carried out in this repository.
They are not optional style preferences.

### 1. Test-First / Proof-First

- For every non-trivial behavior change, bug fix, parser rule, state
  transition, serialization path, boundary condition, or regression fix:
  **write or update the tests first** so the expected behavior is explicit
  before implementation changes begin.
- Tests are not cleanup. They are the primary proof artifact that the design
  was understood correctly and implemented correctly.
- The intended workflow is **red → green → refactor**:
  1. encode the requirement in a test
  2. observe the old implementation fail or lack the behavior
  3. implement the change
  4. rerun the tests to prove the new behavior
- If a task is purely structural (rename, move, formatting, comment-only
  cleanup) and has no behavioral delta, a new failing test is not required.
  But any task that changes runtime behavior must be test-led.
- If a true test-first path is impossible for a narrow case (for example,
  infrastructure scaffolding with no callable surface yet), document why and
  add the nearest executable proof in the same change set before claiming the
  work complete.
- When closing work, call out the exact tests, demos, replay captures, or
  benchmark artifacts that serve as evidence. "Implemented" without proof is
  not acceptable.

### 2. Commenting and Documentation for Context Isolation

- Write comments for the reader who lacks project context: a new maintainer,
  an occasional contributor, or an LLM reading one file in isolation.
- Every non-trivial module should begin with `//!` module docs that explain:
  - what the module owns
  - where it fits in the crate / system / pipeline
  - what depends on it or feeds into it
- Public structs, enums, error types, traits, and non-trivial functions or
  methods should have `///` doc comments that explain:
  - **what** the item does
  - **why** it exists / why this approach was chosen
  - important invariants, edge cases, and failure modes
- Inline `//` comments are required for non-obvious logic, algorithm phases,
  workarounds, safety guards, and domain-specific choices. Comments should
  explain *why this code is written this way*, not merely restate syntax.
- When code depends on an external framework, engine subsystem, or specialized
  library that a capable Rust reader may not already know, comments must teach
  the local mental model instead of assuming prior familiarity.
- For Bevy code in particular, explain the role of the Bevy concepts in use:
  what a `Plugin`, `App`, `System`, `Component`, `Resource`, `Asset`,
  `AssetLoader`, `Handle`, schedule hook, query, or event is doing in this
  specific file, and what behavior the engine is trying to achieve with it.
- Write framework-facing comments as onboarding notes for a maintainer learning
  the framework while reading the code. The standard is: the reader should be
  able to understand both **what this Bevy or library code does** and **why
  this project uses that mechanism here** without consulting outside material.
- Apply the same teaching standard to tests and setup code when they use
  framework-specific APIs, fixtures, lifecycle hooks, or builder patterns that
  would otherwise be opaque to a reader.
- Do not turn comments into line-by-line paraphrases of syntax. Focus on
  concepts, runtime behavior, ownership boundaries, data flow, and the reason a
  given framework feature was chosen over simpler or more direct alternatives.
- Constants and magic numbers must be documented with their origin and meaning
  when that meaning is not self-evident.
- Temporary workarounds, placeholders, and deferred behavior must be marked
  explicitly with the reason, scope limit, and blocker or later phase where
  they should be revisited.
- Avoid obvious comments like "increment counter". The code already says that.
  Spend comments on context, rationale, and constraints.

### 3. Test Documentation Standard

- Tests are part of the permanent design record. They must be readable without
  reverse-engineering the test body.
- Every non-trivial `#[test]` should have a `///` doc comment describing:
  - **what** scenario is being tested
  - **why** the scenario matters (bug, invariant, boundary, regression, security risk)
  - **how** the test is constructed when that is not obvious from the code
- Test names should describe the behavioral contract, not just the function
  under test.
- Test helpers must carry the same documentation standard as production code
  if they encode non-obvious binary layouts, fixtures, determinism setup, or
  scenario construction.

### 4. RAG / LLM-Friendly Project Tree

- The repository must stay navigable when read through file-by-file search,
  embeddings, or a limited context window. Structure the tree so a reader can
  load only the relevant files for the task at hand.
- Prefer small focused files over giant mixed-purpose files. As a rule of
  thumb, split files before they become hard to read in one pass; **~600 lines
  is the soft ceiling** for either production code or test files.
- For non-trivial modules, separate production code from heavy test scaffolding
  using directory modules such as:
  - `foo/mod.rs` for production logic
  - `foo/tests.rs` for unit tests
  - `foo/tests_validation.rs` or similarly named files for boundary/security/diagnostic tests when needed
- Keep test-only builders, fixtures, and scaffolding in test files unless they
  are genuinely shared by production code.
- Favor a stable top-to-bottom file layout so any reader knows where to look:
  module docs → imports → constants → types → impl blocks / functions → tests.
- When crate layout, module layout, or ownership boundaries change, update
  `CODE-INDEX.md` in the same change set so humans and LLMs can still route
  to the right files immediately.

## Design Change Escalation Workflow

When implementation reveals a conflict with canonical design docs:

1. Open an issue/PR in the design-doc repo (or designated design tracker) labeled `design-gap` or `design-contradiction`
2. Include:
   - target `M#` / `G*`
   - affected code paths and crates
   - affected canonical docs / `Dxxx` decisions
   - concrete conflict or missing "how"
   - proposed options and tradeoffs
   - impact on milestones/dependencies/priority
3. Document the divergence rationale locally:
   - a note in `docs/design-gap-requests/` with full reasoning
   - inline code comments at the divergence point referencing the issue
4. Link the request in the implementation PR/issue
5. Keep local workaround scope narrow until the design is resolved
6. If accepted, update the design-doc tracker/overlay in the same planning pass

### What Counts as a Design Gap

Open a request when:

- the docs specify *what* but not enough *how* for the target `G*` step
- two canonical docs disagree on behavior
- a new dependency/ordering constraint is discovered
- a feature requires a new policy/trust/legal decision (`Pxxx`)
- implementation experience shows a documented approach is not viable or perf-safe

Do **not** open a request for:

- local refactors that preserve behavior/invariants
- code organization improvements internal to one crate
- test harness additions that do not change accepted design behavior

## Local Repo-Specific Rules

- **Language:** Rust (2021 edition)
- **Build:** `cargo build --workspace`
- **Test:** `cargo test --workspace`
- **Lint:** `cargo clippy --workspace -- -D warnings`
- **Format:** `cargo fmt --all --check`
- **CI expectations:** All tests pass, clippy clean (zero warnings), fmt check clean. `clippy::disallowed_types` enforces determinism rules in `ic-sim`
- **Perf profiling:** `cargo bench` for hot-path microbenchmarks; Tracy/Superluminal for frame profiling
- **Security constraints:** No `unsafe` without review comment. WASM mods use capability-gated API only (D005). Order validation is deterministic (D012). Replay hashes use Ed25519 signing (D010)

## LLM / Agent Use Rules

- Read `CODE-INDEX.md` before broad codebase exploration
- Prefer targeted file reads over repo-wide scans once the index points to likely files
- Use canonical design docs (linked above) for behavior decisions; use local code/docs for implementation specifics
- If docs and code conflict, treat this as a design-gap or stale-code-index problem and report it — do not silently override
- Never introduce `f32`/`f64`/`HashMap`/`HashSet` in `ic-sim` — CI will reject it
- Never add I/O (file, network, clock) to `ic-sim`
- Never add `ic-net` imports to `ic-sim` or `ic-sim` imports to `ic-net`

## Evidence Rule (Implementation Progress Claims)

Do not claim a feature is complete without evidence:

- tests (unit, integration, or conformance)
- replay/demo capture demonstrating the feature
- benchmark results for perf-sensitive paths
- CI output showing clean build + test pass
- manual verification notes (if no automation exists yet)

## Current Implementation Target (Update Regularly)

- Active milestone: `M1`
- Active `G*` steps: `G1` (RA asset parsing), `G2` (Bevy isometric render), `G3` (unit animation)
- Current blockers: none known
- Parallel work lanes allowed: `G1` and `G2` can overlap (parser feeds renderer)

## Execution Overlay Mapping

- **Milestone:** `M0`
- **Priority:** `P-Core` (process-critical implementation hygiene)
- **Feature Cluster:** `M0.OPS.EXTERNAL_CODE_REPO_BOOTSTRAP_AND_NAVIGATION_TEMPLATES`
- **Depends on (hard):** `M0.CORE.TRACKER_FOUNDATION`, `M0.CORE.DEP_GRAPH_SCHEMA`, `M0.OPS.MAINTENANCE_RULES`, `M0.QA.CI_PIPELINE_FOUNDATION`
