// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Camera state and world/screen conversion for `ic-render`.

use bevy::ecs::resource::Resource;
use bevy::math::{Rect, Vec2, Vec3};

use crate::scene::RenderLayer;

/// Central render-side camera state for the local viewport.
///
/// This follows the design-doc rule that camera state lives in `ic-render`,
/// not in the simulation. The sim has no concept of zoom, panning, follow
/// targets, or screen shake; those are presentation choices made entirely on
/// the client.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct GameCamera {
    /// World position currently centered in the viewport.
    pub position: Vec2,
    /// Current zoom applied to render-space coordinates.
    pub zoom: f32,
    /// Lowest allowed zoom level.
    pub zoom_min: f32,
    /// Highest allowed zoom level.
    pub zoom_max: f32,
    /// Bounds the camera should remain inside.
    pub bounds: Rect,
    /// Interpolation factor used when approaching `zoom_target`.
    pub zoom_smoothing: f32,
    /// Interpolation factor used when approaching `position_target`.
    pub pan_smoothing: f32,
    /// Desired zoom after smoothing.
    pub zoom_target: f32,
    /// Desired position after smoothing.
    pub position_target: Vec2,
    /// Edge-scroll speed in world units per second.
    pub edge_scroll_speed: f32,
    /// Keyboard pan speed in world units per second.
    pub keyboard_pan_speed: f32,
    /// Optional unit/player follow mode for observer or cinematic use.
    pub follow_target: Option<FollowTarget>,
    /// Current screen-shake state. This affects only the final presentation.
    pub shake: ScreenShake,
}

impl Default for GameCamera {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            zoom_min: 0.5,
            zoom_max: 4.0,
            bounds: Rect::from_corners(Vec2::splat(-4096.0), Vec2::splat(4096.0)),
            zoom_smoothing: 0.15,
            pan_smoothing: 0.2,
            zoom_target: 1.0,
            position_target: Vec2::ZERO,
            edge_scroll_speed: 600.0,
            keyboard_pan_speed: 800.0,
            follow_target: None,
            shake: ScreenShake::default(),
        }
    }
}

/// What, if anything, the camera is locked to.
///
/// The final engine will point these variants at real domain IDs once the sim
/// crate exists locally. For the current static-scene bootstrap, string/slot
/// identifiers are enough to preserve the intent without inventing sim types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowTarget {
    /// Follow a named render object or scripted camera anchor.
    RenderObject(String),
    /// Follow the current view associated with a player slot.
    PlayerSlot(u8),
}

/// Screen-shake state driven by explosions or other dramatic effects.
///
/// Shake is presentation-only. Even if the shake math changes, the sim result
/// is unaffected because only the camera offset changes.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenShake {
    /// Current shake amplitude in screen-space units.
    pub amplitude: f32,
    /// How quickly amplitude decays per second.
    pub decay_rate: f32,
    /// Oscillation speed used by later runtime shake systems.
    pub frequency: f32,
    /// Offset applied to the final render position this frame.
    pub offset: Vec2,
}

impl Default for ScreenShake {
    fn default() -> Self {
        Self {
            amplitude: 0.0,
            decay_rate: 12.0,
            frequency: 24.0,
            offset: Vec2::ZERO,
        }
    }
}

/// Converts screen-space cursor positions into world-space positions.
///
/// This trait exists because the engine supports more than one render model.
/// A classic isometric camera, a free-look 3D camera, or a mod-provided render
/// mode all answer "what world point is under this cursor?" differently.
pub trait ScreenToWorld {
    /// Converts a screen-space pixel position into world-space coordinates.
    fn screen_to_world(&self, screen_pos: Vec2, camera: &GameCamera, viewport: Vec2) -> Vec2;
}

/// Classic Red Alert-style isometric camera model.
///
/// The model assumes diamond-isometric tiles: moving east increases screen X
/// and screen Y, while moving south decreases screen X and increases screen Y.
/// The default tile size uses 48×24 pixels, which gives the 24/12 half-steps
/// asserted by the tests and matches the common classic-isometric convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassicIsometricCameraModel {
    /// Full tile width in screen pixels at zoom 1.0.
    pub tile_width: f32,
    /// Full tile height in screen pixels at zoom 1.0.
    pub tile_height: f32,
}

impl Default for ClassicIsometricCameraModel {
    fn default() -> Self {
        Self {
            tile_width: 48.0,
            tile_height: 24.0,
        }
    }
}

impl ClassicIsometricCameraModel {
    fn half_tile_width(&self) -> f32 {
        self.tile_width / 2.0
    }

    fn half_tile_height(&self) -> f32 {
        self.tile_height / 2.0
    }

    fn project_world(&self, world_pos: Vec2) -> Vec2 {
        Vec2::new(
            (world_pos.x - world_pos.y) * self.half_tile_width(),
            (world_pos.x + world_pos.y) * self.half_tile_height(),
        )
    }

    fn unproject_screen(&self, screen_pos: Vec2) -> Vec2 {
        let x =
            (screen_pos.x / self.half_tile_width() + screen_pos.y / self.half_tile_height()) / 2.0;
        let y =
            (screen_pos.y / self.half_tile_height() - screen_pos.x / self.half_tile_width()) / 2.0;
        Vec2::new(x, y)
    }

    /// Converts a world-space point into local render translation coordinates.
    ///
    /// "Local render" means relative to the camera center, before viewport
    /// centering. This is the form a sprite system uses when building entity
    /// translations inside the scene.
    pub fn world_to_render(
        &self,
        world_pos: Vec2,
        camera: &GameCamera,
        layer: RenderLayer,
    ) -> Vec3 {
        let screen_pos = (self.project_world(world_pos) - self.project_world(camera.position))
            * camera.zoom
            + camera.shake.offset;
        Vec3::new(screen_pos.x, screen_pos.y, layer.z())
    }

    /// Converts a world-space point into screen-space coordinates.
    ///
    /// This adds the viewport center on top of [`Self::world_to_render`] so
    /// input systems can compare cursor positions against the visible scene.
    pub fn world_to_screen(&self, world_pos: Vec2, camera: &GameCamera, viewport: Vec2) -> Vec2 {
        self.world_to_render(world_pos, camera, RenderLayer::UiOverlay)
            .truncate()
            + viewport / 2.0
    }
}

impl ScreenToWorld for ClassicIsometricCameraModel {
    fn screen_to_world(&self, screen_pos: Vec2, camera: &GameCamera, viewport: Vec2) -> Vec2 {
        let local = (screen_pos - viewport / 2.0 - camera.shake.offset) / camera.zoom;
        self.unproject_screen(local + self.project_world(camera.position))
    }
}

#[cfg(test)]
mod tests;
