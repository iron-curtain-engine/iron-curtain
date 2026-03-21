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
#[derive(Debug)]
enum PreviewLoadMessage {
    /// A single decoded first frame, sent immediately so the content lab can
    /// show a visible image while the rest of the video is still decoding.
    FirstFrame {
        selection: (usize, usize),
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

/// Tracks the currently active visual/audio preview surfaces.
///
/// This resource lets the content lab coordinate several Bevy mechanisms that
/// are otherwise independent:
/// - temporary `Image` assets used by the sprite renderer
/// - temporary PCM audio assets used by Bevy audio playback
/// - frame timing and the user's current transport state
///
/// Keeping that state in one resource avoids re-decoding preview content every
/// frame and gives the right-hand diagnostics panel one canonical runtime
/// status source.
#[derive(Resource, Debug, Default)]
pub(crate) struct ContentPreviewTracker {
    current_selection: Option<(usize, usize)>,
    pub(crate) selected_image_entity: Option<Entity>,
    display_image_handle: Option<Handle<Image>>,
    visual_session: Option<VisualPreviewSession>,
    #[cfg(target_os = "windows")]
    audio_handle: Option<Handle<PcmAudioSource>>,
    #[cfg(target_os = "windows")]
    audio_entity: Option<Entity>,
    frame_timer_seconds: f32,
    playback_requested: bool,
    audio_available: bool,
    /// When true, video uses timer-based advance instead of audio-position
    /// sync.  Set for streaming VQA where audio arrives after playback has
    /// already started — syncing to audio position 0 would restart the video.
    force_timer_playback: bool,
    audio_duration_seconds: Option<f32>,
    text_available: bool,
    current_family: Option<ContentFamily>,
    loading: bool,
}

impl ContentPreviewTracker {
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
        self.selected_image_entity = None;
        self.visual_session = None;
        self.frame_timer_seconds = 0.0;
        self.playback_requested = false;
        self.audio_available = false;
        self.force_timer_playback = false;
        self.audio_duration_seconds = None;
        self.text_available = false;
        self.current_family = None;
        self.loading = false;
    }

    #[cfg(not(target_os = "windows"))]
    fn clear_dynamic_assets(&mut self, images: &mut Assets<Image>) {
        if let Some(handle) = self.display_image_handle.take() {
            images.remove(handle.id());
        }

        self.visual_session = None;
        self.frame_timer_seconds = 0.0;
        self.playback_requested = false;
        self.audio_available = false;
        self.force_timer_playback = false;
        self.audio_duration_seconds = None;
        self.text_available = false;
        self.selected_image_entity = None;
        self.current_family = None;
        self.loading = false;
    }

    fn has_visual(&self) -> bool {
        self.visual_session.is_some() && self.display_image_handle.is_some()
    }

    fn has_animation(&self) -> bool {
        self.visual_session.as_ref().is_some_and(|session| {
            session.frame_count() > 1 && session.frame_duration_seconds().is_some()
        })
    }

    fn has_audio(&self) -> bool {
        self.audio_available
    }

    fn has_text(&self) -> bool {
        self.text_available
    }

    fn has_transport(&self) -> bool {
        self.has_animation() || self.has_audio()
    }

    fn frame_count(&self) -> usize {
        self.visual_session
            .as_ref()
            .map_or(0, VisualPreviewSession::frame_count)
    }

    fn current_frame_index(&self) -> usize {
        self.visual_session
            .as_ref()
            .map_or(0, VisualPreviewSession::current_frame_index)
    }

    pub(crate) fn current_image_handle(&self) -> Option<Handle<Image>> {
        self.display_image_handle.clone()
    }

    fn current_visual_frame(&self) -> Option<&ic_render::sprite::RgbaSpriteFrame> {
        self.visual_session
            .as_ref()
            .map(VisualPreviewSession::current_frame)
    }

    pub(crate) fn frame_dimensions(&self) -> Option<(u32, u32)> {
        self.visual_session
            .as_ref()
            .map(VisualPreviewSession::current_frame_dimensions)
    }

    fn current_family(&self) -> Option<ContentFamily> {
        self.current_family
    }

    fn frame_duration_seconds(&self) -> Option<f32> {
        self.visual_session
            .as_ref()
            .and_then(VisualPreviewSession::frame_duration_seconds)
    }

    fn select_frame(&mut self, frame_index: usize) {
        if let Some(session) = &mut self.visual_session {
            session.select_frame(frame_index);
        }
    }

    #[cfg(target_os = "windows")]
    fn runtime_status(&self, audio_sink: Option<&AudioSink>) -> String {
        if self.loading {
            return "preview: loading in background\naudio: waiting for decoded preview\ntransport: unavailable during preparation".into();
        }

        let mut lines = Vec::new();

        if self.has_visual() {
            if let Some((width, height)) = self.frame_dimensions() {
                if self.has_animation() {
                    let state = if self.playback_requested {
                        "playing"
                    } else {
                        "paused"
                    };
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
            if let Some(session) = &self.visual_session {
                lines.push(session.runtime_summary());
            }
        } else {
            lines.push("visual: none".into());
        }

        if self.has_audio() {
            match audio_sink {
                Some(sink) => {
                    let state = if sink.is_paused() {
                        "paused"
                    } else {
                        "playing"
                    };
                    let total = self.audio_duration_seconds.unwrap_or_default();
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
                if self.playback_requested {
                    "running"
                } else {
                    "paused"
                }
            ));
        } else {
            lines.push("transport: not applicable".into());
        }

        lines.join("\n")
    }

    #[cfg(not(target_os = "windows"))]
    fn runtime_status(&self) -> String {
        if self.loading {
            return "preview: loading in background\naudio: waiting for decoded preview\ntransport: unavailable during preparation".into();
        }

        let mut lines = Vec::new();

        if self.has_visual() {
            if let Some((width, height)) = self.frame_dimensions() {
                if self.has_animation() {
                    let state = if self.playback_requested {
                        "playing"
                    } else {
                        "paused"
                    };
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
            if let Some(session) = &self.visual_session {
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
                if self.playback_requested {
                    "running"
                } else {
                    "paused"
                }
            ));
        } else {
            lines.push("transport: not applicable".into());
        }

        lines.join("\n")
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
    existing_audio_entities: Query<Entity, With<ContentPreviewAudio>>,
) {
    let selection = state.selected_location();
    if tracker.current_selection == selection {
        return;
    }

    for entity in &existing_audio_entities {
        commands.entity(entity).despawn();
    }
    #[cfg(target_os = "windows")]
    tracker.clear_dynamic_assets(&mut images, &mut audio_sources);
    #[cfg(not(target_os = "windows"))]
    tracker.clear_dynamic_assets(&mut images);
    tracker.current_selection = selection;
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
    tracker.current_family = Some(entry.family);

    if should_background_load_preview_for_family(entry.family) {
        tracker.loading = true;
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

/// Applies background preview-preparation messages as they arrive.
///
/// Video resources send two messages: a lightweight `FirstFrame` as soon as the
/// opening frame is decoded (typically within milliseconds of the file read),
/// followed by a `Full` result once all remaining frames and audio are ready.
/// Non-video resources send only `Full`.  The task resource is removed once the
/// final message has been consumed.
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
        tracker.loading = false;
        state.set_preview_summary(
            "Background preview preparation failed because the preview receiver could not be locked.",
        );
        state.set_playback_summary("Runtime unavailable because preview preparation failed.");
        commands.remove_resource::<ContentPreviewLoadTask>();
        return;
    };

    match receiver.try_recv() {
        Ok(PreviewLoadMessage::FirstFrame {
            selection,
            entry,
            preview,
        }) => {
            if tracker.current_selection != Some(selection) {
                return;
            }
            if let Ok(Some(preview)) = apply_prepared_preview(
                &entry,
                preview,
                &mut commands,
                &mut images,
                #[cfg(target_os = "windows")]
                &mut audio_sources,
                &mut tracker,
            ) {
                state.set_preview_summary(preview.summary_text());
            }
            // Start playback immediately — don't wait for all frames.
            // Streaming batches will append more frames as they arrive.
            tracker.playback_requested = true;
            // Audio arrives later; use timer-based advance so the video
            // doesn't jump back to frame 0 when audio-sync kicks in.
            tracker.force_timer_playback = true;
            // Keep loading — streaming batches follow.
            tracker.loading = true;
        }
        Ok(PreviewLoadMessage::Full {
            selection,
            entry,
            preview,
        }) => {
            if tracker.current_selection != Some(selection) {
                commands.remove_resource::<ContentPreviewLoadTask>();
                return;
            }

            tracker.loading = false;

            // If a first-frame session already exists, upgrade it in place so
            // the display surface stays stable.  Otherwise apply from scratch.
            let result = if tracker.visual_session.is_some() {
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
        }
        Ok(PreviewLoadMessage::VqaStream { selection, batch }) => {
            if tracker.current_selection != Some(selection) {
                return;
            }

            // Append streamed frames to the existing visual session.
            if !batch.frames.is_empty() {
                if let Some(session) = tracker.visual_session.as_mut() {
                    session.push_frames(batch.frames);
                }
            }

            // When the final batch arrives with audio, create the audio asset
            // and reset playback to frame 0 so the audio-synced animation
            // doesn't jump back after timer-based frames have already advanced.
            if batch.done {
                let total_samples = batch.audio_samples.len();
                if total_samples > 0 {
                    if let (Some(sample_rate), Some(channels)) =
                        (batch.audio_sample_rate, batch.audio_channels)
                    {
                        tracker.audio_available = true;
                        let duration_seconds = if sample_rate > 0 && channels > 0 {
                            total_samples as f32 / (sample_rate as f32 * channels as f32)
                        } else {
                            0.0
                        };
                        tracker.audio_duration_seconds = Some(duration_seconds);

                        #[cfg(target_os = "windows")]
                        {
                            let pcm = PcmAudioSource::new(
                                Arc::from(batch.audio_samples),
                                sample_rate,
                                channels,
                            );
                            tracker.audio_handle = Some(audio_sources.add(pcm));
                            respawn_preview_audio_entity(&mut commands, &mut tracker);
                        }
                    }
                }
                // Audio arrived — do NOT reset to frame 0. Playback already
                // started on FirstFrame; restarting would cause a visible skip.
                tracker.loading = false;
                commands.remove_resource::<ContentPreviewLoadTask>();
            }
        }
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => {
            tracker.loading = false;
            state.set_preview_summary(
                "Background preview preparation failed because the worker thread disconnected.",
            );
            state.set_playback_summary("Runtime unavailable because preview preparation failed.");
            commands.remove_resource::<ContentPreviewLoadTask>();
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
        tracker.playback_requested = !tracker.playback_requested;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        tracker.playback_requested = true;
        tracker.select_frame(0);
        tracker.frame_timer_seconds = 0.0;
        apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        respawn_preview_audio_entity(&mut commands, &mut tracker);
    }

    if tracker.has_animation() {
        if keyboard.just_pressed(KeyCode::Comma) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
            let previous_frame =
                (tracker.current_frame_index() + tracker.frame_count() - 1) % tracker.frame_count();
            tracker.select_frame(previous_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
        if keyboard.just_pressed(KeyCode::Period) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
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
        tracker.playback_requested = !tracker.playback_requested;
    }

    if keyboard.just_pressed(KeyCode::Enter) {
        tracker.playback_requested = true;
        tracker.select_frame(0);
        tracker.frame_timer_seconds = 0.0;
        apply_current_preview_frame(&tracker, &mut images, &mut image_query);
    }

    if tracker.has_animation() {
        if keyboard.just_pressed(KeyCode::Comma) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
            let previous_frame =
                (tracker.current_frame_index() + tracker.frame_count() - 1) % tracker.frame_count();
            tracker.select_frame(previous_frame);
            apply_current_preview_frame(&tracker, &mut images, &mut image_query);
        }
        if keyboard.just_pressed(KeyCode::Period) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
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
#[cfg(target_os = "windows")]
pub(crate) fn sync_content_preview_audio_state(
    tracker: Res<ContentPreviewTracker>,
    audio_sink_query: Query<&AudioSink, With<ContentPreviewAudio>>,
) {
    let Ok(sink) = audio_sink_query.single() else {
        return;
    };

    if tracker.playback_requested && sink.is_paused() {
        sink.play();
    } else if !tracker.playback_requested && !sink.is_paused() {
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
    let Some(frame_duration) = tracker.frame_duration_seconds() else {
        return;
    };
    if !tracker.has_animation() || !tracker.playback_requested {
        return;
    }

    let next_frame = if tracker.has_audio() && !tracker.force_timer_playback {
        audio_sink_query.single().ok().map(|sink| {
            let frame_count = tracker.frame_count().max(1);
            let cycle_seconds = frame_duration * frame_count as f32;
            let position = sink.position().as_secs_f32();
            ((position % cycle_seconds) / frame_duration).floor() as usize % frame_count
        })
    } else {
        tracker.frame_timer_seconds += time.delta_secs();
        let mut advanced = tracker.current_frame_index();
        while tracker.frame_timer_seconds >= frame_duration {
            tracker.frame_timer_seconds -= frame_duration;
            advanced = (advanced + 1) % tracker.frame_count();
        }
        Some(advanced)
    };

    if let Some(next_frame) = next_frame {
        if tracker.current_frame_index() != next_frame {
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
    let Some(frame_duration) = tracker.frame_duration_seconds() else {
        return;
    };
    if !tracker.has_animation() || !tracker.playback_requested {
        return;
    }

    tracker.frame_timer_seconds += time.delta_secs();
    let mut advanced = tracker.current_frame_index();
    while tracker.frame_timer_seconds >= frame_duration {
        tracker.frame_timer_seconds -= frame_duration;
        advanced = (advanced + 1) % tracker.frame_count();
    }

    if tracker.current_frame_index() != advanced {
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
    let audio_sink = audio_sink_query.single().ok();
    state.set_playback_summary(tracker.runtime_status(audio_sink));
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn refresh_content_preview_status(
    tracker: Res<ContentPreviewTracker>,
    mut state: ResMut<ContentLabState>,
) {
    state.set_playback_summary(tracker.runtime_status());
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

    tracker.loading = false;
    tracker.current_family = Some(entry.family);

    if let Some(visual) = preview.visual().cloned() {
        if let Some(session) =
            VisualPreviewSession::new(visual.frames().to_vec(), visual.frame_duration_seconds())
        {
            let initial_frame = session.current_frame();
            tracker.display_image_handle = Some(images.add(rgba_frame_to_image(initial_frame)));
            tracker.visual_session = Some(session);
        }
    }

    if let Some(audio) = preview.audio() {
        tracker.audio_available = true;
        tracker.audio_duration_seconds = Some(audio.duration_seconds());

        #[cfg(target_os = "windows")]
        {
            tracker.audio_handle =
                Some(audio_sources.add(PcmAudioSource::from_preview_audio(audio)));
            respawn_preview_audio_entity(commands, tracker);
        }
    }

    tracker.text_available = preview.text_body().is_some();
    tracker.playback_requested = auto_play_visual;

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
    entry: &ContentCatalogEntry,
    preview: Result<Option<PreparedContentPreview>, PreviewLoadError>,
    commands: &mut Commands,
    #[cfg(target_os = "windows")] audio_sources: &mut Assets<PcmAudioSource>,
    tracker: &mut ContentPreviewTracker,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    let Some(preview) = preview? else {
        return Ok(None);
    };

    tracker.loading = false;
    tracker.current_family = Some(entry.family);

    if let Some(visual) = preview.visual().cloned() {
        if let Some(session) =
            VisualPreviewSession::new(visual.frames().to_vec(), visual.frame_duration_seconds())
        {
            tracker.visual_session = Some(session);
        }
    }

    if let Some(audio) = preview.audio() {
        tracker.audio_available = true;
        tracker.audio_duration_seconds = Some(audio.duration_seconds());

        #[cfg(target_os = "windows")]
        {
            tracker.audio_handle =
                Some(audio_sources.add(PcmAudioSource::from_preview_audio(audio)));
            respawn_preview_audio_entity(commands, tracker);
        }
    }

    #[cfg(not(target_os = "windows"))]
    let _ = commands;

    tracker.text_available = preview.text_body().is_some();
    tracker.playback_requested = true;

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
        let show_loading = if tracker.loading && !tracker.has_visual() {
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

pub(crate) fn rgba_frame_to_image(frame: &ic_render::sprite::RgbaSpriteFrame) -> Image {
    Image::new(
        Extent3d {
            width: frame.width(),
            height: frame.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        frame.rgba8_pixels().to_vec(),
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
    let frame_pixels = frame.rgba8_pixels();
    let image_data = image.data.get_or_insert_with(Vec::new);
    if image_data.len() == frame_pixels.len() {
        image_data.copy_from_slice(frame_pixels);
    } else {
        *image_data = frame_pixels.to_vec();
    }
    image.texture_descriptor.size.width = frame.width();
    image.texture_descriptor.size.height = frame.height();
}
