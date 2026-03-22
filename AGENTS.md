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
| `ic-cnc-content` | Iron Curtain-side C&C content integration (`.mix`, `.shp`, `.pal`, `.aud`, `.vqa`, MiniYAML)   | 0–1   |
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
- Every problem, issue, or bug fixed must include a regression test and
  additional security/vulnerability tests to prevent regressions and
  exercise the relevant failure modes. These tests must be implemented as
  part of the resolution/patch (i.e., included in the same change set that
  fixes the issue) so the fix is verifiable and protected by automated
  checks.

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

### Error Design

- Use a **single shared `Error` enum** in `src/error.rs` for all modules.
- Every variant must carry **structured fields** (named, not positional) that
  provide enough context for callers to produce diagnostics without a debugger.
- Never use stringly-typed errors; prefer `&'static str` context tags over
  allocated `String`.
- Implement `Display` so the human-readable message embeds the numeric context
  (byte counts, offsets, limits).

### Integer Overflow Safety

- Use `saturating_add` (or `checked_add` where recovery is needed) at **every
  arithmetic boundary** where untrusted input influences the operands —
  especially `header_size + payload_size`, `offset + size`, and decompression
  output length calculations.
- This applies to both parsing paths and lookup/retrieval paths (e.g.
  `get_by_crc`).
- Never rely on Rust's debug-mode overflow panics as the safety mechanism;
  the code must be correct in release mode.

### Safe Indexing — No Direct Indexing in Production Code

Production code must **never** use direct indexing on **any type** —
`&[u8]`, `&str`, `Vec<T>`, or any other indexable container.  This applies
regardless of whether the index "feels safe" (e.g. derived from `.find()`
or bounded by a loop guard).  Direct indexing panics on out-of-bounds
access, which is a denial-of-service vector.

For **sequential processing**, use iterators, combinators, and transformers
(`.iter()`, `.map()`, `.filter()`, `.enumerate()`, `.zip()`, `.flat_map()`,
`.fold()`, etc.) instead of index-based loops. Prefer `.windows()`,
`.chunks()`, `.split()`, and similar slice iterators over manual index range
loops. When iterating with an index for bookkeeping, use `.enumerate()`
rather than a manual counter.

**Banned patterns (all of these panic on OOB):**

```rust
data[offset]           // byte slice indexing
data[start..end]       // byte slice range
line[pos..]            // string slicing
content[..colon_pos]   // string slicing with find()-derived index
entries[i].0           // vec/slice element access
bytes[i]               // byte array indexing
value.as_bytes()[0]    // first-byte access
```

**Required replacements:**

| Banned                | Replacement                                            |
| --------------------- | ------------------------------------------------------ |
| `data[offset]`        | `read_u8(data, offset)?` or `data.get(offset)`         |
| `data[start..end]`    | `data.get(start..end).ok_or(Error::…)?`                |
| `line[pos..]`         | `line.get(pos..).unwrap_or("")`                        |
| `&line[..pos]`        | `line.get(..pos).unwrap_or(line)`                      |
| `entries[i]`          | `entries.get(i).map(…)` or `entries.get_mut(i).map(…)` |
| `bytes[i]`            | `bytes.get(i) == Some(&val)`                           |
| `value.as_bytes()[0]` | `value.as_bytes().first()`                             |

**Binary parsers** should use the centralised safe-read helpers in
`src/read.rs`:

- `read_u8(data, offset)` — reads one byte via `.get()`
- `read_u16_le(data, offset)` — reads two bytes via `.get()`, little-endian
- `read_u32_le(data, offset)` — reads four bytes via `.get()`, little-endian

All helpers return `Result<_, Error::UnexpectedEof>` with structured context
(needed offset, available length).  They use `checked_add` internally to
prevent integer overflow on offset arithmetic.

**Text parsers** should use `.get()` with `.unwrap_or("")` (or
`.unwrap_or(original)` when the fallback is the unsliced source).
Even though `str::find()` returns valid UTF-8-aligned indices, the rule
is absolute — no reviewer should ever need to *reason* about whether an
index is safe.  If it compiles without `.get()`, it's wrong.

**Test code** (`#[cfg(test)]` blocks) may use direct indexing when the test
controls the input and panic-on-bug is acceptable.

### No `.unwrap()` in Production Code

Production code must **never** call `.unwrap()`, `.expect()`, or any method
that panics on `None`/`Err`. Use `?`, `.ok_or()`, `.map_err()`, or
`.unwrap_or()` instead.

**Test code** may use `.unwrap()` freely — a panic in a test is an acceptable
failure mode.

### Type Safety — Make Invalid States Unrepresentable

- Use the Rust type system to **prevent invalid, incorrect, or ambiguous
  states at compile time** rather than guarding against them at runtime.
- **Enum state machines over boolean flags.** When an object moves through
  distinct phases (e.g., loading → streaming → ready), model each phase as
  an enum variant carrying only the data valid for that phase. This makes
  impossible states (such as "has audio info but hasn't finished loading")
  structurally unrepresentable.
- **Newtypes for domain identifiers.** Use newtype wrappers for domain-specific
  integer identifiers to prevent accidental mixing of semantically different
  values. The newtype should:
  - Derive: `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`
  - Provide `from_raw(value) -> Self` and `to_raw(self) -> inner` accessors
  - Implement `Display` with a human-readable format
- **Current newtypes:**

  | Type     | Inner | Module | Purpose                        |
  | -------- | ----- | ------ | ------------------------------ |
  | `MixCrc` | `u32` | `mix`  | Westwood MIX filename CRC hash |

- When adding new format modules, evaluate whether key identifiers (offsets,
  indices, hashes) would benefit from newtype wrapping. Apply newtypes where
  misuse could cause silent data corruption or security issues — not for every
  integer.
- **Typestate where appropriate.** When an API has a mandatory call sequence
  (build → configure → finalize), encode each step as a distinct type so
  callers cannot skip or reorder steps. In Bevy ECS contexts where a single
  concrete `Resource` type is required, prefer an internal enum over
  typestate on the outer type.
- **`Option` / `Result` over sentinel values.** Never use `-1`, `0`,
  `""`, or `null`-equivalent magic values to signal absence. Use `Option`
  or `Result` so the compiler forces callers to handle the missing case.
- **Visibility and constructor control.** Keep struct fields private and
  expose transition methods that enforce invariants. If a struct can only
  be in a valid state when constructed through specific paths, make the
  invalid construction path impossible rather than documenting "don't do
  this."
- **Exhaustive matching.** Prefer `match` over `if let` when handling enums
  so that adding a new variant produces a compile error at every site that
  must handle it, rather than silently falling through.

### Lifetime Naming

- Lifetime parameter names must be meaningful: name the lifetime after the
  item whose lifetime it represents (for example `'input` for an input
  slice, `'buf` for a buffer, `'frame` for frame data, `'palette` for a
  borrowed palette). Avoid vague single-letter names like `'a` in public
  APIs; single-letter lifetimes may be acceptable in very small local
  scopes or short-lived closures.
- Prefer descriptive lifetime names in structs and function signatures so
  reviewers and automated tools can immediately identify what is being
  borrowed and why. This improves readability and reduces confusion when
  multiple lifetimes are present.

### Parser Design Philosophy

- **Parsers are pure functions** of their input (`&[u8]`). No hidden state,
  no side effects, no filesystem access. Calling a parser twice on the same
  input must yield identical results.
- **Permissive on unknown values.** Parsers accept unrecognised enum values
  (e.g. compression IDs, flags) and store them as-is. Callers decide whether
  they can handle the value. This supports future and modded game files.
- **Strict on structural integrity.** Offsets, sizes, and counts must be
  validated against actual buffer lengths before any slice operation.

### `std` and Allocation

- The crate uses `std`. Use standard library types (`Vec`, `String`, `HashMap`)
  as appropriate.
- The `&[u8]` parsing API remains the primary interface (callers provide bytes).
  Large container formats should also expose reader-based streaming APIs when
  they materially reduce whole-file memory use.

### Heap Allocation Policy

This crate processes game assets in real-time contexts. Minimise heap
allocation to reduce allocator overhead, GC pauses, and memory fragmentation.

**Rules (in priority order):**

1. **Hot paths must not heap-allocate.** Any function called per-frame, per-lookup,
   or per-byte (e.g. `crc()`, LCW command handlers, ADPCM nibble decode) must be
   zero-allocation. Use stack buffers, byte-by-byte processing, or iterator
   patterns instead of `String`, `Vec`, or `Box`.

2. **Parsers should borrow, not copy.** When the parsed result can reference the
   input slice (via `&'a [u8]`), prefer borrowing over `.to_vec()`. This
   eliminates per-entry allocations during bulk parsing. Example: `ShpFrame<'a>`
   borrows frame data from the input; `MixArchive<'a>` borrows the data section.

3. **Fixed-size scratch buffers belong on the stack.** When the maximum size is
   bounded and small (≤ ~4 KB), use a `[T; N]` array instead of `Vec<T>`.
   Example: BigNum double-width multiplication buffers in `mix_crypt` use
   `[u32; BN_DOUBLE]` (516 bytes) instead of `vec![0u32; len]`.

4. **`Vec::with_capacity` for necessary allocations.** When a heap allocation
   is unavoidable (variable-length output like decompressed pixel data), always
   use `Vec::with_capacity(known_size)` to avoid reallocation.

5. **Prefer bulk operations over per-element loops.**
   - `Vec::extend_from_slice` over N × `push` for literal copies (memcpy).
   - `Vec::extend_from_within` over N × indexed-push for non-overlapping
     back-references (memcpy from self).
   - `Vec::resize(len + n, value)` over N × `push(value)` for fills (memset).
   These let the compiler emit SIMD/vectorised memory operations.

6. **`#[inline]` on small hot functions.** Trivial accessors
   (`from_raw`/`to_raw`, `is_stereo`, `has_embedded_palette`), CRC computation,
   binary-search lookup (`get`, `get_by_crc`), and the safe-read helpers must
   carry `#[inline]` to guarantee inlining across crate boundaries.

7. **Release profile optimisation.** `Cargo.toml` specifies `lto = true` and
   `codegen-units = 1` for release builds, enabling cross-crate inlining and
   whole-program dead-code elimination.

**Current allocation profile by module:**

| Module      | Parse-time allocs       | Runtime allocs       | Notes                       |
| ----------- | ----------------------- | -------------------- | --------------------------- |
| `mix`       | 1 (entry Vec)           | 0 per lookup         | `crc()` is zero-alloc       |
| `pal`       | 0                       | 0                    | Fixed `[PalColor; 256]`     |
| `shp`       | 2 (offset + frame Vecs) | 1 per `pixels()`     | Frame data borrows input    |
| `aud`       | 0 (borrows input)       | 1 per `decode_adpcm` | With-capacity Vec           |
| `lcw`       | 1 (output Vec)          | —                    | With-capacity, bulk ops     |
| `mix_crypt` | 1 (decrypt output)      | 0 in RSA loop        | BigNum is stack `[u32; 64]` |
| `tmp`       | 1 (tile Vec)            | 0                    | Tile data borrows input     |
| `vqa`       | 2 (chunk + frame Vecs)  | 0                    | Chunk data borrows input    |
| `wsa`       | 2 (offset + frame Vecs) | 0                    | Frame data borrows input    |
| `fnt`       | 1 (glyph Vec)           | 0                    | Glyph data borrows input    |
| `ini`       | 3 (HashMap + 2 Vecs)    | 0                    | String allocs per entry     |
| `miniyaml`  | N (node tree)           | 0                    | String allocs per node      |
| `cps`       | 1 (output Vec)          | 0                    | LCW decompress or raw copy  |
| `shp_ts`    | 2 (offset + frame Vecs) | 1 per `pixels()`    | Frame data borrows input    |
| `vxl`       | 3 (headers + tailers)   | 0                    | Body data borrows input     |
| `hva`       | 2 (names + transforms)  | 0                    | Float data parsed into Vec  |
| `w3d`       | N (chunk tree)          | 0                    | Leaf data borrows input     |
| `csf`       | N (category + strings)  | 0                    | String data parsed into Vec |

### Implementation Comments (What / Why / How)

A reviewer should be able to learn and understand the entire design by reading
the source alone — without consulting external documentation, git history, or
the original author.

Every non-trivial block of implementation code must carry comments that answer
up to three questions:

1. **What** — what this code does (one-line summary above the block or method).
2. **Why** — the design decision, security invariant, or domain rationale that
   motivated this approach over alternatives.
3. **How** (when non-obvious) — algorithm steps, bit-level encoding, reference
   to the original format spec or EA source file name.

Specific guidance:

- **Constants and magic numbers:** document the origin and meaning.  If a
  constant derives from a V38 security cap, say so.  If it mirrors a value
  from the original game binary, name the source file.
- **Section headers:** use `// ── Section name ───…` comment bars to visually
  separate logical phases within a long function (e.g. header parsing, offset
  table, frame extraction).
- **Safety-critical paths:** every V38 guard (ratio cap, output limit,
  bounds check, forward-progress assertion) must have an inline comment
  explaining *what* it prevents and *why* the chosen limit is correct.
- **Algorithm steps:** multi-step algorithms (LCW commands, IMA ADPCM nibble
  decode, CRC accumulation) should have per-step inline comments so a reader
  can follow the logic without cross-referencing an external spec.
- **Permissive vs. strict:** where the parser intentionally accepts values it
  doesn't recognise (unknown compression IDs, out-of-range palette bytes),
  comment that the permissiveness is deliberate and why.

This standard applies equally to production code and test helpers (e.g.
`build_shp`, `build_aud`).  The same what/why/how structure used for `#[test]`
doc comments (see Testing Standards below) applies to implementation code via
`///` doc comments on public items and `//` inline comments on internal logic.

### 3. Testing Standards

#### Test Documentation

Every `#[test]` function must have a `///` doc comment with up to three
paragraphs:

1. **What** (first line) — the scenario being tested.
2. **Why** (second paragraph) — the security invariant, correctness guarantee,
   or edge-case rationale that motivates the test.
3. **How** (optional third paragraph) — non-obvious test construction details
   (byte encoding, overflow mechanics, manual binary layout).

Omit the "How" paragraph when the test body is self-explanatory.

Test names should describe the behavioral contract, not just the function
under test. Test helpers must carry the same documentation standard as
production code if they encode non-obvious binary layouts, fixtures,
determinism setup, or scenario construction.

#### Doc Examples Must Compile and Pass

All `///` and `//!` code examples (doctests) must compile, run, and pass.
Never use `no_run`, `ignore`, or `compile_fail` annotations to skip execution.
If a code example requires filesystem access, network, or other unavailable
resources, rewrite it to use in-memory data so it runs in CI without external
dependencies.

#### Test Organisation

Tests within each module are grouped under section-comment headers:

```rust
// ── Category name ────────────────────────────────────────────────────
```

Standard categories (in order): basic functionality, error field & Display
verification, known-value cross-validation, determinism, boundary tests,
integer overflow safety, security edge-case tests.

#### Required Test Categories

Every parser module must include tests for:

- **Happy path:** parse well-formed input, verify fields.
- **Error paths:** each `Error` variant the module can return must be tested,
  including verification that structured fields carry correct values.
- **Display messages:** at least one test asserting `Error::Display` output
  contains the key numeric values.
- **Determinism:** parse (or decode) the same input twice, assert equality.
- **Boundary:** test both sides of every limit (exactly at cap succeeds,
  one past cap fails; minimum valid input succeeds, one byte short fails).
- **Overflow safety:** craft inputs with `u32::MAX` or near-max values to
  exercise `saturating_add` / bounds-check paths; assert no panic and
  correct error return.

#### Security Testing (V38)

Every parser module must include **adversarial** tests that exercise the V38
safety invariants with crafted malicious inputs.  These tests ensure that
future changes do not regress the security guarantees.

#### Test Fixture Legality and CI Portability

- Never commit proprietary, copyrighted, or otherwise redistribution-restricted
  game assets to this repository unless there is a clearly documented license
  that explicitly allows public redistribution in git.
- Do not assume that "modding is allowed" means "raw assets may be checked into
  source control." Code, mods, and owned-install import workflows are separate
  questions from public asset redistribution.
- CI-required tests must be self-contained and legally redistributable. The
  default solution is:
  - generate tiny synthetic fixtures inline
  - build minimal valid binary payloads with local test helpers
  - use authored/open assets owned by this project
  - assert metadata, decode behavior, round-trips, and invariants without
    shipping original EA payloads
- When real installed assets are useful for extra validation, keep that path
  opt-in and local-only:
  - gate it behind explicit environment variables or ignored/manual tests
  - never make GitHub Actions depend on proprietary local installs
  - document the source expectation and ownership requirement in the test docs
- Prefer generated fixtures over opaque checked-in binaries. Generated fixtures
  keep the legal status clearer, the test intent more readable, and the CI
  story portable across fresh runners.

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
- **Build:** `cargo build --workspace --locked`
- **Test:** `cargo test --workspace --locked`
- **Lint:** `cargo clippy --workspace --all-targets --locked -- -D warnings`
- **Format:** `cargo fmt --all --check`
- **Build/run rule:** Agents must **not** use `cargo build`, `cargo run`, or equivalent binary/example launch commands for this repo unless the user explicitly requests them. Let the user build and run the project in their own environment.
- **Why this rule exists:** The agent environment may not match the user's local graphics, windowing, driver, audio, or platform setup. Pure build/run attempts can waste time while proving less than the same compile work done through targeted tests or lint checks.
- **Allowed verification paths:** `cargo clippy`, `cargo test`, `cargo check`, `cargo fmt --check`, `./ci`, `ci-local.sh`, `ci-local.ps1`, and other non-run validation commands are allowed. Prefer the repo dispatcher first, then direct cargo commands when a narrower probe is enough.
- **Host-native validation rule:** `cfg(target_os)` and other platform-gated native code is only considered validated when linted on that host OS or by the GitHub Actions OS matrix. Non-host targets such as `wasm32-unknown-unknown`, `aarch64-linux-android`, and `aarch64-apple-ios` are only considered validated when checked/clippied for that exact target or by their dedicated GitHub Actions lanes. Use `./ci lint` on Unix-like hosts, `./ci lint --host windows` from WSL when Windows PowerShell is available, `.\ci.ps1 lint` on native Windows, and direct target `cargo check` / `cargo clippy` commands for web/mobile targets before claiming those code paths are green.
- **CI expectations:** All tests pass, clippy clean (zero warnings), fmt check clean, and the GitHub Actions matrix stays green on Ubuntu, Windows, macOS, plus the dedicated `wasm32-unknown-unknown`, `aarch64-linux-android`, and `aarch64-apple-ios` lanes. `clippy::disallowed_types` enforces determinism rules in `ic-sim`
- **Perf profiling:** `cargo bench` for hot-path microbenchmarks; Tracy/Superluminal for frame profiling
- **Security constraints:** No `unsafe` without review comment. WASM mods use capability-gated API only (D005). Order validation is deterministic (D012). Replay hashes use Ed25519 signing (D010)

## LLM / Agent Use Rules

- Read `CODE-INDEX.md` before broad codebase exploration
- Prefer targeted file reads over repo-wide scans once the index points to likely files
- Use canonical design docs (linked above) for behavior decisions; use local code/docs for implementation specifics
- If docs and code conflict, treat this as a design-gap or stale-code-index problem and report it — do not silently override
- Never use `cargo build`, `cargo run`, or similar pure build/run commands unless the user explicitly asks; prefer `cargo clippy` first, then `cargo test`, and use `cargo check` as a lighter fallback
- Treat `./ci lint` as the preferred repo-level validation entrypoint before falling back to `ci-local.*` or ad-hoc cargo commands for partial checks
- When a task would normally end with “run the app locally”, provide the exact user-run command instead of executing it yourself
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
- Active `G*` steps: `G2` (Bevy content-lab/bootstrap render slice), with `G1` content-pipeline foundations feeding it and `G3` (unit animation) next
- Current blockers: none known
- Parallel work lanes allowed: `G1` and `G2` can overlap (parser feeds renderer)

## Execution Overlay Mapping

- **Milestone:** `M0`
- **Priority:** `P-Core` (process-critical implementation hygiene)
- **Feature Cluster:** `M0.OPS.EXTERNAL_CODE_REPO_BOOTSTRAP_AND_NAVIGATION_TEMPLATES`
- **Depends on (hard):** `M0.CORE.TRACKER_FOUNDATION`, `M0.CORE.DEP_GRAPH_SCHEMA`, `M0.OPS.MAINTENANCE_RULES`, `M0.QA.CI_PIPELINE_FOUNDATION`
