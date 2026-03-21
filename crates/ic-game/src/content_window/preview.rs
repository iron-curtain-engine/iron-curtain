// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing preview runtime for the content lab.
//!
//! `preview_decode` owns the pure "turn bytes into preview surfaces" half of
//! the lab. This module owns the engine/runtime half:
//! - create temporary Bevy `Image` assets and one custom PCM audio asset from
//!   decoded data
//! - keep animation playback, audio playback, and keyboard transport controls
//!   in sync with the selected catalog entry
//!
//! That split matters because the decode rules should stay testable without a
//! window, while the Bevy layer should stay small and focused on presentation.

use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::sync::Arc;
use std::thread;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::widget::ImageNode;
use bevy::window::PrimaryWindow;
use bevy::reflect::TypePath;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

#[cfg(target_os = "windows")]
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings};

use super::catalog::{ContentCatalog, ContentCatalogEntry, ContentFamily};
#[cfg(target_os = "windows")]
use super::preview_audio::PcmAudioSource;
use super::state::ContentLabState;

#[cfg(test)]
pub(crate) use super::preview_decode::preview_capabilities_for_entry;
#[cfg(test)]
pub(crate) use super::preview_decode::resolve_palette_entry_for_visual as resolve_palette_entry_for_sprite;
pub(crate) use super::preview_decode::{
    load_preview_for_entry, PreparedContentPreview, PreviewLoadError,
};
use super::preview_decode::build_first_frame_preview;

/// Presentation policy for the world-space fallback surface.
///
/// Static art and diagnostics benefit from leaving visible space for the
/// gallery chrome around them. Videos are different: when a player chooses a
/// cutscene, the expected experience is closer to a movie viewer. This policy
/// keeps both cases aspect-correct while allowing VQA previews to claim most
/// of the fullscreen window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PreviewSurfacePolicy {
    pub(crate) max_width_fraction: f32,
    pub(crate) max_height_fraction: f32,
}

/// Returns the contain-fit sizing budget for one selected content family,
/// using values from the TOML playback config.
pub(crate) fn preview_surface_policy_for_family(
    family: Option<ContentFamily>,
    playback: &crate::config::PlaybackConfig,
) -> PreviewSurfacePolicy {
    match family {
        Some(ContentFamily::Video) => PreviewSurfacePolicy {
            max_width_fraction: playback.movie_max_width,
            max_height_fraction: playback.movie_max_height,
        },
        _ => PreviewSurfacePolicy {
            max_width_fraction: playback.preview_max_width,
            max_height_fraction: playback.preview_max_height,
        },
    }
}

/// Returns `true` when preview preparation should happen off the main Bevy
/// thread.
///
/// With the current `cnc-formats` API, VQA preview preparation is whole-file:
/// read bytes, decode every frame, extract audio, and convert frames to RGBA.
/// Moving that work onto a background thread is the best local improvement we
/// can make in this repo without first extending the lower-level parser crate
/// to expose a true frame-by-frame streaming API.
pub(crate) fn should_background_load_preview_for_family(family: ContentFamily) -> bool {
    matches!(family, ContentFamily::Video)
}

/// Marker for the visible world-space fallback surface that mirrors the
/// currently selected preview.
///
/// The Bevy UI overlay is still under active development. This sprite-based
/// fallback makes the selected SHP/WSA/VQA frame appear on the one render path
/// we already know is visible on every machine running `ic-game`: the main 2D
/// world camera. That guarantees that when audio starts for a video resource,
/// the corresponding frame is also on screen.
#[derive(Component)]
pub(crate) struct ContentPreviewBillboard;

/// Marker for the world-space loading text that indicates background work.
#[derive(Component)]
pub(crate) struct ContentPreviewLoadingText;

/// Background preview-preparation handle for heavy selected resources.
///
/// The current VQA path in `preview_decode` performs full file read and full
/// decode before a preview becomes usable. Running that on the Bevy update
/// thread makes the window feel frozen. This task resource mirrors the
/// background source-scan pattern: worker thread does the CPU-heavy prep, the
/// main thread later turns the prepared preview into Bevy assets.
#[derive(Resource)]
pub(crate) struct ContentPreviewLoadTask {
    receiver: Mutex<Receiver<PreviewLoadMessage>>,
}

/// Messages sent from the background preview-preparation thread.
///
/// For video resources, the thread sends a lightweight first-frame preview as
/// soon as the initial frame is decoded, followed by the full result once all
/// frames and audio have been prepared.  Non-video resources send only the
/// `Full` variant.
/// How frame advancement is synchronized with audio.
///
/// Encoding the valid modes as distinct variants prevents the class of
/// bugs where audio-position sync restarts a video that was already playing
/// via timer (the exact regression we saw with streaming VQA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaybackSyncMode {
    /// Frame advancement is driven by a local wall-clock timer.
    /// Used for animations without audio, and for streaming VQA where
    /// audio arrives after playback has already started.
    Timer,
    /// Frame advancement tracks the audio sink position so video and
    /// audio stay aligned.  Only valid when an audio sink is active.
    AudioSync,
}

/// Audio metadata carried alongside a ready preview.
///
/// This exists as a separate struct so it can only appear inside
/// `PreviewPhase::Ready`, enforcing the invariant that audio is never
/// "available" before the preview is fully loaded.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AudioInfo {
    pub(crate) duration_seconds: f32,
}

/// The lifecycle phase of the content preview.
///
/// Each variant carries exactly the data valid for that phase.  Fields
/// that are meaningless in a given state simply do not exist on that
/// variant, making invalid combinations unrepresentable at compile time.
///
/// ```text
///   Empty ──▶ Loading ──▶ StreamingFirstFrame ──▶ Ready
///                    │                                ▲
///                    └──── (non-video Full) ──────────┘
/// ```
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreviewPhase {
    /// No content loaded.
    Empty,

    /// A background decode is in progress.  The content family is known
    /// (needed for surface-sizing policy) but no visual or audio data
    /// exists yet.
    Loading {
        family: ContentFamily,
    },

    /// The first decoded frame is on screen and timer-based playback is
    /// running while the background thread continues decoding.
    ///
    /// Invariants enforced by construction:
    /// - `sync_mode` is always `Timer` (audio has not arrived yet)
    /// - `playback_requested` starts `true` (auto-play on first frame)
    StreamingFirstFrame {
        family: ContentFamily,
        visual_session: VisualPreviewSession,
        frame_timer_seconds: f32,
        playback_requested: bool,
    },

    /// All frames have been decoded, optionally with audio and text.
    Ready {
        family: ContentFamily,
        visual_session: VisualPreviewSession,
        frame_timer_seconds: f32,
        playback_requested: bool,
        sync_mode: PlaybackSyncMode,
        audio_info: Option<AudioInfo>,
        text_available: bool,
    },
}

impl Default for PreviewPhase {
    fn default() -> Self {
        Self::Empty
    }
}

#[derive(Debug)]
enum PreviewLoadMessage {
    /// A single decoded first frame, sent immediately so the content lab can
    /// show a visible image while the rest of the video is still decoding.
    FirstFrame {
        selection: (usize, usize),
        #[allow(dead_code, reason = "carried for diagnostic logging; not consumed by the receiver yet")]
        entry: ContentCatalogEntry,
        preview: Result<Option<PreparedContentPreview>, PreviewLoadError>,
    },
    /// The complete decode result with all frames and audio.
    Full {
        selection: (usize, usize),
        entry: ContentCatalogEntry,
        preview: Result<Option<PreparedContentPreview>, PreviewLoadError>,
    },
    /// A batch of incrementally streamed VQA frames and/or audio.
    VqaStream {
        selection: (usize, usize),
        batch: super::vqa_stream::VqaStreamBatch,
    },
}

/// CPU-side visual playback state for the selected preview resource.
///
/// The current content-lab decode path is still eager for some media types,
/// especially VQA. This session type is the local runtime improvement that
/// keeps that eagerness from leaking all the way into Bevy: we cache decoded
/// frames in ordinary Rust memory, but present them through one stable display
/// surface that the UI and world-space fallback can both reference.
///
/// That mirrors the shape we ultimately want once `cnc-formats` exposes true
/// incremental media decode: a playback session owns timing and frame
/// selection, while the rendering layer owns a single mutable presentation
/// target rather than a separate GPU asset per frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VisualPreviewSession {
    frames: Vec<ic_render::sprite::RgbaSpriteFrame>,
    current_frame: usize,
    frame_duration_seconds: Option<f32>,
}

impl VisualPreviewSession {
    /// Creates a playback session from decoded RGBA frames.
    ///
    /// The session rejects empty input because the runtime always needs one
    /// concrete frame to seed the persistent Bevy image surface.
    pub(crate) fn new(
        frames: Vec<ic_render::sprite::RgbaSpriteFrame>,
        frame_duration_seconds: Option<f32>,
    ) -> Option<Self> {
        if frames.is_empty() {
            return None;
        }

        Some(Self {
            frames,
            current_frame: 0,
            frame_duration_seconds,
        })
    }

    /// Number of decoded frames currently cached in the session.
    pub(crate) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Number of display surfaces the runtime needs for this session.
    ///
    /// Today that is intentionally fixed at one: the selected preview updates a
    /// single Bevy texture in place instead of constructing one GPU image per
    /// decoded frame.
    pub(crate) fn display_surface_count(&self) -> usize {
        1
    }

    /// Zero-based index of the currently selected frame.
    pub(crate) fn current_frame_index(&self) -> usize {
        self.current_frame
    }

    /// Current RGBA frame that should be visible on screen.
    pub(crate) fn current_frame(&self) -> &ic_render::sprite::RgbaSpriteFrame {
        &self.frames[self.current_frame]
    }

    /// Shared frame cadence for animation playback, if this session is not
    /// static.
    pub(crate) fn frame_duration_seconds(&self) -> Option<f32> {
        self.frame_duration_seconds
    }

    /// Selects the active frame without rebuilding the cached session.
    pub(crate) fn select_frame(&mut self, frame_index: usize) {
        if self.frames.is_empty() {
            self.current_frame = 0;
        } else {
            self.current_frame = frame_index % self.frames.len();
        }
    }

    /// Appends additional frames to the session (for incremental streaming).
    ///
    /// The current frame index is preserved so playback continues smoothly
    /// from where it was rather than jumping forward or wrapping.
    pub(crate) fn push_frames(&mut self, new_frames: Vec<ic_render::sprite::RgbaSpriteFrame>) {
        self.frames.extend(new_frames);
    }

    /// Dimensions of the currently active frame.
    pub(crate) fn current_frame_dimensions(&self) -> (u32, u32) {
        let frame = self.current_frame();
        (frame.width(), frame.height())
    }

    /// Human-readable runtime summary for diagnostics and tests.
    pub(crate) fn runtime_summary(&self) -> String {
        format!(
            "visual backend: eager decoded session | cached frames: {} | display surface: {}",
            self.frame_count(),
            self.display_surface_count()
        )
    }
}

/// GPU-composited CRT scanlines overlay — one semi-transparent black child
/// `MaterialNode` rendered at screen resolution over the video `ImageNode`.
///
/// Mirrors the OpenRA `VideoPlayerWidget.DrawOverlay` approach: alternating
/// transparent and 50%-black rows spaced at half a VQA pixel-row height in
/// physical screen pixels, so the lines stay thin regardless of upscale factor.
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub(crate) struct ScanlinesMaterial {
    /// x = half_row_height in physical screen pixels; y/z/w unused.
    #[uniform(0)]
    pub params: Vec4,
}

impl UiMaterial for ScanlinesMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/scanlines.wgsl".into()
    }
}

/// Marker for the scanlines overlay child node.
#[derive(Component)]
struct ContentScanlineOverlay;

/// Tracks the currently active visual/audio preview surfaces.
///
/// The Bevy-specific asset handles live on the outer struct because they
/// must be cleaned up regardless of lifecycle phase.  All state-dependent
/// fields live inside `phase: PreviewPhase`, which enforces that only data
/// valid for the current phase is accessible.
#[derive(Resource, Debug, Default)]
pub(crate) struct ContentPreviewTracker {
    // --- Bevy handles (state-independent) ---
    current_selection: Option<(usize, usize)>,
    pub(crate) selected_image_entity: Option<Entity>,
    display_image_handle: Option<Handle<Image>>,
    /// The spawned fullscreen scanlines overlay entity.
    scanlines_overlay_entity: Option<Entity>,
    /// Material handle for the scanlines overlay (kept for uniform updates).
    scanlines_overlay_material: Option<Handle<ScanlinesMaterial>>,
    #[cfg(target_os = "windows")]
    audio_handle: Option<Handle<PcmAudioSource>>,
    #[cfg(target_os = "windows")]
    audio_entity: Option<Entity>,

    // --- State machine ---
    phase: PreviewPhase,

    // --- Playlist loop detection ---
    /// Wall-clock seconds elapsed since the current entry started playing.
    /// Reset to 0 on every selection change.
    loop_elapsed_seconds: f32,
    /// Set to `true` when one full playback cycle has completed.
    /// Consumed by `handle_playlist_advance` to advance to the next entry.
    pub(crate) playlist_advance_pending: bool,
    /// Duration of audio-only content (e.g. AUD files) that never reaches
    /// `PreviewPhase::Ready` because they have no visual session.  Used by
    /// the loop-detection path so the playlist still advances for them.
    audio_only_loop_duration: Option<f32>,
}

impl ContentPreviewTracker {
    // ── Asset cleanup ────────────────────────────────────────────────

    #[cfg(target_os = "windows")]
    fn clear_dynamic_assets(
        &mut self,
        images: &mut Assets<Image>,
        audio_sources: &mut Assets<PcmAudioSource>,
    ) {
        if let Some(handle) = self.display_image_handle.take() {
            images.remove(handle.id());
        }
        if let Some(handle) = self.audio_handle.take() {
            audio_sources.remove(handle.id());
        }
        self.audio_entity = None;
        // scanlines_overlay_entity and _material are session-persistent —
        // the fullscreen overlay is reused across entries, not per-entry.
        self.selected_image_entity = None;
        self.phase = PreviewPhase::Empty;
    }

    #[cfg(not(target_os = "windows"))]
    fn clear_dynamic_assets(&mut self, images: &mut Assets<Image>) {
        if let Some(handle) = self.display_image_handle.take() {
            images.remove(handle.id());
        }
        // scanlines_overlay_entity and _material are session-persistent.
        self.selected_image_entity = None;
        self.phase = PreviewPhase::Empty;
    }

    // ── Phase transitions ────────────────────────────────────────────

    /// Transition: any → Loading.
    fn begin_loading(&mut self, family: ContentFamily) {
        self.phase = PreviewPhase::Loading { family };
    }

    /// Transition: Loading → StreamingFirstFrame.
    fn begin_streaming(&mut self, visual_session: VisualPreviewSession) {
        let family = self.phase_family()
            .expect("begin_streaming requires a phase with a known family");
        self.phase = PreviewPhase::StreamingFirstFrame {
            family,
            visual_session,
            frame_timer_seconds: 0.0,
            playback_requested: true,
        };
    }

    /// Append frames to the streaming session.
    ///
    /// Only valid in `StreamingFirstFrame`.  The type system guarantees
    /// that `push_frames` on `VisualPreviewSession` is unreachable from
    /// `Ready` — only `finalize_streaming` can produce the transition.
    fn push_streaming_frames(
        &mut self,
        frames: Vec<ic_render::sprite::RgbaSpriteFrame>,
    ) {
        match &mut self.phase {
            PreviewPhase::StreamingFirstFrame { visual_session, .. } => {
                visual_session.push_frames(frames);
            }
            _ => {}
        }
    }

    /// Transition: StreamingFirstFrame → Ready.
    ///
    /// Preserves the current frame position, timer, and Timer sync mode so
    /// playback continues without a visible skip.  The caller is responsible
    /// for aligning the audio start position with the video's current frame
    /// before spawning the audio entity (see `rotate_audio_to_video_position`
    /// in `poll_content_preview_load`).
    fn finalize_streaming(
        &mut self,
        audio_info: Option<AudioInfo>,
    ) {
        let old = std::mem::take(&mut self.phase);
        match old {
            PreviewPhase::StreamingFirstFrame {
                family,
                visual_session,
                frame_timer_seconds,
                playback_requested,
            } => {
                self.phase = PreviewPhase::Ready {
                    family,
                    visual_session,
                    frame_timer_seconds,
                    playback_requested,
                    sync_mode: PlaybackSyncMode::Timer,
                    audio_info,
                    text_available: false,
                };
            }
            other => {
                // Not in the expected state — restore and ignore.
                self.phase = other;
            }
        }
    }

    /// Transition: any → Ready (non-streaming immediate decode).
    fn apply_ready(
        &mut self,
        family: ContentFamily,
        visual_session: VisualPreviewSession,
        playback_requested: bool,
        sync_mode: PlaybackSyncMode,
        audio_info: Option<AudioInfo>,
        text_available: bool,
    ) {
        self.phase = PreviewPhase::Ready {
            family,
            visual_session,
            frame_timer_seconds: 0.0,
            playback_requested,
            sync_mode,
            audio_info,
            text_available,
        };
    }

    /// Transition: StreamingFirstFrame → Ready via full-decode upgrade.
    fn upgrade_to_ready(
        &mut self,
        visual_session: VisualPreviewSession,
        audio_info: Option<AudioInfo>,
        text_available: bool,
    ) {
        let family = self.phase_family()
            .expect("upgrade_to_ready requires a phase with a known family");
        let sync_mode = if audio_info.is_some() {
            PlaybackSyncMode::AudioSync
        } else {
            PlaybackSyncMode::Timer
        };
        self.phase = PreviewPhase::Ready {
            family,
            visual_session,
            frame_timer_seconds: 0.0,
            playback_requested: true,
            sync_mode,
            audio_info,
            text_available,
        };
    }

    // ── Phase queries ────────────────────────────────────────────────

    fn phase_family(&self) -> Option<ContentFamily> {
        match &self.phase {
            PreviewPhase::Empty => None,
            PreviewPhase::Loading { family, .. }
            | PreviewPhase::StreamingFirstFrame { family, .. }
            | PreviewPhase::Ready { family, .. } => Some(*family),
        }
    }

    fn is_loading(&self) -> bool {
        matches!(
            self.phase,
            PreviewPhase::Loading { .. } | PreviewPhase::StreamingFirstFrame { .. }
        )
    }

    fn visual_session(&self) -> Option<&VisualPreviewSession> {
        match &self.phase {
            PreviewPhase::StreamingFirstFrame { visual_session, .. }
            | PreviewPhase::Ready { visual_session, .. } => Some(visual_session),
            _ => None,
        }
    }

    fn visual_session_mut(&mut self) -> Option<&mut VisualPreviewSession> {
        match &mut self.phase {
            PreviewPhase::StreamingFirstFrame { visual_session, .. }
            | PreviewPhase::Ready { visual_session, .. } => Some(visual_session),
            _ => None,
        }
    }

    fn has_visual(&self) -> bool {
        self.visual_session().is_some() && self.display_image_handle.is_some()
    }

    fn has_animation(&self) -> bool {
        self.visual_session().is_some_and(|session| {
            session.frame_count() > 1 && session.frame_duration_seconds().is_some()
        })
    }

    fn has_audio(&self) -> bool {
        matches!(self.phase, PreviewPhase::Ready { audio_info: Some(_), .. })
    }

    fn has_text(&self) -> bool {
        matches!(self.phase, PreviewPhase::Ready { text_available: true, .. })
    }

    fn has_transport(&self) -> bool {
        self.has_animation() || self.has_audio()
    }

    fn playback_requested(&self) -> bool {
        match &self.phase {
            PreviewPhase::StreamingFirstFrame { playback_requested, .. }
            | PreviewPhase::Ready { playback_requested, .. } => *playback_requested,
            _ => false,
        }
    }

    fn set_playback_requested(&mut self, value: bool) {
        match &mut self.phase {
            PreviewPhase::StreamingFirstFrame { playback_requested, .. }
            | PreviewPhase::Ready { playback_requested, .. } => {
                *playback_requested = value;
            }
            _ => {}
        }
    }

    fn toggle_playback(&mut self) {
        let current = self.playback_requested();
        self.set_playback_requested(!current);
    }

    fn sync_mode(&self) -> PlaybackSyncMode {
        match &self.phase {
            PreviewPhase::StreamingFirstFrame { .. } => PlaybackSyncMode::Timer,
            PreviewPhase::Ready { sync_mode, .. } => *sync_mode,
            _ => PlaybackSyncMode::Timer,
        }
    }

    fn frame_timer_seconds_mut(&mut self) -> Option<&mut f32> {
        match &mut self.phase {
            PreviewPhase::StreamingFirstFrame { frame_timer_seconds, .. }
            | PreviewPhase::Ready { frame_timer_seconds, .. } => Some(frame_timer_seconds),
            _ => None,
        }
    }

    fn audio_info(&self) -> Option<&AudioInfo> {
        match &self.phase {
            PreviewPhase::Ready { audio_info, .. } => audio_info.as_ref(),
            _ => None,
        }
    }

    fn frame_count(&self) -> usize {
        self.visual_session()
            .map_or(0, VisualPreviewSession::frame_count)
    }

    fn current_frame_index(&self) -> usize {
        self.visual_session()
            .map_or(0, VisualPreviewSession::current_frame_index)
    }

    pub(crate) fn current_image_handle(&self) -> Option<Handle<Image>> {
        self.display_image_handle.clone()
    }

    fn current_visual_frame(&self) -> Option<&ic_render::sprite::RgbaSpriteFrame> {
        self.visual_session()
            .map(VisualPreviewSession::current_frame)
    }

    pub(crate) fn frame_dimensions(&self) -> Option<(u32, u32)> {
        self.visual_session()
            .map(VisualPreviewSession::current_frame_dimensions)
    }

    fn current_family(&self) -> Option<ContentFamily> {
        self.phase_family()
    }

    fn frame_duration_seconds(&self) -> Option<f32> {
        self.visual_session()
            .and_then(VisualPreviewSession::frame_duration_seconds)
    }

    fn select_frame(&mut self, frame_index: usize) {
        if let Some(session) = self.visual_session_mut() {
            session.select_frame(frame_index);
        }
    }

    fn is_ready(&self) -> bool {
        matches!(self.phase, PreviewPhase::Ready { .. })
    }

    /// Total duration of one playback cycle: frame count × frame duration for
    /// visual content, audio duration for audio-only content.
    pub(crate) fn loop_duration_seconds(&self) -> Option<f32> {
        if let Some(fd) = self.frame_duration_seconds() {
            let fc = self.frame_count();
            if fc > 0 {
                return Some(fd * fc as f32);
            }
        }
        if let Some(ai) = self.audio_info() {
            return Some(ai.duration_seconds);
        }
        self.audio_only_loop_duration
    }

    /// Resets loop-detection state when a new entry is selected.
    pub(crate) fn reset_loop_state(&mut self) {
        self.loop_elapsed_seconds = 0.0;
        self.playlist_advance_pending = false;
        self.audio_only_loop_duration = None;
    }

    // ── Runtime diagnostics ──────────────────────────────────────────

    #[cfg(target_os = "windows")]
    fn runtime_status(&self, audio_sink: Option<&AudioSink>) -> String {
        if self.is_loading() && !self.has_visual() {
            return "preview: loading in background\naudio: waiting for decoded preview\ntransport: unavailable during preparation".into();
        }

        let mut lines = Vec::new();

        if self.has_visual() {
            if let Some((width, height)) = self.frame_dimensions() {
                if self.has_animation() {
                    let state = if self.playback_requested() { "playing" } else { "paused" };
                    lines.push(format!(
                        "visual: {state} | frame {}/{} | {}x{}",
                        self.current_frame_index() + 1,
                        self.frame_count(),
                        width,
                        height
                    ));
                } else {
                    lines.push(format!("visual: static | 1 frame | {}x{}", width, height));
                }
            }
            if let Some(session) = self.visual_session() {
                lines.push(session.runtime_summary());
            }
        } else {
            lines.push("visual: none".into());
        }

        if self.has_audio() {
            match audio_sink {
                Some(sink) => {
                    let state = if sink.is_paused() { "paused" } else { "playing" };
                    let total = self.audio_info().map_or(0.0, |a| a.duration_seconds);
                    lines.push(format!(
                        "audio: {state} | {:.2}s / {:.2}s",
                        sink.position().as_secs_f32(),
                        total
                    ));
                }
                None => lines.push("audio: preparing Bevy sink".into()),
            }
            lines.push("audio backend: fully decoded PCM preview buffer".into());
        } else {
            lines.push("audio: none".into());
        }

        lines.push(format!(
            "text excerpt: {}",
            if self.has_text() { "available" } else { "none" }
        ));

        if self.has_transport() {
            lines.push(format!(
                "transport: {}",
                if self.playback_requested() { "running" } else { "paused" }
            ));
        } else {
            lines.push("transport: not applicable".into());
        }

        lines.join("\n")
    }

    #[cfg(not(target_os = "windows"))]
    fn runtime_status(&self) -> String {
        if self.is_loading() && !self.has_visual() {
            return "preview: loading in background\naudio: waiting for decoded preview\ntransport: unavailable during preparation".into();
        }

        let mut lines = Vec::new();

        if self.has_visual() {
            if let Some((width, height)) = self.frame_dimensions() {
                if self.has_animation() {
                    let state = if self.playback_requested() { "playing" } else { "paused" };
                    lines.push(format!(
                        "visual: {state} | frame {}/{} | {}x{}",
                        self.current_frame_index() + 1,
                        self.frame_count(),
                        width,
                        height
                    ));
                } else {
                    lines.push(format!("visual: static | 1 frame | {}x{}", width, height));
                }
            }
            if let Some(session) = self.visual_session() {
                lines.push(session.runtime_summary());
            }
        } else {
            lines.push("visual: none".into());
        }

        if self.has_audio() {
            lines.push(
                "audio: decoded PCM ready; Bevy playback is enabled on Windows builds".into(),
            );
            lines.push("audio backend: fully decoded PCM preview buffer".into());
        } else {
            lines.push("audio: none".into());
        }

        lines.push(format!(
            "text excerpt: {}",
            if self.has_text() { "available" } else { "none" }
        ));

        if self.has_transport() {
            lines.push(format!(
                "transport: {}",
                if self.playback_requested() { "running" } else { "paused" }
            ));
        } else {
            lines.push("transport: not applicable".into());
        }

        lines.join("\n")
    }
}

// ── Test-only accessors ──────────────────────────────────────────────
//
// The tracker's phase-transition and query methods are intentionally
// private so production code must go through the well-defined system
// functions.  Tests need direct access to drive the state machine and
// assert on internal state.

#[cfg(test)]
impl ContentPreviewTracker {
    pub(crate) fn test_begin_loading(&mut self, family: ContentFamily) {
        self.begin_loading(family);
    }

    pub(crate) fn test_begin_streaming(&mut self, session: VisualPreviewSession) {
        self.begin_streaming(session);
    }

    pub(crate) fn test_finalize_streaming(&mut self, audio_info: Option<AudioInfo>) {
        self.finalize_streaming(audio_info);
    }

    pub(crate) fn test_select_frame(&mut self, index: usize) {
        self.select_frame(index);
    }

    pub(crate) fn test_current_frame_index(&self) -> usize {
        self.current_frame_index()
    }

    pub(crate) fn test_sync_mode(&self) -> PlaybackSyncMode {
        self.sync_mode()
    }

    pub(crate) fn test_playback_requested(&self) -> bool {
        self.playback_requested()
    }

    /// Sets the scanlines overlay entity as if `sync_scanlines_overlay` had
    /// spawned it, without requiring a real Bevy world.
    pub(crate) fn test_set_scanlines_overlay_entity(&mut self, entity: bevy::prelude::Entity) {
        self.scanlines_overlay_entity = Some(entity);
    }

    /// Returns the current scanlines overlay entity handle.
    pub(crate) fn test_scanlines_overlay_entity(&self) -> Option<bevy::prelude::Entity> {
        self.scanlines_overlay_entity
    }

    /// Exposes `current_family` for assertion in tests.
    pub(crate) fn test_current_family(&self) -> Option<ContentFamily> {
        self.current_family()
    }

    /// Exercises the navigation-clear path (clearing per-entry state) using a
    /// dummy image asset store so the test does not need a full Bevy world.
    pub(crate) fn test_clear_for_navigation(&mut self) {
        let mut images = bevy::asset::Assets::<bevy::prelude::Image>::default();
        #[cfg(not(target_os = "windows"))]
        self.clear_dynamic_assets(&mut images);
        #[cfg(target_os = "windows")]
        {
            let mut audio =
                bevy::asset::Assets::<super::preview_audio::PcmAudioSource>::default();
            self.clear_dynamic_assets(&mut images, &mut audio);
        }
    }
}

/// Marker component for the one audio-preview entity.
#[derive(Component)]
pub(crate) struct ContentPreviewAudio;

/// Spawns the camera and one hidden world-space sprite used for the selected
/// preview surface.
///
/// `Camera2d` is the Bevy component that renders the ordinary 2D scene.
/// `IsDefaultUiCamera` explicitly tells Bevy to route UI to this camera as
/// well, which keeps the UI and world-sprite fallback on one known render
/// target.
pub(crate) fn setup_content_window_scene(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera));
    commands.spawn((
        ContentPreviewBillboard,
        Sprite::default(),
        Visibility::Hidden,
        Transform::default(),
    ));
    commands.spawn((
        ContentPreviewLoadingText,
        Text2d::new("Loading..."),
        TextFont {
            font_size: 24.0,
            ..default()
        },
        TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)),
        Visibility::Hidden,
        Transform::from_xyz(0.0, 0.0, 5.0),
    ));
}

/// Loads or clears the active preview when the selected catalog entry changes.
///
/// This is where the pure decode layer meets Bevy. The selected resource is
/// decoded once into a `PreparedContentPreview`, then turned into transient
/// engine assets the user can inspect and control from the content lab.
pub(crate) fn refresh_content_preview(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    #[cfg(target_os = "windows")] mut audio_sources: ResMut<Assets<PcmAudioSource>>,
    mut state: ResMut<ContentLabState>,
    mut tracker: ResMut<ContentPreviewTracker>,
    archive_cache: Res<super::ArchivePreloadCache>,
    #[cfg(target_os = "windows")]
    existing_audio_entities: Query<(Entity, Option<&AudioSink>), With<ContentPreviewAudio>>,
    #[cfg(not(target_os = "windows"))]
    existing_audio_entities: Query<Entity, With<ContentPreviewAudio>>,
) {
    let selection = state.selected_location();
    if tracker.current_selection == selection {
        return;
    }

    // Pause then despawn old audio entities.  `sink.pause()` takes effect
    // immediately, while `commands.entity().despawn()` is deferred until the
    // end of the frame.  Without the explicit pause, the old movie's audio
    // continues to play through the OS audio buffer for the remainder of the
    // frame — audible as a brief burst of stale audio when switching movies
    // quickly.
    #[cfg(target_os = "windows")]
    for (entity, sink) in &existing_audio_entities {
        if let Some(sink) = sink {
            sink.pause();
        }
        commands.entity(entity).despawn();
    }
    #[cfg(not(target_os = "windows"))]
    for entity in &existing_audio_entities {
        commands.entity(entity).despawn();
    }
    #[cfg(target_os = "windows")]
    tracker.clear_dynamic_assets(&mut images, &mut audio_sources);
    #[cfg(not(target_os = "windows"))]
    tracker.clear_dynamic_assets(&mut images);
    tracker.current_selection = selection;
    tracker.reset_loop_state();
    commands.remove_resource::<ContentPreviewLoadTask>();

    let Some((catalog_index, entry_index)) = selection else {
        state.set_preview_summary("No content entry is currently selected.");
        state.set_playback_summary("No active preview runtime.");
        return;
    };
    let Some(entry) = state
        .catalogs()
        .get(catalog_index)
        .and_then(|catalog| catalog.entries.get(entry_index))
        .cloned()
    else {
        state.set_preview_summary("The current selection points outside the visible catalog.");
        state.set_playback_summary("No active preview runtime.");
        return;
    };
    if should_background_load_preview_for_family(entry.family) {
        tracker.begin_loading(entry.family);
        state.set_preview_summary(format!(
            "Loading preview for {}...\nFirst frame will appear momentarily while remaining frames decode in the background.",
            entry.relative_path
        ));
        state.set_playback_summary(
            "Preview runtime will start when the first frame is ready.",
        );
        commands.insert_resource(start_content_preview_load(
            (catalog_index, entry_index),
            entry,
            state.catalogs().to_vec(),
            archive_cache.clone(),
        ));
        return;
    }

    match apply_prepared_preview(
        &entry,
        load_preview_for_selected_entry(&entry, state.catalogs(), Some(&archive_cache)),
        &mut commands,
        &mut images,
        #[cfg(target_os = "windows")]
        &mut audio_sources,
        &mut tracker,
    ) {
        Ok(Some(preview)) => {
            state.set_preview_summary(preview.summary_text());
            #[cfg(target_os = "windows")]
            state.set_playback_summary(tracker.runtime_status(None));
            #[cfg(not(target_os = "windows"))]
            state.set_playback_summary(tracker.runtime_status());
        }
        Ok(None) => {
            state.set_preview_summary(format!(
                "No preview surface for {} yet.\nThis entry is cataloged, but the content lab does not decode it yet.",
                entry.family
            ));
            state.set_playback_summary("No active preview runtime.");
        }
        Err(error) => {
            state.set_preview_summary(format!(
                "Preview load failed for {}:\n{error}",
                entry.relative_path
            ));
            state.set_playback_summary("Runtime unavailable because preview decode failed.");
        }
    }
}

/// Drains all pending background preview-preparation messages in a single frame.
///
/// Video resources send a lightweight `FirstFrame` as soon as the opening frame
/// is decoded, then N `VqaStream` frame batches, then one final `VqaStream`
/// with `done: true` carrying the audio payload.  Non-video resources send only
/// `Full`.  The task resource is removed once the final message has been
/// consumed.
///
/// **Why drain instead of one-per-frame?**  The background decode thread can
/// outpace the 60 fps main loop, buffering many messages in the channel.  With
/// a single `try_recv()` per frame the done-batch (carrying audio) would not
/// arrive until N+2 frames later, producing an A/V delay proportional to the
/// number of batches (i.e. the movie length).  Draining eliminates that delay.
pub(crate) fn poll_content_preview_load(
    mut commands: Commands,
    task: Option<Res<ContentPreviewLoadTask>>,
    mut images: ResMut<Assets<Image>>,
    #[cfg(target_os = "windows")] mut audio_sources: ResMut<Assets<PcmAudioSource>>,
    mut state: ResMut<ContentLabState>,
    mut tracker: ResMut<ContentPreviewTracker>,
) {
    let Some(task) = task else {
        return;
    };

    let Ok(receiver) = task.receiver.lock() else {
        tracker.phase = PreviewPhase::Empty;
        state.set_preview_summary(
            "Background preview preparation failed because the preview receiver could not be locked.",
        );
        state.set_playback_summary("Runtime unavailable because preview preparation failed.");
        commands.remove_resource::<ContentPreviewLoadTask>();
        return;
    };

    // Drain every pending message in a single frame.  The background
    // decode thread sends one FirstFrame, then N VqaStream frame-batch
    // messages, then one VqaStream{done} with audio.  With a single
    // try_recv() per frame, the done-batch (carrying the audio payload)
    // would not be processed until N+2 frames later — during which the
    // video plays silently via timer.  For long cutscenes with many
    // batches, that delay grows proportionally and becomes audible as an
    // A/V desync that worsens with movie length.  Draining the channel
    // ensures the audio entity is created on the same frame all frames
    // arrive, keeping the delay to at most one frame regardless of batch
    // count.
    loop {
        match receiver.try_recv() {
            Ok(PreviewLoadMessage::FirstFrame {
                selection,
                entry: _,
                preview,
            }) => {
                if tracker.current_selection != Some(selection) {
                    continue;
                }
                // Build the visual session from the first frame and seed the
                // display image, but do NOT call apply_ready — we use
                // begin_streaming to enter the StreamingFirstFrame phase.
                if let Ok(Some(preview)) = &preview {
                    if let Some(visual) = preview.visual().cloned() {
                        if let Some(session) = VisualPreviewSession::new(
                            visual.frames().to_vec(),
                            visual.frame_duration_seconds(),
                        ) {
                            let initial_frame = session.current_frame();
                            tracker.display_image_handle =
                                Some(images.add(rgba_frame_to_image(initial_frame)));
                            tracker.begin_streaming(session);
                        }
                    }
                    state.set_preview_summary(preview.summary_text());
                }
            }
            Ok(PreviewLoadMessage::Full {
                selection,
                entry,
                preview,
            }) => {
                if tracker.current_selection != Some(selection) {
                    commands.remove_resource::<ContentPreviewLoadTask>();
                    break;
                }

                // If a first-frame session already exists, upgrade it in place so
                // the display surface stays stable.  Otherwise apply from scratch.
                let result = if tracker.visual_session().is_some() {
                    upgrade_preview_with_full_decode(
                        &entry,
                        preview,
                        &mut commands,
                        #[cfg(target_os = "windows")]
                        &mut audio_sources,
                        &mut tracker,
                    )
                } else {
                    apply_prepared_preview(
                        &entry,
                        preview,
                        &mut commands,
                        &mut images,
                        #[cfg(target_os = "windows")]
                        &mut audio_sources,
                        &mut tracker,
                    )
                };

                match result {
                    Ok(Some(preview)) => {
                        state.set_preview_summary(preview.summary_text());
                        #[cfg(target_os = "windows")]
                        state.set_playback_summary(tracker.runtime_status(None));
                        #[cfg(not(target_os = "windows"))]
                        state.set_playback_summary(tracker.runtime_status());
                    }
                    Ok(None) => {
                        state.set_preview_summary(format!(
                            "No preview surface for {} yet.\nThis entry is cataloged, but the content lab does not decode it yet.",
                            entry.family
                        ));
                        state.set_playback_summary("No active preview runtime.");
                    }
                    Err(error) => {
                        state.set_preview_summary(format!(
                            "Preview load failed for {}:\n{error}",
                            entry.relative_path
                        ));
                        state.set_playback_summary(
                            "Runtime unavailable because preview decode failed.",
                        );
                    }
                }

                commands.remove_resource::<ContentPreviewLoadTask>();
                break;
            }
            Ok(PreviewLoadMessage::VqaStream { selection, batch }) => {
                if tracker.current_selection != Some(selection) {
                    continue;
                }

                // Append streamed frames to the existing visual session.
                if !batch.frames.is_empty() {
                    tracker.push_streaming_frames(batch.frames);
                }

                // When the final batch arrives, transition to Ready.
                if batch.done {
                    let total_samples = batch.audio_samples.len();
                    let audio_info = if total_samples > 0 {
                        if let (Some(sample_rate), Some(channels)) =
                            (batch.audio_sample_rate, batch.audio_channels)
                        {
                            #[cfg(target_os = "windows")]
                            {
                                // Rotate the audio buffer so it starts at the
                                // same point in the movie as the video.  During
                                // streaming the video has been playing via timer,
                                // so it is ahead of second 0.  Without rotation
                                // the audio would start from the beginning while
                                // the video is already at frame N, producing an
                                // audible A/V desync.  Because the audio loops,
                                // the rotated buffer wraps seamlessly: both
                                // audio and video have the same total period and
                                // the same phase within that period.
                                let samples = rotate_audio_to_video_position(
                                    batch.audio_samples,
                                    sample_rate,
                                    channels,
                                    &tracker,
                                );
                                let pcm = PcmAudioSource::new(
                                    Arc::from(samples),
                                    sample_rate,
                                    channels,
                                );
                                tracker.audio_handle = Some(audio_sources.add(pcm));
                                respawn_preview_audio_entity(&mut commands, &mut tracker);
                            }
                            let duration_seconds = if sample_rate > 0 && channels > 0 {
                                total_samples as f32 / (sample_rate as f32 * channels as f32)
                            } else {
                                0.0
                            };
                            Some(AudioInfo { duration_seconds })
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    tracker.finalize_streaming(audio_info);
                    commands.remove_resource::<ContentPreviewLoadTask>();
                    break;
                }
            }
            Err(mpsc::TryRecvError::Empty) => { break; }
            Err(mpsc::TryRecvError::Disconnected) => {
                tracker.phase = PreviewPhase::Empty;
                state.set_preview_summary(
                    "Background preview preparation failed because the worker thread disconnected.",
                );
                state.set_playback_summary("Runtime unavailable because preview preparation failed.");
                commands.remove_resource::<ContentPreviewLoadTask>();
                break;
            }
        }
    }
}

/// Handles transport keys for the currently selected preview.
///
/// The content lab deliberately uses media-player style controls so a single
/// window can validate static art, animations, waveform-backed audio, and
/// video-like resources without inventing a separate UI flow for each format.
#[cfg(target_os = "windows")]
pub(crate) fn handle_content_preview_input(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut tracker: ResMut<ContentPreviewTracker>,
    mut images: ResMut<Assets<Image>>,
    mut image_query: Query<&mut ImageNode>,
) {
    if !tracker.has_transport() {
        return;
    }

    if keyboard.just_pressed(KeyCode::Space) {
        tracker.toggle_playback();
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        tracker.set_playback_requested(true);
        tracker.select_frame(0);
        if let Some(t) = tracker.frame_timer_seconds_mut() { *t = 0.0; }
        apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        respawn_preview_audio_entity(&mut commands, &mut tracker);
    }

    if tracker.has_animation() {
        if keyboard.just_pressed(KeyCode::Comma) {
            tracker.set_playback_requested(false);
            if let Some(t) = tracker.frame_timer_seconds_mut() { *t = 0.0; }
            let previous_frame =
                (tracker.current_frame_index() + tracker.frame_count() - 1) % tracker.frame_count();
            tracker.select_frame(previous_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
        if keyboard.just_pressed(KeyCode::Period) {
            tracker.set_playback_requested(false);
            if let Some(t) = tracker.frame_timer_seconds_mut() { *t = 0.0; }
            let next_frame = (tracker.current_frame_index() + 1) % tracker.frame_count();
            tracker.select_frame(next_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn handle_content_preview_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut tracker: ResMut<ContentPreviewTracker>,
    mut images: ResMut<Assets<Image>>,
    mut image_query: Query<&mut ImageNode>,
) {
    if !tracker.has_transport() {
        return;
    }

    if keyboard.just_pressed(KeyCode::Space) {
        tracker.toggle_playback();
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        tracker.set_playback_requested(true);
        tracker.select_frame(0);
        if let Some(t) = tracker.frame_timer_seconds_mut() { *t = 0.0; }
        apply_current_preview_frame(&tracker, &mut images, &mut image_query);
    }

    if tracker.has_animation() {
        if keyboard.just_pressed(KeyCode::Comma) {
            tracker.set_playback_requested(false);
            if let Some(t) = tracker.frame_timer_seconds_mut() { *t = 0.0; }
            let previous_frame =
                (tracker.current_frame_index() + tracker.frame_count() - 1) % tracker.frame_count();
            tracker.select_frame(previous_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
        if keyboard.just_pressed(KeyCode::Period) {
            tracker.set_playback_requested(false);
            if let Some(t) = tracker.frame_timer_seconds_mut() { *t = 0.0; }
            let next_frame = (tracker.current_frame_index() + 1) % tracker.frame_count();
            tracker.select_frame(next_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
    }
}

/// Keeps the Bevy audio sink in sync with the transport state requested by the
/// content-lab preview tracker.
///
/// The sink may not exist on the exact frame the preview entity is spawned, so
/// this system is intentionally idempotent: it re-applies the desired
/// play/pause state every update until the sink catches up.
///
/// **Important:** We look up the sink by `tracker.audio_entity` rather than
/// using a broad `Query<&AudioSink, With<ContentPreviewAudio>>`.  When the
/// user switches movies quickly, a despawn command for the old movie's audio
/// entity is queued but not yet flushed (Bevy commands are deferred).  A broad
/// query would still find the old entity's sink and erroneously un-pause it,
/// causing the previous movie's audio to play under the new movie's video.
/// Targeting by entity avoids this zombie-sink race entirely.
#[cfg(target_os = "windows")]
pub(crate) fn sync_content_preview_audio_state(
    tracker: Res<ContentPreviewTracker>,
    audio_sink_query: Query<&AudioSink, With<ContentPreviewAudio>>,
) {
    // Only operate on the sink that the tracker currently owns.  If the
    // tracker has no audio_entity (e.g. during a selection transition or for
    // non-audio content), we must not touch any sinks — they are zombies
    // awaiting deferred despawn.
    let Some(entity) = tracker.audio_entity else {
        return;
    };
    let Ok(sink) = audio_sink_query.get(entity) else {
        return;
    };

    if tracker.playback_requested() && sink.is_paused() {
        sink.play();
    } else if !tracker.playback_requested() && !sink.is_paused() {
        sink.pause();
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn sync_content_preview_audio_state() {}

/// Advances animated previews and keeps multi-frame video previews in sync.
///
/// For pure visual animations such as WSA, the tracker uses a local timer.
/// When audio exists too, such as VQA, the frame selection follows the audio
/// position so the video preview stays aligned with the decoded soundtrack.
#[cfg(target_os = "windows")]
pub(crate) fn advance_content_preview_animation(
    time: Res<Time>,
    mut tracker: ResMut<ContentPreviewTracker>,
    audio_sink_query: Query<&AudioSink, With<ContentPreviewAudio>>,
    mut images: ResMut<Assets<Image>>,
    mut image_query: Query<&mut ImageNode>,
) {
    // Accumulate the playlist loop timer.  Gate on `is_ready` so we don't
    // fire early during streaming (when frame_count is still partial).
    // Audio-only content never reaches Ready, so we use audio_only_loop_duration
    // as the fallback signal instead.
    let track_loop = (tracker.is_ready() && tracker.playback_requested())
        || tracker.audio_only_loop_duration.is_some();
    if track_loop {
        tracker.loop_elapsed_seconds += time.delta_secs();
        if let Some(duration) = tracker.loop_duration_seconds() {
            if tracker.loop_elapsed_seconds >= duration {
                tracker.loop_elapsed_seconds %= duration;
                tracker.playlist_advance_pending = true;
            }
        }
    }

    let Some(frame_duration) = tracker.frame_duration_seconds() else {
        return;
    };
    if !tracker.has_animation() || !tracker.playback_requested() {
        return;
    }

    let frame_count = tracker.frame_count();
    let current = tracker.current_frame_index();

    // When audio-syncing, derive the frame index from the audio sink's
    // playback position.  We look up the sink by tracker.audio_entity rather
    // than a broad query to avoid reading the position of a zombie sink that
    // belongs to a previously-selected movie (still alive because Bevy
    // commands are deferred).
    let next_frame = if tracker.has_audio() && tracker.sync_mode() == PlaybackSyncMode::AudioSync {
        tracker.audio_entity
            .and_then(|entity| audio_sink_query.get(entity).ok())
            .map(|sink| {
                let fc = frame_count.max(1);
                let cycle_seconds = frame_duration * fc as f32;
                let position = sink.position().as_secs_f32();
                ((position % cycle_seconds) / frame_duration).floor() as usize % fc
            })
    } else {
        let Some(timer) = tracker.frame_timer_seconds_mut() else {
            return;
        };
        *timer += time.delta_secs();
        let mut advanced = current;
        while *timer >= frame_duration {
            *timer -= frame_duration;
            advanced = (advanced + 1) % frame_count;
        }
        Some(advanced)
    };

    if let Some(next_frame) = next_frame {
        if current != next_frame {
            tracker.select_frame(next_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn advance_content_preview_animation(
    time: Res<Time>,
    mut tracker: ResMut<ContentPreviewTracker>,
    mut images: ResMut<Assets<Image>>,
    mut image_query: Query<&mut ImageNode>,
) {
    let track_loop = (tracker.is_ready() && tracker.playback_requested())
        || tracker.audio_only_loop_duration.is_some();
    if track_loop {
        tracker.loop_elapsed_seconds += time.delta_secs();
        if let Some(duration) = tracker.loop_duration_seconds() {
            if tracker.loop_elapsed_seconds >= duration {
                tracker.loop_elapsed_seconds %= duration;
                tracker.playlist_advance_pending = true;
            }
        }
    }

    let Some(frame_duration) = tracker.frame_duration_seconds() else {
        return;
    };
    if !tracker.has_animation() || !tracker.playback_requested() {
        return;
    }

    let frame_count = tracker.frame_count();
    let current = tracker.current_frame_index();

    let Some(timer) = tracker.frame_timer_seconds_mut() else {
        return;
    };
    *timer += time.delta_secs();
    let mut advanced = current;
    while *timer >= frame_duration {
        *timer -= frame_duration;
        advanced = (advanced + 1) % frame_count;
    }

    if current != advanced {
        tracker.select_frame(advanced);
        apply_current_preview_frame(&tracker, &mut images, &mut image_query);
    }
}

/// Refreshes the right-hand runtime-status panel from live preview state.
///
/// Unlike the static preview summary, this status can change every frame while
/// audio or animation is playing. It is kept separate so the decode summary
/// remains a stable description of the selected resource.
#[cfg(target_os = "windows")]
pub(crate) fn refresh_content_preview_status(
    tracker: Res<ContentPreviewTracker>,
    mut state: ResMut<ContentLabState>,
    audio_sink_query: Query<&AudioSink, With<ContentPreviewAudio>>,
) {
    // Target by tracker.audio_entity to avoid reading a zombie sink that
    // belongs to a previously-selected movie awaiting deferred despawn.
    let audio_sink = tracker.audio_entity
        .and_then(|entity| audio_sink_query.get(entity).ok());
    state.set_playback_summary(tracker.runtime_status(audio_sink));
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn refresh_content_preview_status(
    tracker: Res<ContentPreviewTracker>,
    mut state: ResMut<ContentLabState>,
) {
    state.set_playback_summary(tracker.runtime_status());
}

/// Advances to the next playlist or browse-all entry when a full playback
/// cycle has been detected by `advance_content_preview_animation`.
pub(crate) fn handle_playlist_advance(
    mut state: ResMut<ContentLabState>,
    mut tracker: ResMut<ContentPreviewTracker>,
) {
    if !tracker.playlist_advance_pending {
        return;
    }
    tracker.playlist_advance_pending = false;
    if state.is_autoplay_active() {
        state.advance_current_autoplay();
    }
}

/// Manages the GPU-composited CRT scanlines overlay for video playback.
///
/// A single fullscreen `MaterialNode<ScanlinesMaterial>` is lazily spawned
/// once and kept alive for the session.  Its visibility is toggled each frame
/// based on whether a VQA is actively playing and scanlines are enabled.
///
/// `half_row_height` is derived from the world-space billboard's displayed
/// size, not the inspector thumbnail, so the lines are always matched to the
/// actual on-screen video dimensions — exactly as OpenRA does with its
/// `halfRowHeight = round(videoScale * windowScale / 2)` formula.
pub(crate) fn sync_scanlines_overlay(
    mut commands: Commands,
    mut tracker: ResMut<ContentPreviewTracker>,
    mut scanlines_materials: ResMut<Assets<ScanlinesMaterial>>,
    billboard_query: Query<&Sprite, With<ContentPreviewBillboard>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    playback: Res<super::PlaybackSettings>,
) {
    // Scanlines are only shown when enabled in config, during video playback,
    // and while at least one frame has been decoded.
    let should_show = playback.0.scanlines
        && tracker
            .current_family()
            .map_or(false, |f| matches!(f, ContentFamily::Video))
        && tracker.has_visual();

    let half_row_height = if should_show {
        compute_scanline_half_row_height(&tracker, &billboard_query, &window_query).unwrap_or(3.0)
    } else {
        3.0
    };

    // Lazy-spawn the fullscreen overlay node once.
    if tracker.scanlines_overlay_entity.is_none() {
        let material_handle = scanlines_materials.add(ScanlinesMaterial {
            params: Vec4::new(half_row_height, 0.0, 0.0, 0.0),
        });
        let entity = commands
            .spawn((
                ContentScanlineOverlay,
                MaterialNode(material_handle.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                // Below the text panels (Z=100) but above the world-space billboard.
                GlobalZIndex(50),
                Visibility::Hidden,
            ))
            .id();
        tracker.scanlines_overlay_entity = Some(entity);
        tracker.scanlines_overlay_material = Some(material_handle);
    }

    // Toggle visibility.
    if let Some(entity) = tracker.scanlines_overlay_entity {
        commands.entity(entity).insert(if should_show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }

    // Update the uniform when half_row_height changes.
    if let Some(ref handle) = tracker.scanlines_overlay_material.clone() {
        if let Some(mat) = scanlines_materials.get_mut(handle.id()) {
            if (mat.params.x - half_row_height).abs() > 0.5 {
                mat.params.x = half_row_height;
            }
        }
    }
}

/// Computes `half_row_height` in physical screen pixels for the scanlines
/// overlay, matching OpenRA's `halfRowHeight` formula:
///
/// `halfRowHeight = round(displayedHeight / sourceHeight * dpiScale / 2)`
///
/// `displayedHeight` comes from the world-space billboard (`sprite.custom_size`)
/// so the computation reflects the actual on-screen video height at all window
/// sizes and aspect ratios.
fn compute_scanline_half_row_height(
    tracker: &ContentPreviewTracker,
    billboard_query: &Query<&Sprite, With<ContentPreviewBillboard>>,
    window_query: &Query<&Window, With<PrimaryWindow>>,
) -> Option<f32> {
    let frame = tracker.current_visual_frame()?;
    let frame_height = frame.height() as f32;
    if frame_height <= 0.0 {
        return None;
    }
    let display_height = billboard_query.single().ok()?.custom_size?.y;
    if display_height <= 0.0 {
        return None;
    }
    let window_scale = window_query.single().ok()?.scale_factor();
    // Clamp to at least 3 physical pixels — matches OpenRA's minimum
    // visibility on a 1080p display at standard zoom.
    let half_row = (display_height / frame_height * window_scale / 2.0)
        .round()
        .max(3.0);
    Some(half_row)
}

fn load_preview_for_selected_entry(
    entry: &ContentCatalogEntry,
    catalogs: &[super::catalog::ContentCatalog],
    cache: Option<&super::ArchivePreloadCache>,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    load_preview_for_entry(entry, catalogs, cache)
}

fn apply_prepared_preview(
    entry: &ContentCatalogEntry,
    preview: Result<Option<PreparedContentPreview>, PreviewLoadError>,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    #[cfg(target_os = "windows")] audio_sources: &mut Assets<PcmAudioSource>,
    tracker: &mut ContentPreviewTracker,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    #[cfg(not(target_os = "windows"))]
    let _ = commands;

    let Some(preview) = preview? else {
        return Ok(None);
    };

    let auto_play_visual = preview
        .visual()
        .and_then(|visual| visual.frame_duration_seconds())
        .is_some();

    // Build the visual session and seed the display image.
    let session = preview.visual().cloned().and_then(|visual| {
        let session =
            VisualPreviewSession::new(visual.frames().to_vec(), visual.frame_duration_seconds())?;
        let initial_frame = session.current_frame();
        tracker.display_image_handle =
            Some(images.add(rgba_frame_to_image(initial_frame)));
        Some(session)
    });

    // Build audio info and Bevy handles.
    let audio_info = preview.audio().map(|audio| {
        #[cfg(target_os = "windows")]
        {
            tracker.audio_handle =
                Some(audio_sources.add(PcmAudioSource::from_preview_audio(audio)));
            respawn_preview_audio_entity(commands, tracker);
        }
        AudioInfo {
            duration_seconds: audio.duration_seconds(),
        }
    });

    let text_available = preview.text_body().is_some();
    let sync_mode = if audio_info.is_some() && auto_play_visual {
        PlaybackSyncMode::AudioSync
    } else {
        PlaybackSyncMode::Timer
    };

    if let Some(session) = session {
        tracker.apply_ready(
            entry.family,
            session,
            auto_play_visual,
            sync_mode,
            audio_info,
            text_available,
        );
    } else if let Some(ref ai) = audio_info {
        // Audio-only content (e.g. AUD files): no visual session is created so
        // the phase never transitions to Ready.  Store the duration here so the
        // playlist loop-detection timer can still fire.
        tracker.audio_only_loop_duration = Some(ai.duration_seconds);
    }

    Ok(Some(preview))
}

/// Upgrades an existing first-frame preview session with the full background
/// decode result.
///
/// When the instant first-frame path already created a visual session and
/// display surface, this function replaces the session's frame list with the
/// complete decoded set and adds audio — without tearing down the display
/// image handle.  That avoids a visible flicker between the first-frame
/// preview and the full playback session.
fn upgrade_preview_with_full_decode(
    _entry: &ContentCatalogEntry,
    preview: Result<Option<PreparedContentPreview>, PreviewLoadError>,
    commands: &mut Commands,
    #[cfg(target_os = "windows")] audio_sources: &mut Assets<PcmAudioSource>,
    tracker: &mut ContentPreviewTracker,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    let Some(preview) = preview? else {
        return Ok(None);
    };

    let session = preview.visual().cloned().and_then(|visual| {
        VisualPreviewSession::new(visual.frames().to_vec(), visual.frame_duration_seconds())
    });

    let audio_info = preview.audio().map(|audio| {
        #[cfg(target_os = "windows")]
        {
            tracker.audio_handle =
                Some(audio_sources.add(PcmAudioSource::from_preview_audio(audio)));
            respawn_preview_audio_entity(commands, tracker);
        }
        AudioInfo {
            duration_seconds: audio.duration_seconds(),
        }
    });

    #[cfg(not(target_os = "windows"))]
    let _ = commands;

    let text_available = preview.text_body().is_some();

    if let Some(session) = session {
        tracker.upgrade_to_ready(session, audio_info, text_available);
    }

    Ok(Some(preview))
}

fn start_content_preview_load(
    selection: (usize, usize),
    entry: ContentCatalogEntry,
    catalogs: Vec<ContentCatalog>,
    archive_cache: super::ArchivePreloadCache,
) -> ContentPreviewLoadTask {
    let (sender, receiver) = mpsc::channel();
    let is_video = should_background_load_preview_for_family(entry.family);

    thread::Builder::new()
        .name("ic-content-preview-load".into())
        .spawn(move || {
            if is_video {
                // Streaming VQA path: read file once, decode first frame for
                // instant display, then stream remaining frames incrementally.
                let t0 = std::time::Instant::now();
                let bytes = match super::preview_decode::load_entry_bytes_cached(&entry, Some(&archive_cache)) {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = sender.send(PreviewLoadMessage::Full {
                            selection,
                            entry,
                            preview: Err(e),
                        });
                        return;
                    }
                };
                let t_file = t0.elapsed();
                eprintln!(
                    "[vqa-stream] load_entry_bytes: {:.1}ms ({} bytes, {})",
                    t_file.as_secs_f64() * 1000.0,
                    bytes.len(),
                    entry.relative_path,
                );

                // Decode + send first frame immediately.
                let t1 = std::time::Instant::now();
                match super::vqa_stream::decode_vqa_first_frame(&bytes) {
                    Ok(Some(first)) => {
                        if let Ok(first_preview) =
                            build_first_frame_preview(&entry, first)
                        {
                            let t_first = t1.elapsed();
                            eprintln!(
                                "[vqa-stream] first frame decoded: {:.1}ms (total from start: {:.1}ms)",
                                t_first.as_secs_f64() * 1000.0,
                                t0.elapsed().as_secs_f64() * 1000.0,
                            );
                            let _ = sender.send(PreviewLoadMessage::FirstFrame {
                                selection,
                                entry: entry.clone(),
                                preview: Ok(Some(first_preview)),
                            });
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(PreviewLoadMessage::Full {
                            selection,
                            entry,
                            preview: Ok(None),
                        });
                        return;
                    }
                    Err(e) => {
                        let _ = sender.send(PreviewLoadMessage::Full {
                            selection,
                            entry,
                            preview: Err(e.into()),
                        });
                        return;
                    }
                }

                // Stream remaining frames via VqaStreamBatch messages.
                // We wrap the batch sender to convert VqaStreamBatch into
                // PreviewLoadMessage::VqaStream inline.
                let (batch_tx, batch_rx) =
                    mpsc::channel::<super::vqa_stream::VqaStreamBatch>();

                // Spawn the decoder on the current thread — we're already on
                // the background thread.
                let decode_handle = {
                    let bytes_clone = bytes;
                    thread::Builder::new()
                        .name("ic-vqa-stream-decode".into())
                        .spawn(move || {
                            let _ = super::vqa_stream::stream_vqa_decode(
                                bytes_clone, batch_tx,
                            );
                        })
                        .expect("VQA stream decode thread should start")
                };

                // Relay decoded batches as PreviewLoadMessage on this thread.
                for batch in batch_rx {
                    if sender
                        .send(PreviewLoadMessage::VqaStream { selection, batch })
                        .is_err()
                    {
                        break;
                    }
                }

                let _ = decode_handle.join();
                eprintln!(
                    "[vqa-stream] full streaming decode done: {:.1}ms total",
                    t0.elapsed().as_secs_f64() * 1000.0,
                );
            } else {
                // Non-video path: single full decode.
                let preview = load_preview_for_selected_entry(&entry, &catalogs, Some(&archive_cache));
                let _ = sender.send(PreviewLoadMessage::Full {
                    selection,
                    entry,
                    preview,
                });
            }
        })
        .expect("content preview background load thread should start");

    ContentPreviewLoadTask {
        receiver: Mutex::new(receiver),
    }
}

/// Rotates the raw PCM sample buffer so audio playback begins at the same
/// point in the movie as the video's current frame.
///
/// During streaming, the video plays via a local timer and may be several
/// hundred milliseconds ahead of second 0 by the time the full audio decode
/// finishes.  Starting audio from sample 0 would leave it audibly behind.
///
/// This function calculates the video's elapsed time from the current frame
/// index and frame duration, converts that to a sample offset, and rotates
/// the buffer so that the corresponding audio starts first.  Because the
/// audio loops, the rotation is seamless: the total duration is unchanged
/// and both audio and video wrap at the same period with the same phase.
#[cfg(target_os = "windows")]
fn rotate_audio_to_video_position(
    mut samples: Vec<i16>,
    sample_rate: u32,
    channels: u16,
    tracker: &ContentPreviewTracker,
) -> Vec<i16> {
    let frame_index = tracker.current_frame_index();
    let frame_duration = tracker.frame_duration_seconds().unwrap_or(0.0);

    if frame_index == 0 || frame_duration <= 0.0 || samples.is_empty() {
        return samples;
    }

    let video_elapsed = frame_index as f32 * frame_duration;
    let samples_per_second = sample_rate as f32 * channels as f32;
    let skip_samples = (video_elapsed * samples_per_second).round() as usize;

    // Align to a whole frame (channels) boundary to avoid swapping L/R.
    let ch = channels.max(1) as usize;
    let skip_aligned = (skip_samples / ch) * ch;

    if skip_aligned == 0 || skip_aligned >= samples.len() {
        return samples;
    }

    // In-place rotation — no extra allocation.
    samples.rotate_left(skip_aligned);
    samples
}

#[cfg(target_os = "windows")]
fn respawn_preview_audio_entity(commands: &mut Commands, tracker: &mut ContentPreviewTracker) {
    if let Some(entity) = tracker.audio_entity.take() {
        commands.entity(entity).despawn();
    }

    let Some(audio_handle) = tracker.audio_handle.clone() else {
        return;
    };

    tracker.audio_entity = Some(
        commands
            .spawn((
                ContentPreviewAudio,
                // Bevy's `AudioPlayer::new` helper is hard-coded for the built-in
                // `AudioSource` type. For custom decodable assets such as our
                // direct-PCM preview path, we construct the generic tuple
                // component directly instead.
                AudioPlayer(audio_handle),
                PlaybackSettings::LOOP.paused(),
            ))
            .id(),
    );
}

fn apply_current_preview_frame(
    tracker: &ContentPreviewTracker,
    images: &mut Assets<Image>,
    image_query: &mut Query<&mut ImageNode>,
) {
    let Some(handle) = tracker.current_image_handle() else {
        return;
    };
    let Some(frame) = tracker.current_visual_frame() else {
        return;
    };

    if let Some(image) = images.get_mut(handle.id()) {
        update_image_from_rgba_frame(image, frame);
    }
    let Some(entity) = tracker.selected_image_entity else {
        return;
    };

    if let Ok(mut image_node) = image_query.get_mut(entity) {
        image_node.image = handle;
    }
}

/// Mirrors the selected preview into the always-visible world-space fallback
/// sprite.
///
/// This system is intentionally simple: it reads the selected decoded frame
/// handle from `ContentPreviewTracker`, fits that frame inside a large portion
/// of the current window, and updates one centered `Sprite`. If the Bevy UI
/// overlay fails to render for platform-specific reasons, the user still sees
/// the currently selected resource instead of only the old synthetic demo
/// image.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_content_preview_billboard(
    tracker: Res<ContentPreviewTracker>,
    state: Res<ContentLabState>,
    playback_settings: Res<super::PlaybackSettings>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    mut preview_query: Query<
        (&mut Sprite, &mut Transform, &mut Visibility),
        With<ContentPreviewBillboard>,
    >,
    mut loading_query: Query<
        (&mut Text2d, &mut Visibility),
        (With<ContentPreviewLoadingText>, Without<ContentPreviewBillboard>),
    >,
) {
    let Ok((mut sprite, mut transform, mut billboard_vis)) = preview_query.single_mut() else {
        return;
    };

    // Update loading text visibility — show feedback whenever the main
    // preview area would otherwise be empty/black.
    if let Ok((mut loading_text, mut loading_vis)) = loading_query.single_mut() {
        let show_loading = if tracker.is_loading() && !tracker.has_visual() {
            // Actively loading a preview in the background.
            let detail = tracker.current_family().map_or("", |f| match f {
                ContentFamily::Video => "Decoding video frames",
                _ => "Preparing preview",
            });
            *loading_text = Text2d::new(format!("Loading preview...\n{detail}"));
            true
        } else if tracker.current_selection.is_none() && !tracker.has_visual() {
            // No selection yet — catalog scan may still be running, or
            // finished but found no visual entries.
            let progress = state.scan_progress();
            if state.is_loading() {
                if progress.is_empty() {
                    *loading_text = Text2d::new("Scanning content sources...");
                } else {
                    *loading_text = Text2d::new(progress.to_string());
                }
            } else if !state.catalogs().is_empty() {
                let total: usize = state.catalogs().iter().map(|c| c.entries.len()).sum();
                *loading_text = Text2d::new(format!(
                    "No previewable resources found.\n{total} entries across {} sources.",
                    state.catalogs().len()
                ));
            } else {
                *loading_text = Text2d::new(
                    "No content sources configured.\nEdit config/iron-curtain.toml to add sources."
                );
            }
            true
        } else {
            false
        };
        *loading_vis = if show_loading {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    let Some(image_handle) = tracker.current_image_handle() else {
        *billboard_vis = Visibility::Hidden;
        sprite.custom_size = None;
        return;
    };

    let Some((frame_width, frame_height)) = tracker.frame_dimensions() else {
        *billboard_vis = Visibility::Hidden;
        sprite.custom_size = None;
        return;
    };

    let policy = preview_surface_policy_for_family(tracker.current_family(), &playback_settings.0);
    let fitted_size = primary_window
        .single()
        .ok()
        .map(|window| fit_preview_to_window(frame_width, frame_height, window, policy))
        .unwrap_or_else(|| Vec2::new(frame_width as f32, frame_height as f32));

    sprite.image = image_handle;
    sprite.custom_size = Some(fitted_size);
    *billboard_vis = Visibility::Visible;
    transform.translation = Vec3::new(0.0, 0.0, 10.0);
}

fn fit_preview_to_window(
    source_width: u32,
    source_height: u32,
    window: &Window,
    policy: PreviewSurfacePolicy,
) -> Vec2 {
    let max_width = window.width() * policy.max_width_fraction;
    let max_height = window.height() * policy.max_height_fraction;

    if source_width == 0 || source_height == 0 || max_width <= 0.0 || max_height <= 0.0 {
        return Vec2::new(max_width.max(1.0), max_height.max(1.0));
    }

    let x_scale = max_width / source_width as f32;
    let y_scale = max_height / source_height as f32;
    let scale = x_scale.min(y_scale);

    Vec2::new(
        (source_width as f32 * scale).max(1.0),
        (source_height as f32 * scale).max(1.0),
    )
}

pub(crate) fn rgba_frame_to_image(
    frame: &ic_render::sprite::RgbaSpriteFrame,
) -> Image {
    let pixels = frame.rgba8_pixels().to_vec();
    Image::new(
        Extent3d {
            width: frame.width(),
            height: frame.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Updates the live Bevy display image from a decoded RGBA frame.
///
/// The content lab uses one persistent display surface for the selected
/// preview. When successive frames have the same byte size, reusing the
/// existing pixel buffer avoids an unnecessary allocation during animation or
/// movie playback while keeping the API simple for the current eager-decode
/// stage.
pub(crate) fn update_image_from_rgba_frame(
    image: &mut Image,
    frame: &ic_render::sprite::RgbaSpriteFrame,
) {
    let pixels = frame.rgba8_pixels().to_vec();
    let image_data = image.data.get_or_insert_with(Vec::new);
    if image_data.len() == pixels.len() {
        image_data.copy_from_slice(&pixels);
    } else {
        *image_data = pixels;
    }
    image.texture_descriptor.size.width = frame.width();
    image.texture_descriptor.size.height = frame.height();
}

