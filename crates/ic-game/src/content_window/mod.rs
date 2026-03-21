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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, Window, WindowMode, WindowPlugin, WindowResolution};
use ic_cnc_content::IcCncContentPlugin;
use ic_render::IcRenderPlugin;

use crate::demo::DemoSceneError;
use crate::LaunchOptions;

mod catalog;
mod gallery;
mod preview;
#[cfg(target_os = "windows")]
mod preview_audio;
mod preview_decode;
mod state;
mod vqa_stream;

pub use catalog::{
    ContentCatalog, ContentCatalogEntry, ContentEntryLocation, ContentFamily, ContentRootShape,
    ContentSourceRoot, ContentSupportLevel, source_roots_from_config,
};
use gallery::{refresh_content_gallery, setup_content_gallery_ui, ContentGalleryTracker};
pub use state::ContentLabState;

use preview::{
    advance_content_preview_animation, handle_content_preview_input, poll_content_preview_load,
    refresh_content_preview, refresh_content_preview_status, setup_content_window_scene,
    sync_content_preview_audio_state, sync_content_preview_billboard, ContentPreviewTracker,
};
#[cfg(target_os = "windows")]
use preview_audio::register_preview_audio_source;
use state::{
    handle_content_window_exit_shortcut, handle_content_window_input, refresh_content_window_text,
    setup_content_window_ui, EscapeExitShortcut,
};

/// Background scan handle that feeds finished catalogs back into the app.
///
/// Red Alert sample discs and Remastered installs can be large enough that
/// fully scanning them before `App::run()` feels like a hang. This receiver
/// lets the window appear first, then applies the finished catalog set from a
/// worker thread during normal update ticks.
/// Bevy resource wrapping the playback section of the TOML config.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PlaybackSettings(pub(crate) crate::config::PlaybackConfig);

/// Bevy resource wrapping the audio section of the TOML config.
#[derive(Resource, Debug, Clone)]
#[allow(dead_code, reason = "audio volume will be consumed when the playback runtime reads it")]
pub(crate) struct AudioSettings(pub(crate) crate::config::AudioConfig);

/// In-memory cache of entire archive files, keyed by their absolute path.
///
/// When `--preload-archives` is active, the background catalog scan reads each
/// MIX/MEG file into RAM. Subsequent `load_entry_bytes` calls extract entries
/// from the cached buffer instead of hitting disk, turning each extraction into
/// a fast memcpy.
#[derive(Resource, Clone, Default)]
pub(crate) struct ArchivePreloadCache {
    pub(crate) archives: Arc<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>>,
}

#[derive(Resource)]
struct ContentCatalogScanTask {
    receiver: Mutex<Receiver<Vec<ContentCatalog>>>,
    progress: Mutex<Receiver<String>>,
}

/// Builds the first real-data content browser window.
///
/// The content lab now renders the selected preview resource directly onto the
/// main 2D scene instead of keeping the older synthetic bootstrap sprite in
/// the middle of the screen. That makes the visible world surface match the
/// selected RA asset rather than showing unrelated placeholder art.
pub fn run_content_window_client(options: LaunchOptions) -> Result<(), DemoSceneError> {
    let source_roots = source_roots_from_config(&options.config);
    let content_state = ContentLabState::loading(source_roots);
    let (scan_task, archive_cache) = start_content_catalog_scan(
        content_state.configured_sources().to_vec(),
        options.preload_archives,
    );

    let display = &options.config.display;
    let cc = display.clear_color;

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_linear())
            .set(WindowPlugin {
                primary_window: Some(content_lab_window(display)),
                ..default()
            }),
    );
    #[cfg(target_os = "windows")]
    register_preview_audio_source(&mut app);
    app.add_plugins(IcCncContentPlugin)
        .add_plugins(IcRenderPlugin)
        .insert_resource(ClearColor(Color::srgb_u8(cc[0], cc[1], cc[2])))
        .insert_resource(content_state)
        .insert_resource(scan_task)
        .insert_resource(archive_cache)
        .insert_resource(PlaybackSettings(options.config.playback.clone()))
        .insert_resource(AudioSettings(options.config.audio.clone()))
        .insert_resource(EscapeExitShortcut::default())
        .insert_resource(ContentGalleryTracker::default())
        .insert_resource(ContentPreviewTracker::default())
        .add_systems(
            Startup,
            (
                setup_content_window_scene,
                setup_content_window_ui,
                setup_content_gallery_ui,
                refresh_content_preview,
                refresh_content_gallery,
                sync_content_preview_billboard,
                refresh_content_preview_status,
                refresh_content_window_text,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                poll_content_catalog_scan,
                handle_content_window_input,
                handle_content_window_exit_shortcut,
                refresh_content_preview,
                poll_content_preview_load,
                refresh_content_gallery,
                handle_content_preview_input,
                sync_content_preview_audio_state,
                advance_content_preview_animation,
                sync_content_preview_billboard,
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

fn content_lab_window(display: &crate::config::DisplayConfig) -> Window {
    let mode = match display.mode.as_str() {
        "windowed" => WindowMode::Windowed,
        "fullscreen" => WindowMode::Fullscreen(MonitorSelection::Primary, bevy::window::VideoModeSelection::Current),
        _ => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
    };
    Window {
        title: display.title.clone(),
        resolution: WindowResolution::new(display.width, display.height),
        mode,
        resizable: mode == WindowMode::Windowed,
        ..default()
    }
}

fn start_content_catalog_scan(
    source_roots: Vec<ContentSourceRoot>,
    preload_archives: bool,
) -> (ContentCatalogScanTask, ArchivePreloadCache) {
    let (sender, receiver) = mpsc::channel();
    let (progress_sender, progress_receiver) = mpsc::channel();
    let cache = ArchivePreloadCache::default();
    let cache_for_thread = cache.clone();
    thread::Builder::new()
        .name("ic-content-catalog-scan".into())
        .spawn(move || {
            let source_count = source_roots.len();
            let catalogs: Vec<ContentCatalog> = source_roots
                .into_iter()
                .enumerate()
                .map(|(i, source)| {
                    let _ = progress_sender.send(format!(
                        "Scanning source {}/{}: {}",
                        i + 1,
                        source_count,
                        source.display_name
                    ));
                    ContentCatalog::scan_with_progress(source, &progress_sender)
                })
                .collect();

            if preload_archives {
                // Collect unique archive paths first so we know the total.
                let mut unique_archives: Vec<PathBuf> = Vec::new();
                for catalog in &catalogs {
                    for entry in &catalog.entries {
                        let archive_path = match &entry.location {
                            ContentEntryLocation::MixMember { archive_path, .. }
                            | ContentEntryLocation::MegMember { archive_path, .. } => {
                                archive_path.clone()
                            }
                            _ => continue,
                        };
                        if !unique_archives.contains(&archive_path) {
                            unique_archives.push(archive_path);
                        }
                    }
                }

                let total = unique_archives.len();
                let mut loaded = HashMap::new();
                for (i, archive_path) in unique_archives.into_iter().enumerate() {
                    let name = archive_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let pct = if total > 0 {
                        ((i + 1) * 100) / total
                    } else {
                        100
                    };
                    let _ = progress_sender.send(format!(
                        "Preloading into RAM: {name}\n{}/{total} archives ({pct}%)",
                        i + 1
                    ));
                    if let Ok(bytes) = std::fs::read(&archive_path) {
                        loaded.insert(archive_path, Arc::new(bytes));
                    }
                }
                if let Ok(mut map) = cache_for_thread.archives.lock() {
                    *map = loaded;
                }
            }

            let total_entries: usize = catalogs.iter().map(|c| c.entries.len()).sum();
            let _ = progress_sender.send(format!(
                "Scan complete: {} entries across {} sources",
                total_entries,
                catalogs.len()
            ));
            let _ = sender.send(catalogs);
        })
        .expect("content catalog background scan thread should start");

    let task = ContentCatalogScanTask {
        receiver: Mutex::new(receiver),
        progress: Mutex::new(progress_receiver),
    };
    (task, cache)
}

/// Applies the completed background filesystem scan once it is available.
///
/// The poll uses `try_recv` so the main Bevy thread stays responsive while the
/// scan is still running. Once catalogs arrive, the state switches out of its
/// loading placeholder and the normal preview/gallery systems pick up the new
/// selection on the same update loop.
fn poll_content_catalog_scan(
    mut commands: Commands,
    scan_task: Option<Res<ContentCatalogScanTask>>,
    mut state: ResMut<ContentLabState>,
) {
    let Some(scan_task) = scan_task else {
        return;
    };

    // Drain progress messages so the loading indicator stays up to date.
    if let Ok(progress) = scan_task.progress.lock() {
        while let Ok(msg) = progress.try_recv() {
            state.set_scan_progress(msg);
        }
    }

    let Ok(receiver) = scan_task.receiver.lock() else {
        state.set_preview_summary(
            "Background content scan failed because the scan receiver could not be locked.",
        );
        state.set_playback_summary("No active preview runtime.");
        commands.remove_resource::<ContentCatalogScanTask>();
        return;
    };

    match receiver.try_recv() {
        Ok(catalogs) => {
            state.replace_catalogs(catalogs);
            commands.remove_resource::<ContentCatalogScanTask>();
        }
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => {
            state.set_preview_summary(
                "Background content scan failed because the scanner thread disconnected.",
            );
            state.set_playback_summary("No active preview runtime.");
            commands.remove_resource::<ContentCatalogScanTask>();
        }
    }
}

#[cfg(test)]
mod tests;
