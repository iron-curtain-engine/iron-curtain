// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Static-scene metadata used to bridge parsed content into rendering.

use bevy::ecs::resource::Resource;
use bevy::math::Vec2;
use ic_cnc_content::pal::PaletteRenderHandoff;
use ic_cnc_content::shp::ShpRenderHandoff;
use thiserror::Error;

/// Canonical RA-style draw layers for the classic render path.
///
/// This mirrors the design-doc z-order so the rest of the engine can talk
/// about "vehicle layer" or "shroud layer" semantically instead of passing raw
/// floating-point Z values around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderLayer {
    TerrainTiles,
    Smudges,
    BuildingBibs,
    Shadows,
    Buildings,
    Infantry,
    Vehicles,
    AircraftShadows,
    LowAircraft,
    HighAircraft,
    Projectiles,
    Effects,
    Shroud,
    UiOverlay,
}

impl RenderLayer {
    /// Returns the Bevy-friendly Z depth associated with this render layer.
    pub fn z(self) -> f32 {
        match self {
            Self::TerrainTiles => 0.0,
            Self::Smudges => 1.0,
            Self::BuildingBibs => 2.0,
            Self::Shadows => 3.0,
            Self::Buildings => 4.0,
            Self::Infantry => 5.0,
            Self::Vehicles => 6.0,
            Self::AircraftShadows => 7.0,
            Self::LowAircraft => 8.0,
            Self::HighAircraft => 9.0,
            Self::Projectiles => 10.0,
            Self::Effects => 11.0,
            Self::Shroud => 12.0,
            Self::UiOverlay => 13.0,
        }
    }
}

/// Palette source prepared for renderer consumption.
///
/// `ic-cnc-content` already validated and expanded the palette. The render
/// crate just preserves that ready-to-use data in a shape the upcoming sprite
/// pipeline can consume without knowing how `.pal` files work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteTextureSource {
    source_bytes: usize,
    colors: [[u8; 3]; 256],
}

impl PaletteTextureSource {
    /// Converts parser-side palette handoff data into a render-side source.
    pub fn from_handoff(handoff: PaletteRenderHandoff) -> Self {
        Self {
            source_bytes: handoff.source_bytes,
            colors: handoff.colors,
        }
    }

    /// Returns the number of colors made available to the renderer.
    pub fn color_count(&self) -> usize {
        self.colors.len()
    }

    /// Returns the expanded 8-bit RGB palette table.
    pub fn colors(&self) -> &[[u8; 3]; 256] {
        &self.colors
    }

    /// Returns the original validated source payload size in bytes.
    pub fn source_bytes(&self) -> usize {
        self.source_bytes
    }
}

/// Sprite-sheet metadata consumed by the render crate.
///
/// The render layer uses these fields to validate frame access and later build
/// atlas/material data. It intentionally does not re-parse SHP bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSheetSource {
    handoff: ShpRenderHandoff,
}

impl SpriteSheetSource {
    /// Converts parser-side SHP handoff data into a render-side source.
    pub fn from_handoff(handoff: ShpRenderHandoff) -> Self {
        Self { handoff }
    }

    /// Returns the number of frames available in the source sheet.
    pub fn frame_count(&self) -> usize {
        self.handoff.frame_count
    }

    /// Returns the shared frame width in pixels.
    pub fn width(&self) -> u16 {
        self.handoff.width
    }

    /// Returns the shared frame height in pixels.
    pub fn height(&self) -> u16 {
        self.handoff.height
    }

    /// Returns whether the source SHP included an embedded palette.
    pub fn has_embedded_palette(&self) -> bool {
        self.handoff.has_embedded_palette
    }
}

/// Validation errors raised while building a static render scene.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SceneValidationError {
    #[error("requested SHP frame {requested} but sheet only has {frame_count} frames")]
    FrameOutOfRange {
        requested: usize,
        frame_count: usize,
    },
}

/// Trait implemented by things the render crate can place into a scene.
///
/// This is the first local step toward the `Renderable` seam described in the
/// design docs. The current static-scene slice only needs layering and world
/// position; richer runtime rendering hooks will be added when entity-backed
/// rendering arrives.
pub trait Renderable {
    /// Which canonical draw layer this object belongs to.
    fn layer(&self) -> RenderLayer;
    /// Where this object sits in world-space map coordinates.
    fn world_position(&self) -> Vec2;
}

/// One sprite entry inside a static render scene.
///
/// This descriptor is intentionally small and validation-focused. It says
/// which frame to draw, which palette to apply, and which layer/z-order to use
/// without yet committing to a final GPU material or Bevy entity layout.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticRenderSprite {
    sheet: SpriteSheetSource,
    frame_index: usize,
    palette: PaletteTextureSource,
    layer: RenderLayer,
    world_position: Vec2,
}

impl StaticRenderSprite {
    /// Builds one validated sprite descriptor for a future render scene.
    pub fn new(
        sheet: SpriteSheetSource,
        frame_index: usize,
        palette: PaletteRenderHandoff,
        layer: RenderLayer,
    ) -> Result<Self, SceneValidationError> {
        if frame_index >= sheet.frame_count() {
            return Err(SceneValidationError::FrameOutOfRange {
                requested: frame_index,
                frame_count: sheet.frame_count(),
            });
        }

        Ok(Self {
            sheet,
            frame_index,
            palette: PaletteTextureSource::from_handoff(palette),
            layer,
            world_position: Vec2::ZERO,
        })
    }

    /// Assigns the world-space position for this static sprite.
    pub fn with_world_position(mut self, world_position: Vec2) -> Self {
        self.world_position = world_position;
        self
    }

    /// Returns the validated frame index to draw.
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// Returns the sprite sheet metadata source.
    pub fn sheet(&self) -> &SpriteSheetSource {
        &self.sheet
    }

    /// Returns the palette source applied to this sprite.
    pub fn palette(&self) -> &PaletteTextureSource {
        &self.palette
    }
}

impl Renderable for StaticRenderSprite {
    fn layer(&self) -> RenderLayer {
        self.layer
    }

    fn world_position(&self) -> Vec2 {
        self.world_position
    }
}

/// Current static-scene resource stored in the Bevy app.
///
/// This resource is deliberately simple: it gives the app somewhere to place
/// validated scene descriptors during the `G2` bootstrap before a full map
/// loader or sim-driven render snapshot exists.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct StaticRenderScene {
    sprites: Vec<StaticRenderSprite>,
}

impl StaticRenderScene {
    /// Returns all validated sprites currently staged for drawing.
    pub fn sprites(&self) -> &[StaticRenderSprite] {
        &self.sprites
    }

    /// Adds a validated sprite to the current static scene.
    pub fn push_sprite(&mut self, sprite: StaticRenderSprite) {
        self.sprites.push(sprite);
    }
}

#[cfg(test)]
mod tests;
