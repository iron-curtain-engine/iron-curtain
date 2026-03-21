# DG001 — Remastered `.bk2` Support Boundary

> Status: `proposal-only`
> Date: `2026-03-20`

## Execution Overlay

- `Milestone:` `M1`
- `Execution Step:` `G2`
- `Priority:` `P-Core`
- `Dependencies:` `D003`, `D017`, `D041`, `D076`
- `Evidence planned:` local design note, upstream parser/API proposal, optional backend spike, CI-safe synthetic tests only

## Summary

Iron Curtain should support Remastered content broadly, but `.bk2` support
should **not** be treated as ordinary clean-room format work inside the current
`cnc-formats` scope.

Recommended boundary:

1. keep `cnc-formats` focused on classic Westwood/C&C-family formats and
   engine-agnostic media primitives we can justify as clean-room baseline work
2. support Remastered `.meg`, `.wav`, `.dds`, text/config, and related assets
   in the ordinary engine pipeline now
3. treat `.bk2` as a **separate optional backend decision** with explicit legal
   and architectural acceptance criteria
4. if pursued, implement `.bk2` in an isolated crate or feature-gated backend
   first, then wrap it from `ic-cnc-content` / `ic-game`

## Why This Is A Design Gap

The current design docs and local repo architecture cover:

- classic RA content fidelity
- clean-room parser boundaries
- Bevy-side content integration
- the content-lab/render slice

But they do **not** yet settle:

- whether proprietary Remastered movie playback belongs in the parser baseline
- whether `.bk2` should be a required capability or an optional enhancement
- whether an official SDK backend is acceptable
- whether a true clean-room `.bk2` decoder is inside project legal risk
  tolerance

That makes `.bk2` support a policy/architecture gap, not just an unimplemented
decoder.

## Current Evidence

### What `.bk2` Is

`.bk2` is Bink 2 video, a proprietary movie format used by many games,
including Remastered-era assets. It is not one of the original classic
Westwood formats such as `.vqa`, `.aud`, `.wsa`, `.shp`, or `.mix`.

### Why Caution Is Required

- RAD Game Tools presents Bink/Bink 2 as a commercial licensed SDK/product, not
  an open published implementation baseline:
  - `https://www.radgametools.com/binkfaq.htm`
  - `https://www.radgametools.com/binksdk.htm`
  - `https://www.radgametools.com/bnkmain.htm`
- The public Remastered source release does **not** provide a ready-made open
  `.bk2` playback path for us to adopt. Its README only states that the repo
  contains the preserved DLL/editor source and that owners still need the game:
  - `https://raw.githubusercontent.com/electronicarts/CnC_Remastered_Collection/master/README.md`
- FFmpeg documentation confirms that the broader Bink family is technically
  decodable in open-source tooling, and FFmpeg-devel shows a Bink 2 decoder
  patch series existed, which proves feasibility:
  - `https://ffmpeg.org/general.html`
  - `https://ffmpeg.org/pipermail/ffmpeg-devel/2022-May/296763.html`

Inference from those sources:

- `.bk2` support is technically possible
- `.bk2` support is not automatically policy-safe just because open-source
  decoder work exists elsewhere
- the project should decide this deliberately before it becomes a hidden
  dependency in the media pipeline

## Recommendation

### Decision

For now, `.bk2` should **not** be part of the required `cnc-formats` clean-room
baseline.

Instead:

- `cnc-formats` remains responsible for classic C&C/Westwood media and the
  incremental media APIs needed by the engine
- Iron Curtain should continue implementing Remastered support for assets that
  fit the current clean-room and engine-wrapping model:
  - `.meg`
  - `.wav`
  - `.dds`
  - text/config/metadata surfaces
- `.bk2` should be isolated behind an **optional backend boundary**

### Preferred Ownership

Preferred initial location if pursued:

- a separate optional crate or feature-gated backend, for example:
  - `ic-bk2`
  - `ic-remastered-video`
  - or an equivalent isolated media backend crate

Not preferred as the first step:

- putting `.bk2` directly into the core `cnc-formats` baseline

Reason:

- it mixes a high-risk proprietary-adjacent codec decision into the general
  clean-room parser crate
- it raises legal/patent/licensing review questions that do not apply equally
  to the classic format work
- it may introduce platform/backend constraints that the ordinary parser crate
  should not inherit

## Acceptance Criteria

`.bk2` support should not be accepted into the Iron Curtain family until all of
the following are satisfied.

### Legal / Policy

1. A written project decision exists for which path is being used:
   - official RAD SDK backend
   - true clean-room decoder
   - or deferred/no support
2. If the path is clean-room:
   - no RAD SDK
   - no RAD headers
   - no copied proprietary code
   - no decompiled assets/code used as implementation input
3. If the path is SDK-based:
   - license terms, redistribution constraints, and packaging limits are
     explicitly documented
4. The backend is optional by default until its licensing posture is accepted
   for normal distribution

### Technical

1. The backend exposes metadata early:
   - dimensions
   - duration/frame cadence
   - audio presence and audio metadata
2. Playback can start with low latency:
   - no full-movie decode requirement before first frame
   - small preroll
   - bounded buffering
3. Audio can act as the master clock when present
4. The runtime supports modern presentation expectations:
   - contain-fit fullscreen playback
   - no aspect distortion
   - explicit pause/restart/seek behavior
5. The backend does not force `cnc-formats` or `ic-cnc-content` to inherit
   unrelated proprietary/runtime baggage when the feature is disabled

### Testing / Evidence

1. CI-safe tests use only legal fixtures:
   - authored/open fixtures if available
   - generated structural fixtures where possible
   - no proprietary Remastered movie assets in git
2. Real `.bk2` validation remains local-only or separately gated until the
   project explicitly approves a lawful redistributable fixture strategy
3. The runtime demonstrates:
   - first-frame latency
   - A/V sync correctness
   - error handling for malformed/truncated input
   - stable behavior across supported platforms

## Implementation Paths

### Path A — Recommended Near-Term Path

Defer `.bk2` playback and continue Remastered support around the other asset
families first.

This gives Iron Curtain immediate value with lower risk:

- `.meg` browsing and extraction
- `.wav` playback
- `.dds` image presentation
- text/config inspection
- classic `.vqa` / `.aud` / `.wsa` / `.shp` support

This path should remain the default until a `.bk2` policy decision is accepted.

### Path B — Optional Official SDK Backend

Use RAD's official Bink 2 SDK as an optional, separately packaged backend.

Pros:

- likely the most straightforward technical path
- best chance of parity with the source asset format

Cons:

- proprietary dependency
- licensing and redistribution burden
- poor fit for the clean-room baseline

This path should stay outside `cnc-formats` core scope.

### Path C — Optional Clean-Room Decoder

If Iron Curtain chooses the clean-room route, the implementation path should
be staged:

1. create an isolated experimental backend crate
2. define an engine-facing trait around:
   - open
   - metadata
   - next frame
   - next audio chunk
   - rewind/restart
3. validate the backend locally against owner-provided installs
4. only after policy acceptance, expose it through `ic-cnc-content`
5. only consider folding it closer to shared parser infrastructure if the legal
   posture, API quality, and maintenance burden are all clearly acceptable

## Proposed Downstream Integration Shape

If `.bk2` is eventually supported, Iron Curtain should consume it like this:

- `ic-game` / content lab requests movie playback through a backend trait
- `ic-cnc-content` owns backend selection and capability reporting
- the backend is optional and feature-gated
- the Bevy runtime treats it like any other streamed movie source:
  - audio-master-clock playback
  - one persistent presentation surface
  - bounded frame/audio queues

That keeps the UI/runtime shape stable even if the backend differs from the
classic `.vqa` path.

## Current Local Conclusion

Until a design/policy decision is accepted, local work should assume:

- Remastered support is a goal
- `.bk2` support is **desirable but optional**
- `.bk2` is **blocked on explicit policy approval**
- ordinary Remastered support work should continue without waiting for `.bk2`

## Follow-Up Needed

1. Open the corresponding design-doc issue/PR in the canonical design repo.
2. Decide whether a new policy blocker ID is needed for proprietary optional
   media backends.
3. If approved, choose one path:
   - defer
   - official SDK backend
   - clean-room backend spike

