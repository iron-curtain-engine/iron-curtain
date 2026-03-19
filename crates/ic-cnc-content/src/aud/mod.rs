// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for Westwood `.aud` clips.
//!
//! `cnc-formats` owns the binary parser and codec details. This module keeps
//! the engine integration concerns local to Iron Curtain: a Bevy `Asset`
//! wrapper, a loader, and cached metadata that higher-level audio systems can
//! inspect without reopening files from disk.
//!
//! In Bevy terms, `AudAudio` is the asset type and `AudLoader` is the adapter
//! Bevy calls to turn raw `.aud` bytes into that typed asset.

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::aud as cnc_aud;
use thiserror::Error;

/// Errors returned while loading an `.aud` file through Bevy.
///
/// The wrapper distinguishes transport failures from parser validation
/// failures so callers can tell whether the asset path or the asset data is
/// broken.
#[derive(Debug, Error)]
pub enum AudLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("AUD parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Bevy asset that stores a parsed `.aud` clip plus its original byte stream.
///
/// The raw bytes stay attached to the asset so later systems can reopen a
/// borrowed `cnc_formats::aud::AudFile<'_>` view without duplicating parser
/// state or re-reading the source file.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct AudAudio {
    raw_bytes: Vec<u8>,
    /// Header metadata cached at load time for quick inspection.
    pub header: cnc_aud::AudHeader,
}

impl AudAudio {
    /// Parses raw `.aud` bytes into the engine-facing asset wrapper.
    ///
    /// The wrapper caches the file header for quick inspection while keeping
    /// the original bytes available for later parser-backed reopening.
    /// Malformed `.aud` payloads are rejected by `cnc-formats`.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, cnc_formats::Error> {
        let file = cnc_aud::AudFile::parse(&bytes)?;
        let header = file.header.clone();
        Ok(Self {
            raw_bytes: bytes,
            header,
        })
    }

    /// Returns the exact source bytes stored in the asset.
    ///
    /// Audio/import systems may need the untouched byte stream for hashing,
    /// persistence, or reopening the parser view without another asset-source
    /// read.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    /// Reopens the stored bytes as a borrowed `cnc-formats` parser view.
    ///
    /// Re-parsing here is intentional: the clean-room parser borrows from
    /// `self.raw_bytes`, which keeps the asset representation small and avoids
    /// duplicating codec state inside Bevy.
    pub fn file(&self) -> Result<cnc_aud::AudFile<'_>, cnc_formats::Error> {
        cnc_aud::AudFile::parse(&self.raw_bytes)
    }
}

/// Bevy loader that bridges the asset system into the clean-room `.aud` parser.
///
/// The derived `TypePath` gives Bevy a stable runtime identity for the loader
/// type when it is registered on the app.
#[derive(Default, TypePath)]
pub struct AudLoader;

impl AssetLoader for AudLoader {
    type Asset = AudAudio;
    type Settings = ();
    type Error = AudLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // Bevy passes a generic byte `Reader` so loaders work with any asset
        // source, not just local files. `LoadContext` would be used for labeled
        // sub-assets, but this loader only returns one top-level asset.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(AudAudio::parse(bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["aud"]
    }
}

#[cfg(test)]
mod tests;
