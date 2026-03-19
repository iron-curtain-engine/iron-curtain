// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for Petroglyph `.meg` archives.
//!
//! Remastered content is commonly packaged in `.meg` containers. Unlike
//! classic Westwood `.mix` files, MEG stores real filenames on disk, so this
//! wrapper can expose stable logical names without relying on CRC heuristics.
//!
//! For a Bevy newcomer:
//! - `MegArchive` is the typed asset stored in Bevy's asset world
//! - `MegLoader` is the adapter Bevy runs when it sees a `.meg` or `.pgm`
//!   path
//! - the wrapper keeps the original bytes so later systems can reopen the
//!   clean-room parser view without re-reading the filesystem

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::meg as cnc_meg;
use thiserror::Error;

/// Errors returned while reading or parsing a `.meg` archive.
#[derive(Debug, Error)]
pub enum MegLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("MEG parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Metadata for one file stored in a parsed MEG archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegEntry {
    /// Logical filename stored directly in the archive.
    pub name: String,
    /// Absolute byte offset within the archive.
    pub offset: u64,
    /// Payload size in bytes.
    pub size: u64,
}

/// Importer-facing metadata for one physical MEG entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegStagedEntry {
    /// Physical position in the archive record table.
    pub archive_index: usize,
    /// Logical filename stored in the archive.
    pub logical_name: String,
    /// Absolute byte offset within the archive.
    pub offset: u64,
    /// Payload size in bytes.
    pub size: u64,
}

/// Extracted MEG payload plus the metadata that explains where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegStagedFile {
    /// Directory metadata for the extracted entry.
    pub entry: MegStagedEntry,
    /// Exact source payload bytes copied from the archive body.
    pub bytes: Vec<u8>,
}

/// Errors specific to importer staging on top of a parsed `.meg` archive.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MegStagingError {
    #[error("MEG parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
    #[error("archive index {archive_index} is out of range for MEG with {entry_count} entries")]
    EntryOutOfRange {
        archive_index: usize,
        entry_count: usize,
    },
    #[error("archive entry {archive_index} ({logical_name}) could not be sliced after validation")]
    EntryPayloadUnavailable {
        archive_index: usize,
        logical_name: String,
    },
}

/// Engine asset wrapper around a parsed `.meg` archive.
///
/// The wrapper snapshots entry metadata into owned Rust values for fast UI and
/// importer inspection while retaining the original archive bytes for later
/// borrowed parsing through `cnc-formats`.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct MegArchive {
    raw_bytes: Vec<u8>,
    entries: Vec<MegEntry>,
}

impl MegArchive {
    fn staged_entry_from_parser(archive_index: usize, entry: &cnc_meg::MegEntry) -> MegStagedEntry {
        MegStagedEntry {
            archive_index,
            logical_name: entry.name.clone(),
            offset: entry.offset,
            size: entry.size,
        }
    }

    /// Parses raw `.meg` bytes and caches the directory metadata needed by IC.
    ///
    /// The wrapper eagerly snapshots filenames, offsets, and sizes into owned
    /// values so Bevy systems can inspect the archive without holding a
    /// borrowed parser view open all the time.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, cnc_formats::Error> {
        let archive = cnc_meg::MegArchive::parse(&bytes)?;
        let entries = archive
            .entries()
            .iter()
            .map(|entry| MegEntry {
                name: entry.name.clone(),
                offset: entry.offset,
                size: entry.size,
            })
            .collect();

        Ok(Self {
            raw_bytes: bytes,
            entries,
        })
    }

    /// Returns the original archive bytes stored in the asset.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }

    /// Reopens the stored bytes as a borrowed `cnc-formats` archive view.
    ///
    /// This keeps the Bevy asset small while still exposing the richer
    /// filename-based lookup behavior implemented in the clean-room parser.
    pub fn archive(&self) -> Result<cnc_meg::MegArchive<'_>, cnc_formats::Error> {
        cnc_meg::MegArchive::parse(&self.raw_bytes)
    }

    /// Returns the cached directory entries in archive order.
    pub fn entries(&self) -> &[MegEntry] {
        &self.entries
    }

    /// Returns the number of directory entries in the archive.
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the bytes stored under one logical filename.
    ///
    /// MEG stores real filenames on disk, so higher-level code can use this
    /// lookup path directly without any CRC indirection.
    pub fn get(&self, filename: &str) -> Result<Option<Vec<u8>>, cnc_formats::Error> {
        let archive = self.archive()?;
        Ok(archive.get(filename).map(|bytes| bytes.to_vec()))
    }

    /// Builds importer-facing metadata for every physical archive entry.
    ///
    /// This parallels the `.mix` staging surface so later importer code can
    /// treat the two archive families consistently.
    pub fn staged_entries(&self) -> Result<Vec<MegStagedEntry>, cnc_formats::Error> {
        let archive = self.archive()?;
        Ok(archive
            .entries()
            .iter()
            .enumerate()
            .map(|(archive_index, entry)| Self::staged_entry_from_parser(archive_index, entry))
            .collect())
    }

    /// Extracts one physical archive entry for importer staging.
    pub fn extract_entry_for_staging(
        &self,
        archive_index: usize,
    ) -> Result<MegStagedFile, MegStagingError> {
        let archive = self.archive()?;
        let entry =
            archive
                .entries()
                .get(archive_index)
                .ok_or(MegStagingError::EntryOutOfRange {
                    archive_index,
                    entry_count: archive.entries().len(),
                })?;
        let bytes = archive.get_by_index(archive_index).ok_or_else(|| {
            MegStagingError::EntryPayloadUnavailable {
                archive_index,
                logical_name: entry.name.clone(),
            }
        })?;

        Ok(MegStagedFile {
            entry: Self::staged_entry_from_parser(archive_index, entry),
            bytes: bytes.to_vec(),
        })
    }

    /// Extracts every physical archive entry into importer-owned buffers.
    pub fn extract_all_for_staging(&self) -> Result<Vec<MegStagedFile>, MegStagingError> {
        let archive = self.archive()?;
        archive
            .entries()
            .iter()
            .enumerate()
            .map(|(archive_index, entry)| {
                let bytes = archive.get_by_index(archive_index).ok_or_else(|| {
                    MegStagingError::EntryPayloadUnavailable {
                        archive_index,
                        logical_name: entry.name.clone(),
                    }
                })?;

                Ok(MegStagedFile {
                    entry: Self::staged_entry_from_parser(archive_index, entry),
                    bytes: bytes.to_vec(),
                })
            })
            .collect()
    }
}

/// Bevy loader that bridges the asset system into the clean-room `.meg` parser.
#[derive(Default, TypePath)]
pub struct MegLoader;

impl AssetLoader for MegLoader {
    type Asset = MegArchive;
    type Settings = ();
    type Error = MegLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(MegArchive::parse(bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        &["meg", "pgm"]
    }
}

#[cfg(test)]
mod tests;
