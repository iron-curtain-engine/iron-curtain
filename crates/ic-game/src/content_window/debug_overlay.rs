// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! F3 debug overlay — on-screen telemetry for CPU, RAM, FPS, frame time, and
//! content-lab diagnostics.
//!
//! Toggle visibility with **F3**. The overlay sits in the top-right corner and
//! updates every 250 ms (4 Hz) to avoid impacting the metrics it reports.

use bevy::diagnostic::{
    DiagnosticsStore, FrameTimeDiagnosticsPlugin, SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;

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
         \n\
         --- Process ---\n\
         CPU: {process_cpu:.1}%\n\
         RAM: {process_mem_mb:.0} MB\n\
         \n\
         --- System ---\n\
         CPU: {system_cpu:.1}%\n\
         MEM: {system_mem_pct:.1}%\
         {preview_info}"
    );

    for mut t in &mut text_query {
        **t = text.clone();
    }
}
