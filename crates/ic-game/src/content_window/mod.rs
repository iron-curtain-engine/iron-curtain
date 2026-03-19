// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Content-browser window for the first real-data `G2` slice.
//!
//! This module keeps the first "resource lab" deliberately small and explicit.
//! It does three jobs:
//! - discover a few locally configured Red Alert / Remastered source roots
//! - scan them into a deterministic file catalog with support-status summaries
//! - render that catalog into a Bevy UI overlay inside `ic-game`
//!
//! The point is not to finish every viewer now. The point is to get a real
//! window pointed at real data so each next iteration can prove more coverage:
//! preview a SHP, inspect a palette, play an AUD, decode a VQA, and so on.

use bevy::prelude::*;
use bevy::window::{MonitorSelection, Window, WindowMode, WindowPlugin, WindowResolution};
use ic_cnc_content::IcCncContentPlugin;
use ic_render::IcRenderPlugin;

use crate::demo::{self, DemoSceneError};

mod catalog;
mod gallery;
mod preview;
#[cfg(target_os = "windows")]
mod preview_audio;
mod preview_decode;
mod state;

pub use catalog::{
    ContentCatalog, ContentCatalogEntry, ContentEntryLocation, ContentFamily, ContentRootShape,
    ContentSourceRoot, ContentSupportLevel,
};
use gallery::{refresh_content_gallery, setup_content_gallery_ui, ContentGalleryTracker};
pub use state::ContentLabState;

use preview::{
    advance_content_preview_animation, handle_content_preview_input, refresh_content_preview,
    refresh_content_preview_status, sync_content_preview_audio_state, ContentPreviewTracker,
};
#[cfg(target_os = "windows")]
use preview_audio::register_preview_audio_source;
use state::{
    handle_content_window_exit_shortcut, handle_content_window_input, refresh_content_window_text,
    setup_content_window_ui, EscapeExitShortcut,
};

// This is only the fallback size used before the window system applies the
// fullscreen request. It should fit comfortably on ordinary 1080p and smaller
// displays if the mode switch is delayed during startup.
const CONTENT_WINDOW_FALLBACK_WIDTH: u32 = 1280;
const CONTENT_WINDOW_FALLBACK_HEIGHT: u32 = 720;

/// Builds the first real-data content browser window.
///
/// This still keeps the synthetic demo sprite as a background proof that the
/// render path works, but the selection panel now drives actual SHP/PAL
/// previews loaded from local Red Alert data when possible.
pub fn run_content_window_client() -> Result<(), DemoSceneError> {
    let demo_scene = demo::build_demo_scene()?;
    let content_state = ContentLabState::from_default_local_sources();

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(WindowPlugin {
                primary_window: Some(content_lab_window()),
                ..default()
            }),
    );
    #[cfg(target_os = "windows")]
    register_preview_audio_source(&mut app);
    app.add_plugins(IcCncContentPlugin)
        .add_plugins(IcRenderPlugin)
        .insert_resource(ClearColor(Color::srgb_u8(15, 20, 24)))
        .insert_resource(demo_scene)
        .insert_resource(content_state)
        .insert_resource(EscapeExitShortcut::default())
        .insert_resource(ContentGalleryTracker::default())
        .insert_resource(ContentPreviewTracker::default())
        .add_systems(
            Startup,
            (
                demo::setup_demo_scene,
                refresh_content_preview,
                setup_content_gallery_ui,
                refresh_content_gallery,
                setup_content_window_ui,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                handle_content_window_input,
                handle_content_window_exit_shortcut,
                refresh_content_preview,
                refresh_content_gallery,
                handle_content_preview_input,
                sync_content_preview_audio_state,
                advance_content_preview_animation,
                refresh_content_preview_status,
            )
                .chain(),
        )
        .add_systems(
            Update,
            refresh_content_window_text.after(refresh_content_preview_status),
        );
    app.run();

    Ok(())
}

fn content_lab_window() -> Window {
    Window {
        title: "Iron Curtain - Content Lab".into(),
        resolution: WindowResolution::new(
            CONTENT_WINDOW_FALLBACK_WIDTH,
            CONTENT_WINDOW_FALLBACK_HEIGHT,
        ),
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        resizable: false,
        ..default()
    }
}

#[cfg(test)]
mod tests;
