// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Smoke tests for crate-level Bevy integration.

use super::*;
use bevy::asset::AssetPlugin;
use bevy::MinimalPlugins;

/// Verifies that the crate plugin can register every asset type and loader.
///
/// This is the narrowest proof that the engine-facing integration seam is
/// wired correctly before any real asset IO is attempted.
#[test]
fn ic_cnc_content_plugin_registers_successfully() {
    let mut app = App::new();
    // `MinimalPlugins` gives us the smallest useful Bevy app shell without
    // pulling in windowing, rendering, or audio.
    app.add_plugins(MinimalPlugins);
    // `AssetPlugin` installs the asset system itself: loader registration,
    // handle management, and asset storage.
    app.add_plugins(AssetPlugin::default());
    // Our plugin adds the specific legacy C&C asset types and loaders.
    app.add_plugins(IcCncContentPlugin);
    // One update tick is enough to prove the app can finish setup cleanly.
    app.update();
}
