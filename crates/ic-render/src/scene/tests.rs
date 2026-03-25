// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for parser-to-render handoff validation.

use super::*;
use ic_cnc_content::pal::PaletteRenderHandoff;
use ic_cnc_content::shp::{ShpFrame, ShpRenderHandoff};

fn test_palette() -> PaletteRenderHandoff {
    PaletteRenderHandoff {
        color_count: 256,
        source_bytes: 768,
        colors: [[0, 0, 0]; 256],
    }
}

fn test_sheet() -> ShpRenderHandoff {
    ShpRenderHandoff {
        width: 24,
        height: 24,
        frame_count: 2,
        has_embedded_palette: false,
        frames: vec![
            ShpFrame {
                frame_index: 0,
                format: ic_cnc_content::cnc_formats::shp::ShpFrameFormat::Lcw,
                file_offset: 0,
                ref_offset: 0,
                ref_format: 0,
                encoded_len: 64,
            },
            ShpFrame {
                frame_index: 1,
                format: ic_cnc_content::cnc_formats::shp::ShpFrameFormat::Lcw,
                file_offset: 0,
                ref_offset: 0,
                ref_format: 0,
                encoded_len: 64,
            },
        ],
    }
}

/// Proves that the render crate accepts the normalized palette handoff from
/// `ic-cnc-content` without re-parsing palette files itself.
#[test]
fn palette_source_preserves_renderer_ready_color_table() {
    let source = PaletteTextureSource::from_handoff(test_palette());

    assert_eq!(source.color_count(), 256);
    assert_eq!(source.colors()[0], [0, 0, 0]);
}

/// Proves that static scene sprites validate frame indices against the SHP
/// handoff metadata before the renderer ever tries to draw them.
#[test]
fn static_scene_sprite_rejects_frame_indices_outside_sheet_bounds() {
    let error = StaticRenderSprite::new(
        SpriteSheetSource::from_handoff(test_sheet()),
        9,
        test_palette(),
        RenderLayer::Vehicles,
    )
    .expect_err("frame index outside sheet bounds should fail");

    assert_eq!(
        error,
        SceneValidationError::FrameOutOfRange {
            requested: 9,
            frame_count: 2,
        }
    );
}

/// Proves that the canonical RA draw order stays encoded in the layer enum the
/// render crate exposes to the rest of the engine.
#[test]
fn render_layer_z_values_follow_ra_draw_order() {
    assert!(RenderLayer::TerrainTiles.z() < RenderLayer::Buildings.z());
    assert!(RenderLayer::Buildings.z() < RenderLayer::Vehicles.z());
    assert!(RenderLayer::Vehicles.z() < RenderLayer::UiOverlay.z());
}
