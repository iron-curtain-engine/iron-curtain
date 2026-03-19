// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the bootstrap palette expansion path.

use super::*;
use crate::scene::PaletteTextureSource;
use ic_cnc_content::pal::PaletteRenderHandoff;

fn demo_palette() -> PaletteTextureSource {
    let mut colors = [[0u8; 3]; 256];
    colors[1] = [255, 0, 0];
    colors[2] = [0, 255, 0];
    colors[3] = [0, 0, 255];

    PaletteTextureSource::from_handoff(PaletteRenderHandoff {
        color_count: 256,
        source_bytes: 768,
        colors,
    })
}

/// Proves that the bootstrap path expands palette indices into exact RGBA
/// pixels while treating palette index 0 as transparent background.
#[test]
fn indexed_frame_expands_into_rgba_pixels() {
    let frame = IndexedSpriteFrame::new(2, 2, vec![0, 1, 2, 3]).expect("dimensions should match");
    let rgba = frame.to_rgba(&demo_palette());

    assert_eq!(
        rgba.rgba8_pixels(),
        &[0, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,]
    );
}

/// Proves that malformed decoded frame buffers are rejected before the startup
/// scene can build an invalid texture.
#[test]
fn indexed_frame_rejects_pixel_count_mismatch() {
    let error = IndexedSpriteFrame::new(3, 2, vec![1, 2, 3]).expect_err("pixel count should fail");

    assert_eq!(
        error,
        SpriteBootstrapError::PixelCountMismatch {
            expected: 6,
            actual: 3,
        }
    );
}
