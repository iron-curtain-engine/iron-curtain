# DG002 — Media Container and Localization Packaging Strategy

> Status: `proposal-only`
> Date: `2026-03-20`

## Execution Overlay

- `Milestone:` `M1`
- `Execution Step:` `G1` / `G2`
- `Priority:` `P-Core`
- `Dependencies:` `D049`, `D068`, `D075`, `src/architecture/ui-theme.md`
- `Evidence planned:` local design note, design-doc follow-up, import/runtime prototype against legal local assets, CI-safe synthetic tests only

## Summary

Iron Curtain should **not** design its own low-level audio/video container.
It should also **not** rely on media containers alone to express campaign and
localization behavior.

Recommended boundary:

1. use existing containers/codecs as **payload formats**
2. keep Iron Curtain's own **manifest/package/index layer** responsible for:
   - logical media identity
   - variant groups
   - language capability metadata
   - subtitle / closed-caption / dub availability
   - translation trust labels
   - install profile compatibility
   - deterministic fallback policy
3. treat rich media containers such as Matroska as optional interchange/import
   formats, not the canonical packaging model for IC content selection
4. keep the runtime video target narrow and predictable:
   - classic RA content remains in native classic formats
   - Remastered `.bk2` follows the existing `BK2 -> WebM` import policy from
     `D075`
   - authored/open modern media should prefer a constrained runtime-friendly
     target instead of an unrestricted "anything FFmpeg can open" baseline

## Why This Is A Design Gap

The current design docs already settle several important requirements:

- `D068` requires language capability metadata and explicit fallback semantics
  for cutscenes, voice, subtitles, and closed captions
- `ui-theme.md` requires shaping, BiDi, RTL/LTR handling, and fallback-font
  correctness for subtitles and other localized text
- `D075` already states that Remastered `.bk2` should be converted to WebM at
  import time instead of decoded directly at runtime

However, the docs do **not** yet clearly state:

- whether IC should adopt an existing container such as Matroska as its
  canonical cutscene packaging layer
- whether subtitle/audio fallback should live inside container tracks or in IC
  package metadata
- whether voice packs, cutscene packs, and subtitle packs should be modeled as
  one bundled movie file or as composable installable resources
- whether new authored media should target Matroska, WebM, Ogg, or a custom IC
  container

That leaves room for accidental design drift.

## Current Requirements From Canonical Docs

### D068 Requires Package-Level Capability Reasoning

`D068` defines the required reasoning model in terms of **installed media
packages** and their capabilities, not in terms of one self-contained movie
file.

At minimum, IC must reason about:

- available cutscene audio/dub languages
- available subtitle languages
- available closed-caption languages
- translation source / trust labeling
- coverage / completeness

It also defines the fallback chain across:

1. preferred dub + preferred subtitle/CC language
2. original audio + preferred subtitle/CC language
3. original audio + secondary subtitle/CC language
4. optional machine-translated subtitle/CC fallback
5. briefing/intermission/text fallback
6. skip cutscene without blocking progression

This is a **package-management and selection problem**, not merely a container
playback problem.

### UI Theme Contract Makes Subtitle Rendering An Engine Responsibility

`ui-theme.md` requires:

- shaping + BiDi support
- RTL/LTR layout behavior
- locale-aware font fallback
- correct rendering for subtitles and closed captions

This means no container can fully solve IC's subtitle problem by itself. Even
if a media file embeds subtitle tracks, the engine still owns:

- text shaping
- BiDi resolution
- font fallback
- truncation/wrap/clipping QA
- directionality overrides

### D075 Already Chooses Import-Time Normalization For BK2

`D075` already points to a normalized runtime strategy:

- Remastered `.bk2` is proprietary
- IC should not ship a runtime BK2 decoder in the baseline
- Asset Studio converts `BK2 -> WebM` at import time

That is a strong sign that IC's long-term design should favor:

- import-time normalization
- predictable runtime playback surfaces
- package metadata above the raw media files

instead of broad, ad-hoc runtime codec sprawl.

## What Existing Formats Teach Us

## Matroska / MKV

Matroska is the strongest reference format for rich media packaging.

Useful lessons:

- multiple audio and subtitle tracks
- language tags
- default / forced track flags
- hearing-impaired and commentary flags
- chapters
- attachments, including fonts
- cues/indexing for seeking

These line up well with IC requirements for:

- alternative audio
- subtitle/CC availability
- cutscene chapters/navigation
- richer localization metadata

But Matroska is still only a **container**. It does not express IC-specific:

- install profiles
- variant groups such as `Original / Clean / AI-Enhanced`
- translation trust labels such as `human / machine / hybrid`
- gameplay-vs-presentation fingerprint scope
- non-blocking campaign fallback policy

Therefore, Matroska is an excellent **interchange/import format**, but not a
sufficient canonical package model for IC by itself.

Sources:

- `RFC 9559` — tracks, cues, chapters, attachments, language codes, default
  and forced track selection:
  - `https://datatracker.ietf.org/doc/rfc9559/`

## WebM

WebM is a constrained subset of Matroska built around predictable codec
support.

Useful lessons:

- narrower runtime target improves interoperability
- container structure can stay Matroska-like while the supported codec matrix
  stays small
- WebVTT text tracks are a straightforward subtitle path when the playback
  stack wants simplicity

This matches IC's existing `D075` strategy well:

- authored or normalized runtime cutscenes can target a constrained playback
  pipeline
- the engine does not need to promise "anything Matroska can carry"

Source:

- WebM FAQ:
  - `https://www.webmproject.org/about/faq/`

## Ogg / Ogg Skeleton / Opus

Ogg and Opus are more relevant to audio packs than to cutscene packaging.

Useful lessons:

- Ogg is a generic multiplexing container
- Ogg Skeleton adds track metadata, language selection hints, time mapping,
  preroll, and seeking aids
- Opus is designed for low-latency interactive audio and bounded buffering

This makes Ogg/Opus attractive for:

- voice packs
- music packs
- speech/dialogue packs
- network voice and low-latency audio playback principles

But for rich cutscene packages with multiple subtitles, chapters, fonts,
variant metadata, and campaign-specific fallback, Matroska/WebM is the better
container family.

Sources:

- Ogg Skeleton:
  - `https://xiph.org/ogg/doc/skeleton.html`
- Ogg Opus:
  - `https://www.rfc-editor.org/rfc/rfc7845.html`
- Opus codec:
  - `https://datatracker.ietf.org/doc/rfc6716/`

## FFmpeg

FFmpeg is not the answer to "which container should IC standardize on".
It is valuable as:

- an import/probe/transcode toolkit
- a model for demux/decode separation
- a model for stream/timestamp/seek-aware architecture

IC should learn from FFmpeg's media pipeline design, but should not define its
content model as "whatever FFmpeg can open".

Source:

- libavformat demuxing/stream model:
  - `https://ffmpeg.org/doxygen/trunk/group__lavf__decoding.html`

## Recommendation

### Decision

IC should adopt a **two-layer media model**.

### Layer 1 — IC Package / Manifest / Index Layer

This is the canonical authority for:

- logical cutscene or voice resource ID
- variant grouping
- installed-pack availability
- language capability matrix
- translation source / trust labels
- coverage / completeness
- campaign fallback behavior
- presentation fingerprint classification

This layer should remain IC-defined because it expresses IC product behavior,
not just byte layout.

### Layer 2 — Payload Media Files

These are the underlying media assets the engine imports or plays.

Recommended usage:

- classic RA assets:
  - keep native `.vqa`, `.aud`, `.wsa`, `.shp`, etc.
- Remastered video:
  - keep `BK2 -> WebM` import normalization from `D075`
- new/authored cutscene interchange:
  - allow Matroska/WebM import where useful
- runtime normalized cutscene playback:
  - prefer a narrow, predictable format target over unrestricted container
    freedom
- music / voice packs:
  - allow Ogg/Opus as a strong open audio payload choice

## What IC Should Not Do

### Do Not Design A New General-Purpose AV Container

That would duplicate solved industry problems:

- multiplexing
- timestamps
- track selection
- subtitle carriage
- chaptering
- attachments
- indexing / seeking

and would still fail to replace the IC-specific manifest layer required by
`D068`.

### Do Not Treat The Container As The Whole Product Model

If IC stores all selection/fallback semantics only inside a movie file, it
would make the following harder or ambiguous:

- separate subtitle packs
- optional localized dubs
- partial-coverage labels
- machine-translation trust labels
- campaign media fallbacks when packs are missing
- install-profile UX
- Workshop dependency reasoning

Those belong above the container.

## Preferred Near-Term Direction

1. Keep `D075` as the Remastered video policy:
   - import-time `BK2 -> WebM`
2. Explicitly define an IC media package manifest/index contract for:
   - audio languages
   - subtitle languages
   - CC languages
   - trust labels
   - coverage
   - variant groups
   - fallback behavior
3. Treat Matroska/WebM as import/runtime payload formats, not the canonical
   policy layer
4. Treat Ogg/Opus as the preferred open audio payload family for new audio
   packs where suitable
5. Keep subtitle rendering and RTL/LTR correctness owned by the engine UI/text
   pipeline, not delegated to the container

## Acceptance Criteria

This gap should be considered resolved only when:

1. the design docs explicitly state that IC uses:
   - standard payload containers/codecs
   - plus IC-owned manifest/package metadata above them
2. the docs explicitly reject creating a custom general-purpose AV container
3. the docs clarify where the canonical language capability matrix lives
4. the docs clarify which runtime video target is expected for imported
   Remastered and authored cutscenes
5. the docs confirm that subtitle/CC rendering policy remains an engine/UI
   concern even when tracks are embedded in a container

## Proposed Follow-Up In Design Docs

Suggested places to clarify this canonically:

- `D068-selective-install.md`
- `D075-remastered-format-compat.md`
- `05-FORMATS.md`
- `04-MODDING.md` or the resource-pack docs
- possibly `17-PLAYER-FLOW.md` / settings docs where subtitle/audio selection
  UX is described

