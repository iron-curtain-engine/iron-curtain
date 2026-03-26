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
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use bevy::prelude::*;
use bevy::render::{ExtractSchedule, RenderApp};
use bevy::window::{
    MonitorSelection, PresentMode, Window, WindowMode, WindowPlugin, WindowResolution,
};
use bevy_framepace::{FramepacePlugin, FramepaceSettings, Limiter};
use ic_cnc_content::IcCncContentPlugin;
use ic_render::IcRenderPlugin;

use crate::demo::DemoSceneError;
use crate::LaunchOptions;

mod catalog;
mod debug_overlay;
mod gallery;
mod preview;
#[cfg(not(target_arch = "wasm32"))]
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
    advance_content_preview_animation, handle_content_preview_input, handle_playlist_advance,
    poll_content_preview_load, refresh_content_preview, refresh_content_preview_status,
    setup_content_window_scene, sync_content_preview_audio_state, sync_content_preview_billboard,
    sync_scanlines_overlay, ContentPreviewTracker, ScanlinesMaterial,
};
#[cfg(not(target_arch = "wasm32"))]
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

/// Stores the focused and unfocused FPS cap values so the focus-change system
/// can switch the `bevy_framepace` limiter without re-reading the TOML.
#[derive(Resource, Debug, Clone, Copy)]
struct PerformanceCapsSettings {
    focused_fps_cap: u32,
    unfocused_fps_cap: u32,
}

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

/// Persistent archive handle cache.
///
/// Keeps parsed `MixArchiveReader` / `MegArchiveReader` handles open for the
/// session so that browsing entries inside a MIX only pays the index-parse cost
/// once. Subsequent reads are a cheap seek + read.
///
/// Each reader is individually `Mutex`-wrapped so concurrent systems (e.g. the
/// VQA streaming thread and the main preview system) only block when accessing
/// the *same* archive file.
#[derive(Resource, Clone, Default)]
pub(crate) struct ArchiveHandleCache {
    mix_handles: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<ic_cnc_content::cnc_formats::mix::MixArchiveReader<std::io::BufReader<std::fs::File>>>>>>>,
    meg_handles: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<ic_cnc_content::cnc_formats::meg::MegArchiveReader<std::io::BufReader<std::fs::File>>>>>>>,
}

impl ArchiveHandleCache {
    /// Returns a persistent MIX reader handle, opening the archive on first access.
    pub(crate) fn get_or_open_mix(
        &self,
        path: &std::path::Path,
    ) -> Result<Arc<Mutex<ic_cnc_content::cnc_formats::mix::MixArchiveReader<std::io::BufReader<std::fs::File>>>>, ic_cnc_content::cnc_formats::Error> {
        use ic_cnc_content::cnc_formats::mix::MixArchiveReader;

        let mut map = self.mix_handles.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = map.get(path) {
            return Ok(Arc::clone(handle));
        }
        let file = std::fs::File::open(path).map_err(|e| ic_cnc_content::cnc_formats::Error::Io {
            context: "opening MIX archive for handle cache",
            kind: e.kind(),
        })?;
        let reader = MixArchiveReader::open(std::io::BufReader::new(file))?;
        let handle = Arc::new(Mutex::new(reader));
        map.insert(path.to_path_buf(), Arc::clone(&handle));
        Ok(handle)
    }

    /// Returns a persistent MEG reader handle, opening the archive on first access.
    pub(crate) fn get_or_open_meg(
        &self,
        path: &std::path::Path,
    ) -> Result<Arc<Mutex<ic_cnc_content::cnc_formats::meg::MegArchiveReader<std::io::BufReader<std::fs::File>>>>, ic_cnc_content::cnc_formats::Error> {
        use ic_cnc_content::cnc_formats::meg::MegArchiveReader;

        let mut map = self.meg_handles.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = map.get(path) {
            return Ok(Arc::clone(handle));
        }
        let file = std::fs::File::open(path).map_err(|e| ic_cnc_content::cnc_formats::Error::Io {
            context: "opening MEG archive for handle cache",
            kind: e.kind(),
        })?;
        let reader = MegArchiveReader::open(std::io::BufReader::new(file))?;
        let handle = Arc::new(Mutex::new(reader));
        map.insert(path.to_path_buf(), Arc::clone(&handle));
        Ok(handle)
    }

}

/// Virtual filesystem overlay built from all mounted MIX archives.
///
/// Uses cnc-formats' `MixOverlayIndex` to provide last-mounted-wins filename
/// resolution across all discovered MIX archives, matching the original game's
/// global `Retrieve("filename")` registry pattern.
#[derive(Resource, Clone, Default)]
pub(crate) struct MixVfs {
    overlay: ic_cnc_content::cnc_formats::mix::MixOverlayIndex<PathBuf>,
}

impl MixVfs {
    /// Resolves a filename across all mounted MIX archives.
    ///
    /// Returns the archive path and entry index of the winning entry.
    #[allow(dead_code)]
    pub(crate) fn resolve_name(&self, filename: &str) -> Option<(&Path, usize)> {
        self.overlay.resolve_name(filename).map(|record| {
            (record.source.as_path(), record.entry_index)
        })
    }

    /// Returns the number of unique CRCs in the overlay.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.overlay.len()
    }
}

/// Builds a `MixVfs` from finished catalog scan results.
fn build_mix_vfs(catalogs: &[ContentCatalog]) -> MixVfs {
    use ic_cnc_content::cnc_formats::mix::{MixArchiveReader, MixOverlayIndex};

    let mut overlay = MixOverlayIndex::new();

    // Collect unique MIX archive paths from the catalogs.
    let mut seen_archives: Vec<PathBuf> = Vec::new();
    for catalog in catalogs {
        for entry in &catalog.entries {
            if let ContentEntryLocation::MixMember { archive_path, parent_indices, .. } = &entry.location {
                // Only mount top-level archives (not nested ones — their entries
                // are already flattened into the catalog with parent_indices).
                if parent_indices.is_empty() && !seen_archives.contains(archive_path) {
                    seen_archives.push(archive_path.clone());
                }
            }
        }
    }

    for archive_path in &seen_archives {
        let file = match std::fs::File::open(archive_path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = match MixArchiveReader::open(std::io::BufReader::new(file)) {
            Ok(r) => r,
            Err(_) => continue,
        };
        overlay.mount_archive(archive_path.clone(), reader.entries());
    }

    MixVfs { overlay }
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
    // ── Phase 1: resolve the graphics profile before app startup ─────────────
    //
    // The full dispatch (modern vs classic frontend) happens in a later phase.
    // For now we resolve the effective profile, warn on unsupported choices,
    // and carry the result into the app as a resource for the F3 overlay.
    let resolved_profile = resolve_graphics_profile(&options);

    let source_roots = source_roots_from_config(&options.config);
    let content_state = ContentLabState::loading(source_roots);
    let (scan_task, archive_cache) = start_content_catalog_scan(
        content_state.configured_sources().to_vec(),
        options.preload_archives,
    );

    let display = &options.config.display;
    let perf = &options.config.performance;
    let cc = display.clear_color;

    let caps = PerformanceCapsSettings {
        focused_fps_cap: perf.fps_cap,
        unfocused_fps_cap: perf.unfocused_fps_cap,
    };

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(ImagePlugin::default_linear())
            .set(WindowPlugin {
                primary_window: Some(content_lab_window(display)),
                ..default()
            }),
    );
    #[cfg(not(target_arch = "wasm32"))]
    register_preview_audio_source(&mut app);
    app.add_plugins(FramepacePlugin)
        .add_plugins(IcCncContentPlugin)
        .add_plugins(IcRenderPlugin)
        .add_plugins(UiMaterialPlugin::<ScanlinesMaterial>::default())
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_plugins(bevy::diagnostic::SystemInformationDiagnosticsPlugin)
        .insert_resource(debug_overlay::DebugOverlayState::default())
        .insert_resource(debug_overlay::GpuMetrics::default())
        .insert_resource(resolved_profile);

    // Register the GPU-info extraction system in the render sub-app so it can
    // read RenderAdapterInfo (render world) and write GpuInfo back to the main
    // world via MainWorld.  Runs once on the first frame then early-returns.
    app.sub_app_mut(RenderApp)
        .add_systems(ExtractSchedule, debug_overlay::extract_gpu_info);

    app
        .insert_resource(FramepaceSettings {
            limiter: fps_cap_to_limiter(caps.focused_fps_cap),
        })
        .insert_resource(caps)
        .insert_resource(ClearColor(Color::srgb_u8(cc[0], cc[1], cc[2])))
        .insert_resource(content_state)
        .insert_resource(scan_task)
        .insert_resource(archive_cache)
        .insert_resource(ArchiveHandleCache::default())
        .insert_resource(MixVfs::default())
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
                debug_overlay::setup_debug_overlay,
                refresh_content_preview,
                refresh_content_gallery,
                sync_content_preview_billboard,
                refresh_content_preview_status,
                refresh_content_window_text,
            )
                .chain(),
        )
        .add_systems(
            Startup,
            // Windows-only PDH GPU utilization query.  Must run after
            // setup_debug_overlay so the GpuMetrics resource already exists.
            #[cfg(target_os = "windows")]
            debug_overlay::setup_gpu_pdh,
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
                sync_scanlines_overlay,
                handle_content_preview_input,
                sync_content_preview_audio_state,
                advance_content_preview_animation,
                handle_playlist_advance,
                sync_content_preview_billboard,
                refresh_content_preview_status,
            )
                .chain(),
        )
        .add_systems(
            Update,
            refresh_content_window_text.after(refresh_content_preview_status),
        )
        .add_systems(
            Update,
            (
                debug_overlay::refresh_gpu_metrics,
                debug_overlay::toggle_debug_overlay,
                debug_overlay::refresh_debug_overlay,
            )
                .chain(),
        )
        .add_systems(Update, update_framepace_on_focus_change);
    app.run();

    Ok(())
}

fn content_lab_window(display: &crate::config::DisplayConfig) -> Window {
    use crate::config::VsyncMode;
    let mode = match display.mode.as_str() {
        "windowed" => WindowMode::Windowed,
        "fullscreen" => WindowMode::Fullscreen(MonitorSelection::Primary, bevy::window::VideoModeSelection::Current),
        _ => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
    };
    // Map config enum to Bevy's PresentMode.  FifoRelaxed (the default)
    // avoids stutter from sim spikes without tearing during normal operation.
    let present_mode = match display.vsync {
        VsyncMode::Off         => PresentMode::AutoNoVsync,
        VsyncMode::Auto        => PresentMode::AutoVsync,
        VsyncMode::Fifo        => PresentMode::Fifo,
        VsyncMode::Mailbox     => PresentMode::Mailbox,
        VsyncMode::FifoRelaxed => PresentMode::FifoRelaxed,
    };
    Window {
        title: display.title.clone(),
        resolution: WindowResolution::new(display.width, display.height),
        mode,
        resizable: mode == WindowMode::Windowed,
        present_mode,
        ..default()
    }
}

/// Converts a configured FPS cap to a `bevy_framepace` limiter.
/// `0` → `Limiter::Auto` (matches the display's actual refresh rate).
/// `N` → `Limiter::Manual` at exactly N fps.
fn fps_cap_to_limiter(fps: u32) -> Limiter {
    if fps == 0 {
        Limiter::Auto
    } else {
        Limiter::from_framerate(fps as f64)
    }
}

/// Swaps the framepace limiter when the window gains or loses focus so the
/// unfocused cap kicks in automatically, saving CPU/GPU in the background.
fn update_framepace_on_focus_change(
    mut events: bevy::ecs::message::MessageReader<bevy::window::WindowFocused>,
    caps: Res<PerformanceCapsSettings>,
    mut framepace: ResMut<FramepaceSettings>,
) {
    for ev in events.read() {
        framepace.limiter = fps_cap_to_limiter(if ev.focused {
            caps.focused_fps_cap
        } else {
            caps.unfocused_fps_cap
        });
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
            eprintln!(
                "[catalog] scan complete: {} entries across {} sources | process RAM: {:.1} MB",
                total_entries,
                catalogs.len(),
                process_memory_mb(),
            );
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
            let total: usize = catalogs.iter().map(|c| c.entries.len()).sum();
            let vfs = build_mix_vfs(&catalogs);
            eprintln!(
                "[catalog] received: {} entries | VFS: {} CRCs | process RAM: {:.1} MB",
                total,
                vfs.len(),
                process_memory_mb(),
            );
            state.replace_catalogs(catalogs);
            commands.insert_resource(vfs);
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

/// Returns the current process memory usage in megabytes.
///
/// Uses the `sysinfo` crate (already a dependency via Bevy's `sysinfo_plugin`
/// feature) to query process memory safely — no `unsafe` FFI.
fn process_memory_mb() -> f64 {
    use sysinfo::{Pid, System};
    let pid = Pid::from_u32(std::process::id());
    let mut sys = System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
    );
    sys.process(pid)
        .map(|p| p.memory() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0)
}

// ─── Graphics profile resolution ─────────────────────────────────────────────

/// Resolves the effective graphics profile from config and CLI, and returns
/// the [`ResolvedProfile`] resource to insert into the Bevy app.
///
/// **Phase 1 behaviour (config+policy only):**
/// The classic frontend is not yet implemented.  Requesting `classic` or
/// `gpu = "off"` logs a warning and falls back to the modern (Bevy) frontend.
/// Phase 2 will add the actual frontend dispatch here before app startup.
fn resolve_graphics_profile(options: &LaunchOptions) -> debug_overlay::ResolvedProfile {
    use crate::config::{GpuPolicy, GraphicsProfile};

    let cfg = &options.config.graphics;

    // gpu = "off" forces classic regardless of profile.
    let wants_classic = cfg.gpu == GpuPolicy::Off
        || cfg.profile == GraphicsProfile::Classic;

    if wants_classic {
        eprintln!(
            "[graphics] classic (non-GPU) frontend requested but not yet implemented. \
             Falling back to modern frontend. (Phase 2 will add the classic path.)"
        );
    }

    // Effective profile: auto resolves to enhanced for the modern frontend.
    let effective = match cfg.profile {
        GraphicsProfile::Auto => GraphicsProfile::Enhanced,
        other => other,
    };

    // The modern Bevy frontend always runs with GPU active.
    debug_overlay::ResolvedProfile { profile: effective, gpu_active: true }
}

#[cfg(test)]
mod tests;
