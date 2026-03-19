// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for Westwood `.pal` files.
//!
//! Palette decoding logic lives in `cnc-formats`. This module exposes a Bevy
//! asset and loader so render code can request palettes through the engine's
//! asset pipeline without depending directly on the parser crate.
//!
//! In Bevy, `Palette` is the stored typed asset and `PalLoader` is the code
//! that builds that asset from source bytes.

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::pal as cnc_pal;
use thiserror::Error;

/// Errors returned while reading or parsing a `.pal` asset.
#[derive(Debug, Error)]
pub enum PalLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PAL parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Engine asset wrapper around a parsed Westwood palette table.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    inner: cnc_pal::Palette,
}

impl Palette {
    /// Parses raw palette bytes into the engine-facing wrapper.
    ///
    /// The wrapper keeps renderer-facing code out of the parser crate while
    /// still rejecting malformed palette data through the underlying
    /// `cnc-formats` validation rules.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, cnc_formats::Error> {
        Ok(Self {
            inner: cnc_pal::Palette::parse(&bytes)?,
        })
    }

    /// Returns the underlying `cnc-formats` palette for advanced callers.
    ///
    /// Most engine code should use the higher-level wrapper methods, but this
    /// escape hatch keeps specialized callers from needing a second parse path.
    pub fn inner(&self) -> &cnc_pal::Palette {
        &self.inner
    }

    /// Expands the palette into 8-bit RGB triples for renderer-friendly use.
    ///
    /// Westwood palettes are indexed assets. The renderer usually wants a
    /// direct 256-entry RGB table instead of parser-specific palette internals.
    pub fn to_rgb8_array(&self) -> [[u8; 3]; cnc_pal::PALETTE_SIZE] {
        self.inner.to_rgb8_array()
    }
}

/// Bevy loader that converts `.pal` files into [`Palette`] assets.
///
/// `TypePath` is part of Bevy's reflection and registration machinery: it lets
/// the engine refer to the loader type at runtime.
#[derive(Default, TypePath)]
pub struct PalLoader;

impl AssetLoader for PalLoader {
    type Asset = Palette;
    type Settings = ();
    type Error = PalLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // The asset system hands us an abstract byte stream instead of file IO
        // primitives. That keeps the loader reusable across Bevy asset sources.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(Palette::parse(bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["pal"]
    }
}

#[cfg(test)]
mod tests;
