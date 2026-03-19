// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the classic isometric camera bootstrap.

use super::*;
use crate::scene::RenderLayer;
use bevy::math::{Rect, Vec2, Vec3};

/// Proves that the classic isometric model projects map-space coordinates into
/// the expected screen-space diamond layout.
#[test]
fn classic_camera_projects_world_positions_into_isometric_screen_space() {
    let model = ClassicIsometricCameraModel::default();
    let camera = GameCamera {
        bounds: Rect::from_corners(Vec2::ZERO, Vec2::new(1024.0, 1024.0)),
        ..GameCamera::default()
    };

    assert_eq!(
        model.world_to_render(Vec2::new(1.0, 0.0), &camera, RenderLayer::TerrainTiles),
        Vec3::new(24.0, 12.0, RenderLayer::TerrainTiles.z())
    );
    assert_eq!(
        model.world_to_render(Vec2::new(0.0, 1.0), &camera, RenderLayer::TerrainTiles),
        Vec3::new(-24.0, 12.0, RenderLayer::TerrainTiles.z())
    );
}

/// Proves that the screen-to-world path can invert the classic projection at
/// the center of the viewport.
#[test]
fn classic_camera_screen_to_world_round_trips_centered_positions() {
    let model = ClassicIsometricCameraModel::default();
    let camera = GameCamera {
        position: Vec2::new(5.0, 7.0),
        position_target: Vec2::new(5.0, 7.0),
        bounds: Rect::from_corners(Vec2::ZERO, Vec2::new(1024.0, 1024.0)),
        ..GameCamera::default()
    };
    let viewport = Vec2::new(1280.0, 720.0);
    let screen = model.world_to_screen(Vec2::new(5.0, 7.0), &camera, viewport);

    assert_eq!(screen, viewport / 2.0);
    assert_eq!(
        model.screen_to_world(screen, &camera, viewport),
        Vec2::new(5.0, 7.0)
    );
}
