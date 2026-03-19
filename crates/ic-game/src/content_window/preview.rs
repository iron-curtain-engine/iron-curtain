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

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::widget::ImageNode;

#[cfg(target_os = "windows")]
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings};

use super::catalog::ContentCatalogEntry;
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
    image_handles: Vec<Handle<Image>>,
    #[cfg(target_os = "windows")]
    audio_handle: Option<Handle<PcmAudioSource>>,
    #[cfg(target_os = "windows")]
    audio_entity: Option<Entity>,
    current_frame: usize,
    frame_duration_seconds: Option<f32>,
    frame_timer_seconds: f32,
    playback_requested: bool,
    audio_available: bool,
    audio_duration_seconds: Option<f32>,
    text_available: bool,
    frame_dimensions: Option<(u32, u32)>,
}

impl ContentPreviewTracker {
    #[cfg(target_os = "windows")]
    fn clear_dynamic_assets(
        &mut self,
        images: &mut Assets<Image>,
        audio_sources: &mut Assets<PcmAudioSource>,
    ) {
        for handle in self.image_handles.drain(..) {
            images.remove(handle.id());
        }
        if let Some(handle) = self.audio_handle.take() {
            audio_sources.remove(handle.id());
        }

        self.audio_entity = None;
        self.selected_image_entity = None;
        self.current_frame = 0;
        self.frame_duration_seconds = None;
        self.frame_timer_seconds = 0.0;
        self.playback_requested = false;
        self.audio_available = false;
        self.audio_duration_seconds = None;
        self.text_available = false;
        self.frame_dimensions = None;
    }

    #[cfg(not(target_os = "windows"))]
    fn clear_dynamic_assets(&mut self, images: &mut Assets<Image>) {
        for handle in self.image_handles.drain(..) {
            images.remove(handle.id());
        }

        self.current_frame = 0;
        self.frame_duration_seconds = None;
        self.frame_timer_seconds = 0.0;
        self.playback_requested = false;
        self.audio_available = false;
        self.audio_duration_seconds = None;
        self.text_available = false;
        self.frame_dimensions = None;
        self.selected_image_entity = None;
    }

    fn has_visual(&self) -> bool {
        !self.image_handles.is_empty()
    }

    fn has_animation(&self) -> bool {
        self.image_handles.len() > 1 && self.frame_duration_seconds.is_some()
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
        self.image_handles.len()
    }

    pub(crate) fn current_image_handle(&self) -> Option<Handle<Image>> {
        self.image_handles.get(self.current_frame).cloned()
    }

    pub(crate) fn frame_dimensions(&self) -> Option<(u32, u32)> {
        self.frame_dimensions
    }

    #[cfg(target_os = "windows")]
    fn runtime_status(&self, audio_sink: Option<&AudioSink>) -> String {
        let mut lines = Vec::new();

        if self.has_visual() {
            if let Some((width, height)) = self.frame_dimensions {
                if self.has_animation() {
                    let state = if self.playback_requested {
                        "playing"
                    } else {
                        "paused"
                    };
                    lines.push(format!(
                        "visual: {state} | frame {}/{} | {}x{}",
                        self.current_frame + 1,
                        self.frame_count(),
                        width,
                        height
                    ));
                } else {
                    lines.push(format!("visual: static | 1 frame | {}x{}", width, height));
                }
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
        let mut lines = Vec::new();

        if self.has_visual() {
            if let Some((width, height)) = self.frame_dimensions {
                if self.has_animation() {
                    let state = if self.playback_requested {
                        "playing"
                    } else {
                        "paused"
                    };
                    lines.push(format!(
                        "visual: {state} | frame {}/{} | {}x{}",
                        self.current_frame + 1,
                        self.frame_count(),
                        width,
                        height
                    ));
                } else {
                    lines.push(format!("visual: static | 1 frame | {}x{}", width, height));
                }
            }
        } else {
            lines.push("visual: none".into());
        }

        if self.has_audio() {
            lines.push(
                "audio: decoded PCM ready; Bevy playback is enabled on Windows builds".into(),
            );
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

    match load_preview_for_selected_entry(
        &entry,
        state.catalogs(),
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
        tracker.current_frame = 0;
        tracker.frame_timer_seconds = 0.0;
        apply_current_preview_frame(&tracker, &mut image_query);
        respawn_preview_audio_entity(&mut commands, &mut tracker);
    }

    if tracker.has_animation() {
        if keyboard.just_pressed(KeyCode::Comma) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
            tracker.current_frame =
                (tracker.current_frame + tracker.frame_count() - 1) % tracker.frame_count();
            apply_current_preview_frame(&tracker, &mut image_query);
        }
        if keyboard.just_pressed(KeyCode::Period) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
            tracker.current_frame = (tracker.current_frame + 1) % tracker.frame_count();
            apply_current_preview_frame(&tracker, &mut image_query);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn handle_content_preview_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut tracker: ResMut<ContentPreviewTracker>,
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
        tracker.current_frame = 0;
        tracker.frame_timer_seconds = 0.0;
        apply_current_preview_frame(&tracker, &mut image_query);
    }

    if tracker.has_animation() {
        if keyboard.just_pressed(KeyCode::Comma) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
            tracker.current_frame =
                (tracker.current_frame + tracker.frame_count() - 1) % tracker.frame_count();
            apply_current_preview_frame(&tracker, &mut image_query);
        }
        if keyboard.just_pressed(KeyCode::Period) {
            tracker.playback_requested = false;
            tracker.frame_timer_seconds = 0.0;
            tracker.current_frame = (tracker.current_frame + 1) % tracker.frame_count();
            apply_current_preview_frame(&tracker, &mut image_query);
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
    mut image_query: Query<&mut ImageNode>,
) {
    let Some(frame_duration) = tracker.frame_duration_seconds else {
        return;
    };
    if !tracker.has_animation() || !tracker.playback_requested {
        return;
    }

    let next_frame = if tracker.has_audio() {
        audio_sink_query.single().ok().map(|sink| {
            let frame_count = tracker.frame_count().max(1);
            let cycle_seconds = frame_duration * frame_count as f32;
            let position = sink.position().as_secs_f32();
            ((position % cycle_seconds) / frame_duration).floor() as usize % frame_count
        })
    } else {
        tracker.frame_timer_seconds += time.delta_secs();
        let mut advanced = tracker.current_frame;
        while tracker.frame_timer_seconds >= frame_duration {
            tracker.frame_timer_seconds -= frame_duration;
            advanced = (advanced + 1) % tracker.frame_count();
        }
        Some(advanced)
    };

    if let Some(next_frame) = next_frame {
        if tracker.current_frame != next_frame {
            tracker.current_frame = next_frame;
            apply_current_preview_frame(&tracker, &mut image_query);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn advance_content_preview_animation(
    time: Res<Time>,
    mut tracker: ResMut<ContentPreviewTracker>,
    mut image_query: Query<&mut ImageNode>,
) {
    let Some(frame_duration) = tracker.frame_duration_seconds else {
        return;
    };
    if !tracker.has_animation() || !tracker.playback_requested {
        return;
    }

    tracker.frame_timer_seconds += time.delta_secs();
    let mut advanced = tracker.current_frame;
    while tracker.frame_timer_seconds >= frame_duration {
        tracker.frame_timer_seconds -= frame_duration;
        advanced = (advanced + 1) % tracker.frame_count();
    }

    if tracker.current_frame != advanced {
        tracker.current_frame = advanced;
        apply_current_preview_frame(&tracker, &mut image_query);
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
    commands: &mut Commands,
    images: &mut Assets<Image>,
    #[cfg(target_os = "windows")] audio_sources: &mut Assets<PcmAudioSource>,
    tracker: &mut ContentPreviewTracker,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    #[cfg(not(target_os = "windows"))]
    let _ = commands;

    let Some(preview) = load_preview_for_entry(entry, catalogs)? else {
        return Ok(None);
    };

    if let Some(visual) = preview.visual() {
        tracker.image_handles = visual
            .frames()
            .iter()
            .map(|frame| images.add(rgba_frame_to_image(frame)))
            .collect();
        tracker.frame_duration_seconds = visual.frame_duration_seconds();
        tracker.frame_dimensions = visual
            .frames()
            .first()
            .map(|frame| (frame.width(), frame.height()));
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
    tracker.playback_requested = false;

    Ok(Some(preview))
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
    image_query: &mut Query<&mut ImageNode>,
) {
    let Some(handle) = tracker.current_image_handle() else {
        return;
    };
    let Some(entity) = tracker.selected_image_entity else {
        return;
    };

    if let Ok(mut image_node) = image_query.get_mut(entity) {
        image_node.image = handle;
    }
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
