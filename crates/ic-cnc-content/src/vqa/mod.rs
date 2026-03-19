// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for Westwood `.vqa` video files.
//!
//! `cnc-formats` owns the binary VQA parser and encoder. This module exposes a
//! Bevy asset that caches headline metadata while keeping the raw bytes around
//! for deeper parser-backed inspection by later video/import systems.
//!
//! In Bevy terms, `VqaVideo` is the value that can sit behind a
//! `Handle<VqaVideo>` and `VqaLoader` is the code that constructs it from a
//! `.vqa` asset source.

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::vqa as cnc_vqa;
use thiserror::Error;

/// Errors returned while reading or parsing a `.vqa` asset.
#[derive(Debug, Error)]
pub enum VqaLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("VQA parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Summary metadata for one chunk in a parsed `.vqa` container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VqaChunk {
    /// FourCC chunk identifier from the container stream.
    pub fourcc: [u8; 4],
    /// Chunk payload length in bytes.
    pub len: usize,
}

/// Engine asset wrapper around a parsed `.vqa` movie.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct VqaVideo {
    raw_bytes: Vec<u8>,
    /// File header cached for quick metadata access.
    pub header: cnc_vqa::VqaHeader,
    /// Number of entries in the optional frame index chunk, when present.
    pub frame_index_len: Option<usize>,
    /// Lightweight summary of the chunk directory exposed to engine code.
    pub chunks: Vec<VqaChunk>,
}

impl VqaVideo {
    /// Parses raw `.vqa` bytes into the engine-facing wrapper.
    ///
    /// The wrapper keeps the full movie byte stream but eagerly snapshots the
    /// headline metadata that engine code is likely to inspect first: the file
    /// header, optional frame-index length, and chunk directory summary.
    /// Invalid VQA containers are rejected by the underlying parser.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, cnc_formats::Error> {
        let file = cnc_vqa::VqaFile::parse(&bytes)?;
        let header = file.header.clone();
        let frame_index_len = file.frame_index.as_ref().map(Vec::len);
        let chunks = file
            .chunks
            .iter()
            .map(|chunk| VqaChunk {
                fourcc: chunk.fourcc,
                len: chunk.data.len(),
            })
            .collect();

        Ok(Self {
            raw_bytes: bytes,
            header,
            frame_index_len,
            chunks,
        })
    }

    /// Returns the original video bytes stored in the asset.
    ///
    /// Later importer or playback code may need the untouched source bytes for
    /// hashing, persistence, or reopening a deeper parser view.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    /// Reopens the stored bytes as a borrowed `cnc-formats` VQA view.
    ///
    /// Re-parsing on demand avoids embedding the full parser state inside the
    /// Bevy asset while still giving advanced callers access to the richer VQA
    /// inspection surface when they need it.
    pub fn file(&self) -> Result<cnc_vqa::VqaFile<'_>, cnc_formats::Error> {
        cnc_vqa::VqaFile::parse(&self.raw_bytes)
    }
}

/// Bevy loader that converts `.vqa` files into [`VqaVideo`] assets.
///
/// `TypePath` participates in Bevy's runtime type registration for loaders.
#[derive(Default, TypePath)]
pub struct VqaLoader;

impl AssetLoader for VqaLoader {
    type Asset = VqaVideo;
    type Settings = ();
    type Error = VqaLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // This wrapper keeps VQA loading simple: read the source bytes once,
        // parse summary metadata, and return a single top-level asset.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(VqaVideo::parse(bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["vqa"]
    }
}

#[cfg(test)]
mod tests;
