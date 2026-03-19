// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for Westwood `.mix` archives.
//!
//! `cnc-formats` owns the binary archive parser. This module adapts it into an
//! engine asset that caches archive metadata for quick inspection while still
//! retaining the original bytes for borrowed re-parsing and file extraction.
//!
//! For a Bevy newcomer: `MixArchive` is the asset other systems will request
//! from Bevy, while `MixLoader` is the adapter Bevy runs to construct that
//! asset from a `.mix` file.

use std::collections::BTreeMap;

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::mix as cnc_mix;
use thiserror::Error;

/// Errors returned while reading or parsing a `.mix` archive.
#[derive(Debug, Error)]
pub enum MixLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("MIX parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Metadata for a single file entry stored in a parsed `.mix` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixEntry {
    /// Westwood CRC identifier used in the MIX directory table.
    pub crc: cnc_mix::MixCrc,
    /// Payload byte offset relative to the archive body.
    pub offset: u32,
    /// Payload size in bytes.
    pub size: u32,
}

/// Engine asset wrapper around a parsed Westwood `.mix` archive.
///
/// The wrapper snapshots directory information into owned Rust types for easy
/// access from Bevy systems while preserving the raw bytes for later borrowed
/// parsing through `cnc-formats`.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct MixArchive {
    raw_bytes: Vec<u8>,
    entries: Vec<MixEntry>,
    embedded_names: BTreeMap<cnc_mix::MixCrc, String>,
}

impl MixArchive {
    /// Parses raw `.mix` bytes and caches the directory metadata needed by IC.
    ///
    /// The wrapper eagerly snapshots entry metadata into owned Rust values so
    /// later Bevy systems can inspect archive contents without holding a
    /// borrowed parser view open all the time. Malformed or truncated archives
    /// are rejected with the underlying `cnc-formats` parse error.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, cnc_formats::Error> {
        let archive = cnc_mix::MixArchive::parse(&bytes)?;
        let entries = archive
            .entries()
            .iter()
            .map(|entry| MixEntry {
                crc: entry.crc,
                offset: entry.offset,
                size: entry.size,
            })
            .collect();
        let embedded_names = archive.embedded_names().into_iter().collect();

        Ok(Self {
            raw_bytes: bytes,
            entries,
            embedded_names,
        })
    }

    /// Returns the original archive bytes stored in the asset.
    ///
    /// Keeping the exact source bytes matters because later systems may want to
    /// re-open the clean-room parser view, hash the original asset, or extract
    /// files without touching the filesystem again.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    /// Reopens the stored bytes as a borrowed `cnc-formats` archive view.
    ///
    /// This keeps the Bevy asset owned and serializable while still exposing
    /// the richer lookup surface implemented in the clean-room parser.
    pub fn archive(&self) -> Result<cnc_mix::MixArchive<'_>, cnc_formats::Error> {
        cnc_mix::MixArchive::parse(&self.raw_bytes)
    }

    /// Returns the cached directory entries in archive order.
    ///
    /// This is the fast metadata-only path for callers that need to enumerate
    /// contents without reopening a parser view or copying any payload bytes.
    pub fn entries(&self) -> &[MixEntry] {
        &self.entries
    }

    /// Returns the number of directory entries in the archive.
    ///
    /// This is a convenience summary for UI/import code that only needs a
    /// count, not the full directory table.
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns embedded long filenames keyed by their CRC identifiers.
    ///
    /// MIX archives can carry a filename table in addition to the CRC-based
    /// directory entries. The wrapper exposes that table directly so higher
    /// layers do not need to rediscover this Westwood-specific detail.
    pub fn embedded_names(&self) -> &BTreeMap<cnc_mix::MixCrc, String> {
        &self.embedded_names
    }

    /// Looks up and copies a payload by filename using `cnc-formats` rules.
    ///
    /// `Ok(None)` means the archive parsed successfully but does not contain
    /// the requested file. `Err(_)` means reopening the parser view failed,
    /// which would indicate corrupt stored bytes or an internal wrapper bug.
    pub fn get(&self, filename: &str) -> Result<Option<Vec<u8>>, cnc_formats::Error> {
        Ok(self.archive()?.get(filename).map(ToOwned::to_owned))
    }
}

/// Bevy loader that converts `.mix` files into [`MixArchive`] assets.
///
/// Bevy requires a runtime-identifiable loader type, which is why this struct
/// derives `TypePath`.
#[derive(Default, TypePath)]
pub struct MixLoader;

impl AssetLoader for MixLoader {
    type Asset = MixArchive;
    type Settings = ();
    type Error = MixLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // We keep the whole archive byte buffer because later lookups borrow
        // from it. The wrapper currently returns one top-level asset and does
        // not register labeled sub-assets through `LoadContext`.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(MixArchive::parse(bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["mix"]
    }
}

#[cfg(test)]
mod tests;
