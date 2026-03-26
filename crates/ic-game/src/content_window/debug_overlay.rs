// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! F3 debug overlay — on-screen telemetry for CPU, RAM, FPS, frame time, GPU
//! identity, and content-lab diagnostics.
//!
//! Toggle visibility with **F3**. The overlay sits in the top-right corner and
//! updates every 250 ms (4 Hz) to avoid impacting the metrics it reports.
//!
//! ## GPU info
//!
//! `GpuInfo` is populated once from the render world via `extract_gpu_info`,
//! which runs in `ExtractSchedule` on the first frame and writes back to the
//! main world through `MainWorld`.  The resource is then available to the
//! regular main-world `refresh_debug_overlay` system.

use bevy::diagnostic::{
    DiagnosticsStore, FrameTimeDiagnosticsPlugin, SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::render::MainWorld;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;

use crate::config::GraphicsProfile;

// ─── Resolved profile ────────────────────────────────────────────────────────

/// The graphics/GPU profile resolved at startup and active for this session.
///
/// Populated once in [`run_content_window_client`] before the Bevy app starts
/// and stored as a resource so [`refresh_debug_overlay`] can display it.
/// The `gpu_active` field reflects whether the GPU renderer is actually
/// running — always `true` for the modern frontend, `false` for classic.
#[derive(Resource, Clone)]
pub(crate) struct ResolvedProfile {
    /// The effective profile in use this session.
    pub profile: GraphicsProfile,
    /// Whether the GPU renderer is currently active.
    pub gpu_active: bool,
}

impl Default for ResolvedProfile {
    fn default() -> Self {
        // The modern (Bevy) frontend always runs with GPU active.
        Self { profile: GraphicsProfile::Enhanced, gpu_active: true }
    }
}

// ─── GPU info ────────────────────────────────────────────────────────────────

/// Static GPU identity captured once from the wgpu render adapter.
///
/// Populated by [`extract_gpu_info`] on the first render frame and stored in
/// the main world so [`refresh_debug_overlay`] can read it without touching
/// the render world.
#[derive(Resource, Default)]
pub(crate) struct GpuInfo {
    /// Adapter/device name reported by the driver (e.g. "NVIDIA GeForce RTX 4080").
    pub adapter_name: String,
    /// Graphics API in use: "DX12", "Vulkan", "Metal", "OpenGL", or "Unknown".
    pub backend: String,
    /// Device class: "Discrete", "Integrated", "Virtual", "CPU (SW)", or "Unknown".
    pub device_type: String,
    /// Driver version/info string, if reported by the adapter.
    pub driver: String,
}

/// Extraction system — runs in `ExtractSchedule` (render world), writes once to
/// the main world through `MainWorld`.  Early-returns on subsequent frames.
pub(crate) fn extract_gpu_info(
    adapter_info: Res<RenderAdapterInfo>,
    mut main_world: ResMut<MainWorld>,
) {
    if main_world.contains_resource::<GpuInfo>() {
        return;
    }
    let info = &*adapter_info.0;
    // wgpu is not a direct dependency — derive display strings from Debug.
    let backend = match format!("{:?}", info.backend).as_str() {
        "Dx12" => "DX12".to_string(),
        "Gl" => "OpenGL".to_string(),
        "BrowserWebGpu" => "WebGPU".to_string(),
        other => other.to_string(),
    };
    let device_type = match format!("{:?}", info.device_type).as_str() {
        "DiscreteGpu" => "Discrete".to_string(),
        "IntegratedGpu" => "Integrated".to_string(),
        "VirtualGpu" => "Virtual".to_string(),
        "Cpu" => "CPU (SW)".to_string(),
        other => other.to_string(),
    };
    main_world.insert_resource(GpuInfo {
        adapter_name: info.name.clone(),
        backend,
        device_type,
        driver: info.driver_info.clone(),
    });
}

// ─── Live GPU metrics ─────────────────────────────────────────────────────────

/// Live GPU metrics updated every overlay refresh cycle.
///
/// Populated by [`refresh_gpu_metrics`] on every `Update` tick while the
/// overlay is visible.  On non-Windows platforms the fields remain at their
/// default (0 / `None`) values.
#[derive(Resource, Default)]
pub(crate) struct GpuMetrics {
    /// Dedicated VRAM currently in use (MB).  0 if unavailable.
    pub vram_used_mb: f32,
    /// Dedicated VRAM budget reported by the driver (MB).  0 if unavailable.
    pub vram_budget_mb: f32,
    /// Shared (non-local) GPU memory in use (MB).  0 if unavailable.
    pub shared_used_mb: f32,
    /// GPU 3D-engine utilisation 0–100 %, or `None` if not available.
    pub gpu_util_pct: Option<f32>,
}

/// Bevy system — refreshes [`GpuMetrics`] from DXGI (VRAM) and PDH (GPU%).
///
/// No-op on non-Windows; on Windows it queries the first DXGI adapter for
/// VRAM usage and reads the PDH GPU Engine utilisation counter.
pub(crate) fn refresh_gpu_metrics(
    mut metrics: ResMut<GpuMetrics>,
    #[cfg(windows)] pdh: Option<ResMut<GpuPdhState>>,
) {
    #[cfg(windows)]
    {
        if let Some((used, budget, shared)) = query_dxgi_vram() {
            metrics.vram_used_mb = used;
            metrics.vram_budget_mb = budget;
            metrics.shared_used_mb = shared;
        }
        if let Some(mut state) = pdh {
            if state.ready {
                metrics.gpu_util_pct = collect_gpu_util_pct(&mut state);
            }
        }
    }
    // Suppress unused-variable warning on non-Windows.
    #[cfg(not(windows))]
    let _ = &metrics;
}

// ─── Windows-only GPU query state ─────────────────────────────────────────────

/// PDH query + counter handles for GPU 3D-engine utilization.
///
/// `PDH_HQUERY` / `PDH_HCOUNTER` wrap `*mut c_void` (raw Windows handles).
/// Safety: these handles are only ever accessed from Bevy's main thread via
/// the `Update` schedule, so `Send + Sync` is safe for this usage pattern.
#[cfg(windows)]
pub(crate) struct GpuPdhState {
    query: windows::Win32::System::Performance::PDH_HQUERY,
    counter_3d: windows::Win32::System::Performance::PDH_HCOUNTER,
    ready: bool,
}

#[cfg(windows)]
// SAFETY: Only accessed from Bevy's main thread (Update systems).
unsafe impl Send for GpuPdhState {}
#[cfg(windows)]
unsafe impl Sync for GpuPdhState {}

#[cfg(windows)]
impl bevy::prelude::Resource for GpuPdhState {}

#[cfg(windows)]
impl Drop for GpuPdhState {
    fn drop(&mut self) {
        if !self.query.0.is_null() {
            unsafe { let _ = windows::Win32::System::Performance::PdhCloseQuery(self.query); }
        }
    }
}

/// Startup system (Windows only) — opens the PDH query for GPU 3D utilization.
///
/// Silently skips if the `GPU Engine` performance counter object is absent
/// (e.g. on older Windows versions or certain driver configurations).
#[cfg(windows)]
pub(crate) fn setup_gpu_pdh(mut commands: Commands) {
    use windows::Win32::System::Performance::*;

    let state = unsafe {
        let mut query = PDH_HQUERY::default();
        if PdhOpenQueryW(windows::core::PCWSTR::null(), 0, &mut query) != 0 {
            return;
        }
        let path: Vec<u16> =
            "\\GPU Engine(*engtype_3D)\\Utilization Percentage\0".encode_utf16().collect();
        let mut counter = PDH_HCOUNTER::default();
        if PdhAddEnglishCounterW(
            query,
            windows::core::PCWSTR(path.as_ptr()),
            0,
            &mut counter,
        ) != 0
        {
            let _ = PdhCloseQuery(query);
            return;
        }
        // Baseline sample — actual rate values arrive on the next collect.
        PdhCollectQueryData(query);
        GpuPdhState { query, counter_3d: counter, ready: true }
    };
    commands.insert_resource(state);
}

// ─── Windows GPU query helpers ────────────────────────────────────────────────

/// Queries VRAM usage from the first DXGI adapter.
///
/// Uses `dxgi.dll` — a Windows system component, no external DLLs needed.
/// Returns `(used_mb, budget_mb, shared_used_mb)` or `None` on failure.
#[cfg(windows)]
fn query_dxgi_vram() -> Option<(f32, f32, f32)> {
    use windows::Win32::Graphics::Dxgi::*;
    use windows::core::Interface as _;
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let adapter: IDXGIAdapter = factory.EnumAdapters(0).ok()?;
        let adapter3: IDXGIAdapter3 = adapter.cast().ok()?;
        let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        let mut nonlocal = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter3
            .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local)
            .ok()?;
        let _ = adapter3.QueryVideoMemoryInfo(
            0,
            DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
            &mut nonlocal,
        );
        let mb = |b: u64| b as f32 / (1024.0 * 1024.0);
        Some((mb(local.CurrentUsage), mb(local.Budget), mb(nonlocal.CurrentUsage)))
    }
}

/// Collects one PDH sample and returns the aggregate 3D-engine GPU utilization
/// across all processes, clamped to 100%.
///
/// Returns `None` on the first call (PDH needs two samples to compute a rate)
/// and on API failure.  Returns `Some(0.0)` when no process is using the GPU.
#[cfg(windows)]
fn collect_gpu_util_pct(state: &mut GpuPdhState) -> Option<f32> {
    use windows::Win32::System::Performance::*;

    const PDH_MORE_DATA: u32 = 0x800007D2;

    unsafe {
        PdhCollectQueryData(state.query);

        let mut buf_size: u32 = 0;
        let mut item_count: u32 = 0;
        let probe = PdhGetFormattedCounterArrayW(
            state.counter_3d,
            PDH_FMT_DOUBLE,
            &mut buf_size,
            &mut item_count,
            None,
        );
        if probe != PDH_MORE_DATA || item_count == 0 || buf_size == 0 {
            return if item_count == 0 { Some(0.0) } else { None };
        }

        let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let buf_bytes = (buf_size as usize).max((item_count as usize).saturating_mul(item_size));
        let mut buf: Vec<u8> = vec![0u8; buf_bytes];
        let items_ptr = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let status = PdhGetFormattedCounterArrayW(
            state.counter_3d,
            PDH_FMT_DOUBLE,
            &mut buf_size,
            &mut item_count,
            Some(items_ptr),
        );
        if status != 0 {
            return None;
        }

        let items = std::slice::from_raw_parts(items_ptr, item_count as usize);
        let total: f64 = items
            .iter()
            .filter(|item| item.FmtValue.CStatus == 0)
            .map(|item| item.FmtValue.Anonymous.doubleValue)
            .sum();
        Some((total as f32).clamp(0.0, 100.0))
    }
}

// ─── Overlay components ───────────────────────────────────────────────────────

/// Marker for the root UI node of the debug overlay.
#[derive(Component)]
pub(crate) struct DebugOverlayRoot;

/// Marker for the text node inside the overlay.
#[derive(Component)]
pub(crate) struct DebugOverlayText;

/// Tracks overlay state and refresh timing.
#[derive(Resource)]
pub(crate) struct DebugOverlayState {
    pub visible: bool,
    refresh_timer: f32,
}

impl Default for DebugOverlayState {
    fn default() -> Self {
        Self {
            visible: false,
            refresh_timer: 0.0,
        }
    }
}

/// How often (seconds) the overlay text is refreshed.
const REFRESH_INTERVAL: f32 = 0.25;

/// Spawns the overlay UI nodes (hidden by default).
pub(crate) fn setup_debug_overlay(mut commands: Commands) {
    commands
        .spawn((
            DebugOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(8.0),
                right: Val::Px(8.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
            Visibility::Hidden,
            // High z-index so it renders above everything.
            ZIndex(900),
        ))
        .with_children(|parent| {
            parent.spawn((
                DebugOverlayText,
                Text::new("Debug overlay loading..."),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::srgba(0.0, 1.0, 0.4, 0.95)),
            ));
        });
}

/// Toggles overlay visibility on F3.
pub(crate) fn toggle_debug_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<DebugOverlayState>,
    mut query: Query<&mut Visibility, With<DebugOverlayRoot>>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        state.visible = !state.visible;
        for mut vis in &mut query {
            *vis = if state.visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Refreshes overlay text with current diagnostics.
pub(crate) fn refresh_debug_overlay(
    time: Res<Time>,
    mut state: ResMut<DebugOverlayState>,
    diagnostics: Res<DiagnosticsStore>,
    profile: Res<ResolvedProfile>,
    gpu_info: Option<Res<GpuInfo>>,
    gpu_metrics: Res<GpuMetrics>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    tracker: Option<Res<super::preview::ContentPreviewTracker>>,
    mut text_query: Query<&mut Text, With<DebugOverlayText>>,
) {
    if !state.visible {
        return;
    }

    state.refresh_timer += time.delta_secs();
    if state.refresh_timer < REFRESH_INTERVAL {
        return;
    }
    state.refresh_timer = 0.0;

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let frame_time_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let process_cpu = diagnostics
        .get(&SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let process_mem_gib = diagnostics
        .get(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let system_cpu = diagnostics
        .get(&SystemInformationDiagnosticsPlugin::SYSTEM_CPU_USAGE)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let system_mem_pct = diagnostics
        .get(&SystemInformationDiagnosticsPlugin::SYSTEM_MEM_USAGE)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);

    let process_mem_mb = process_mem_gib * 1024.0;

    // GPU section — static identity + live VRAM / utilization metrics.
    let gpu_section = match &gpu_info {
        Some(gpu) => {
            let driver_line = if !gpu.driver.is_empty() {
                format!("\nDriver: {}", gpu.driver)
            } else {
                String::new()
            };
            let util_line = match gpu_metrics.gpu_util_pct {
                Some(pct) => format!("\nUsage: {pct:.0}%"),
                None => String::new(),
            };
            let vram_line = if gpu_metrics.vram_budget_mb > 0.0 {
                let pct = gpu_metrics.vram_used_mb / gpu_metrics.vram_budget_mb * 100.0;
                format!(
                    "\nVRAM: {:.0}/{:.0} MB ({:.0}%)",
                    gpu_metrics.vram_used_mb, gpu_metrics.vram_budget_mb, pct
                )
            } else {
                String::new()
            };
            let shared_line = if gpu_metrics.shared_used_mb > 0.0 {
                format!("\nShared: {:.0} MB", gpu_metrics.shared_used_mb)
            } else {
                String::new()
            };
            format!(
                "\n\n--- GPU ---\nGPU: {}\nAPI: {}  Type: {}{}{}{}{}",
                gpu.adapter_name,
                gpu.backend,
                gpu.device_type,
                driver_line,
                util_line,
                vram_line,
                shared_line,
            )
        }
        None => "\n\n--- GPU ---\n(initializing...)".to_string(),
    };

    // Current window resolution.
    let resolution = primary_window
        .single()
        .ok()
        .map(|w| format!("{}×{}", w.physical_width(), w.physical_height()))
        .unwrap_or_else(|| "?×?".into());

    // Active graphics profile and GPU on/off status.
    let gpu_on_off = if profile.gpu_active { "On" } else { "Off" };
    let profile_label = profile.profile.label();

    // Preview tracker info.
    let preview_info = tracker.as_ref().map_or(String::new(), |t| {
        let frame_count = t.frame_count();
        let buffered = t.buffered_frame_count();
        let family = t
            .current_family()
            .map_or("-".into(), |f| format!("{f:?}"));
        format!(
            "\n\n--- Preview ---\nFamily: {family}\nFrames: {buffered}/{frame_count} buffered"
        )
    });

    let text = format!(
        "--- Performance ---\n\
         FPS: {fps:.0}\n\
         Frame: {frame_time_ms:.1} ms\n\
         Res: {resolution}\n\
         \n\
         --- Process ---\n\
         CPU: {process_cpu:.1}%\n\
         RAM: {process_mem_mb:.0} MB\n\
         \n\
         --- System ---\n\
         CPU: {system_cpu:.1}%\n\
         MEM: {system_mem_pct:.1}%\
         {gpu_section}\
         \n\
         --- Frontend ---\n\
         Profile: {profile_label}  GPU: {gpu_on_off}\
         {preview_info}"
    );

    for mut t in &mut text_query {
        **t = text.clone();
    }
}
