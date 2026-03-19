// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ic-render — Iron Curtain render-side viewport and scene glue
//!
//! This crate will own the Bevy-facing render layer for Iron Curtain.
//! The first local slice targets `G2`: camera/resource bootstrap plus the
//! static-scene data needed to turn parsed C&C assets into a faithful RA-style
//! viewport later.

use bevy::app::{App, Plugin};

pub mod camera;
pub mod scene;
pub mod sprite;

/// Registers the render-side resources that later gameplay/editor code will use.
///
/// For a Bevy newcomer: a `Plugin` is a reusable setup unit. When another
/// crate calls `app.add_plugins(IcRenderPlugin)`, Bevy runs `build()` once and
/// installs this crate's resources and systems into the app.
///
/// This first `G2` slice intentionally stays small: it provides the camera
/// resource and static-scene validation layer needed for a non-sim-backed
/// render bootstrap. Full sprite/material/view systems come in later passes.
pub struct IcRenderPlugin;

impl Plugin for IcRenderPlugin {
    fn build(&self, app: &mut App) {
        // `init_resource::<T>()` inserts `T::default()` if the app does not
        // already hold one. That gives later systems a shared camera state
        // without requiring the caller to manually construct it first.
        app.init_resource::<camera::GameCamera>()
            .init_resource::<scene::StaticRenderScene>();
    }
}

#[cfg(test)]
mod tests;
