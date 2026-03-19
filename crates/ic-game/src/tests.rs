// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the first runnable game-client bootstrap.

use super::*;
use ic_render::scene::{RenderLayer, Renderable};

/// Proves that the bootstrap scene produces a non-empty, renderer-ready
/// vehicle sprite through the current SHP/PAL pipeline.
#[test]
fn demo_scene_builds_visible_vehicle_content() {
    let scene = demo::build_demo_scene().expect("bootstrap scene should build");

    assert_eq!(scene.sprite.layer(), RenderLayer::Vehicles);
    assert_eq!(scene.frame.width(), 24);
    assert_eq!(scene.frame.height(), 24);
    assert!(
        scene
            .frame
            .rgba8_pixels()
            .chunks_exact(4)
            .any(|pixel| pixel[3] == 255),
        "the bootstrap sprite should contain visible opaque pixels",
    );
}

/// Proves that the bootstrap scene uses the first validated SHP frame rather
/// than requesting an out-of-range frame index.
#[test]
fn demo_scene_uses_a_valid_first_frame() {
    let scene = demo::build_demo_scene().expect("bootstrap scene should build");

    assert_eq!(scene.sprite.frame_index(), 0);
    assert_eq!(scene.sprite.sheet().frame_count(), 1);
}
