// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for Westwood `.shp` sprite sheets.
//!
//! The clean-room SHP parser and encoder live in `cnc-formats`. This module
//! adapts that functionality into an engine asset that exposes common metadata
//! up front while keeping the raw bytes available for later frame decoding.
//!
//! In Bevy vocabulary, `ShpSprite` is the typed asset and `ShpLoader` is the
//! loader Bevy invokes when a `.shp` asset path is requested.

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::shp as cnc_shp;
use thiserror::Error;

/// Errors returned while reading or parsing a `.shp` sprite file.
#[derive(Debug, Error)]
pub enum ShpLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SHP parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Summary metadata for one frame in a parsed `.shp` sprite sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpFrame {
    /// Zero-based frame position inside the sprite sheet.
    pub frame_index: usize,
    /// Compression/layout format used by the encoded frame payload.
    pub format: cnc_shp::ShpFrameFormat,
    /// Absolute byte offset of this frame's data within the source file.
    pub file_offset: u32,
    /// Reference offset for delta/composite frame variants.
    pub ref_offset: u16,
    /// Reference format tag used by the source file.
    pub ref_format: u16,
    /// Encoded payload length in bytes.
    pub encoded_len: usize,
}

/// Stable summary passed from parser/importer work to the future render crate.
///
/// The render slice should not need to understand the raw SHP header layout
/// just to know how many frames exist, what size they are, or whether an
/// embedded palette came with the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShpRenderHandoff {
    /// Shared frame width from the parsed SHP header.
    pub width: u16,
    /// Shared frame height from the parsed SHP header.
    pub height: u16,
    /// Number of frames available to render or animate.
    pub frame_count: usize,
    /// Whether this SHP carried an embedded palette payload.
    pub has_embedded_palette: bool,
    /// Per-frame metadata used for render/import planning.
    pub frames: Vec<ShpFrame>,
}

/// Engine asset wrapper around a parsed `.shp` file.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct ShpSprite {
    raw_bytes: Vec<u8>,
    /// File header cached for quick metadata access.
    pub header: cnc_shp::ShpHeader,
    /// Optional embedded palette bytes carried by some SHP variants.
    pub embedded_palette: Option<Vec<u8>>,
    /// Per-frame summary metadata extracted during parsing.
    pub frames: Vec<ShpFrame>,
}

impl ShpSprite {
    /// Parses raw `.shp` bytes into the engine-facing wrapper.
    ///
    /// The wrapper keeps the original byte stream for later frame decoding
    /// while caching the header, optional embedded palette, and per-frame
    /// summaries that engine code commonly needs up front. Malformed SHP input
    /// is rejected by the underlying `cnc-formats` parser.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, cnc_formats::Error> {
        let file = cnc_shp::ShpFile::parse(&bytes)?;
        let header = file.header.clone();
        let frames = file
            .frames
            .iter()
            .enumerate()
            .map(|(frame_index, frame)| ShpFrame {
                frame_index,
                format: frame.format,
                file_offset: frame.file_offset,
                ref_offset: frame.ref_offset,
                ref_format: frame.ref_format,
                encoded_len: frame.data.len(),
            })
            .collect();
        let embedded_palette = file.embedded_palette.map(ToOwned::to_owned);

        Ok(Self {
            raw_bytes: bytes,
            header,
            embedded_palette,
            frames,
        })
    }

    /// Returns the original sprite bytes stored in the asset.
    ///
    /// The exact source bytes are preserved so later systems can reopen a
    /// parser view, hash the original asset, or persist the source payload
    /// without reaching back to the asset source.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    /// Reopens the stored bytes as a borrowed `cnc-formats` SHP view.
    ///
    /// Re-parsing here is intentional: the clean-room parser borrows from the
    /// owned byte buffer, which keeps the Bevy asset small while still exposing
    /// the fuller parser API on demand. An error here would mean the stored
    /// bytes no longer satisfy SHP invariants.
    pub fn file(&self) -> Result<cnc_shp::ShpFile<'_>, cnc_formats::Error> {
        cnc_shp::ShpFile::parse(&self.raw_bytes)
    }

    /// Decodes every frame into indexed pixel data.
    ///
    /// This delegates to `cnc-formats` so the engine wrapper never duplicates
    /// format-specific decoding logic.
    pub fn decode_frames(&self) -> Result<Vec<Vec<u8>>, cnc_formats::Error> {
        self.file()?.decode_frames()
    }

    /// Returns the metadata the render crate needs without exposing SHP internals.
    ///
    /// `G1.3` uses this as the explicit handoff from parser/importer work into
    /// the later Bevy render slice. The dimensions, frame count, and per-frame
    /// summaries are already validated by `parse()`, so render code can depend
    /// on this summary instead of re-reading binary file structures.
    pub fn render_handoff(&self) -> ShpRenderHandoff {
        ShpRenderHandoff {
            width: self.header.width,
            height: self.header.height,
            frame_count: self.frames.len(),
            has_embedded_palette: self.embedded_palette.is_some(),
            frames: self.frames.clone(),
        }
    }
}

/// Bevy loader that converts `.shp` files into [`ShpSprite`] assets.
///
/// `TypePath` gives Bevy reflection metadata for this loader at registration
/// time.
#[derive(Default, TypePath)]
pub struct ShpLoader;

impl AssetLoader for ShpLoader {
    type Asset = ShpSprite;
    type Settings = ();
    type Error = ShpLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // The loader turns opaque bytes into a richer typed asset. Later Bevy
        // systems will work with `ShpSprite`, not raw `.shp` file contents.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(ShpSprite::parse(bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["shp"]
    }
}

#[cfg(test)]
mod tests;
