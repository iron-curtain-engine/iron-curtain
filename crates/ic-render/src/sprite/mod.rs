// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bootstrap sprite conversion helpers for the first visible `G2` slice.
//!
//! The canonical long-term plan is palette-aware GPU rendering. This module is
//! a narrower bridge used to get a real window showing C&C-family content
//! sooner: it expands one decoded indexed-color frame into RGBA bytes that a
//! temporary startup scene can hand to Bevy as a plain texture.

use thiserror::Error;

use crate::scene::PaletteTextureSource;

/// Errors raised while building bootstrap sprite textures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SpriteBootstrapError {
    #[error("decoded frame expected {expected} pixels but received {actual}")]
    PixelCountMismatch { expected: usize, actual: usize },
}

/// One decoded palette-indexed frame prepared for bootstrap rendering.
///
/// The input pixels are already decoded from SHP compression into one palette
/// index per pixel. This type exists only to validate width/height agreement
/// and to produce an RGBA buffer for Bevy's temporary startup texture path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSpriteFrame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl IndexedSpriteFrame {
    /// Creates one validated indexed-color frame.
    ///
    /// This catches width/height mismatches immediately so the startup scene
    /// cannot quietly build a corrupted texture from malformed decoded data.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, SpriteBootstrapError> {
        let expected = width as usize * height as usize;
        if pixels.len() != expected {
            return Err(SpriteBootstrapError::PixelCountMismatch {
                expected,
                actual: pixels.len(),
            });
        }

        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Expands the indexed frame into an RGBA8888 image buffer.
    ///
    /// This is deliberately a bootstrap path for the first visible `G2`
    /// milestone. Palette index `0` is treated as transparent background, while
    /// all other entries expand to opaque pixels using the render-side palette
    /// table.
    pub fn to_rgba(&self, palette: &PaletteTextureSource) -> RgbaSpriteFrame {
        let mut rgba8 = Vec::with_capacity(self.pixels.len() * 4);

        for &index in &self.pixels {
            let [r, g, b] = palette.colors()[index as usize];
            let alpha = if index == 0 { 0 } else { 255 };
            rgba8.extend_from_slice(&[r, g, b, alpha]);
        }

        RgbaSpriteFrame {
            width: self.width,
            height: self.height,
            rgba8,
        }
    }
}

/// RGBA8888 sprite image generated from one indexed frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaSpriteFrame {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

impl RgbaSpriteFrame {
    /// Creates one validated RGBA8888 frame.
    ///
    /// Some content-lab previewers, such as waveform, LUT, and text-driven
    /// diagnostic images, do not start from palette-indexed pixels. This
    /// constructor gives those tooling paths the same width/height validation
    /// that indexed bootstrap frames already receive.
    pub fn from_rgba(
        width: u32,
        height: u32,
        rgba8: Vec<u8>,
    ) -> Result<Self, SpriteBootstrapError> {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if rgba8.len() != expected {
            return Err(SpriteBootstrapError::PixelCountMismatch {
                expected,
                actual: rgba8.len(),
            });
        }

        Ok(Self {
            width,
            height,
            rgba8,
        })
    }

    /// Width of the expanded sprite frame in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height of the expanded sprite frame in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// RGBA8888 pixel buffer in row-major order.
    pub fn rgba8_pixels(&self) -> &[u8] {
        &self.rgba8
    }
}

#[cfg(test)]
mod tests;
