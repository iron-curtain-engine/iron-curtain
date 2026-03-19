// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Smoke tests for crate-level Bevy integration.

use super::*;
use bevy::app::App;
use bevy::MinimalPlugins;

/// Verifies that the render plugin can register its core resources without
/// needing the rest of the game runtime to exist yet.
#[test]
fn ic_render_plugin_registers_successfully() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(IcRenderPlugin);
    app.update();

    assert!(app.world().contains_resource::<camera::GameCamera>());
}
