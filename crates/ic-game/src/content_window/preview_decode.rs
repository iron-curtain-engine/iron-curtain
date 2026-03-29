// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Pure preview preparation for the content lab.
//!
//! This module contains the testable half of the resource lab:
//! - read the selected entry bytes, whether they live on disk or in an archive
//! - decode those bytes into visual frames, audio payloads, or text excerpts
//! - return one owned preview object the Bevy runtime can display and control
//!
//! Keeping this logic Bevy-free matters for two reasons. First, it lets the
//! tests prove format coverage without opening a window. Second, it keeps the
//! Bevy-facing runtime code focused on presentation and playback state rather
//! than mixing UI concerns with binary decoding rules.

use std::cell::RefCell;
use std::fs;
use std::io::Cursor;
use std::sync::Arc;

use hound::WavReader;
use ic_cnc_content::cnc_formats;
use ic_cnc_content::meg::MegStagingError;
use ic_cnc_content::mix::MixStagingError;
use ic_cnc_content::pal::Palette;
use ic_cnc_content::shp::ShpSprite;
use ic_render::scene::{
    PaletteTextureSource, RenderLayer, SceneValidationError, SpriteSheetSource, StaticRenderSprite,
};
use ic_render::sprite::{IndexedSpriteFrame, RgbaSpriteFrame, SpriteBootstrapError};
use thiserror::Error;

use super::catalog::{ContentCatalog, ContentCatalogEntry, ContentEntryLocation, ContentFamily};

const PALETTE_PREVIEW_COLUMNS: u32 = 16;
const PALETTE_PREVIEW_ROWS: u32 = 16;
const DEFAULT_PALETTE_NAMES: &[&str] = &["TEMPERAT.PAL", "SNOW.PAL", "INTERIOR.PAL", "EGOPAL.PAL"];
const SNOW_PALETTE_NAMES: &[&str] = &["SNOW.PAL", "TEMPERAT.PAL", "INTERIOR.PAL", "EGOPAL.PAL"];
const INTERIOR_PALETTE_NAMES: &[&str] = &["INTERIOR.PAL", "TEMPERAT.PAL", "SNOW.PAL", "EGOPAL.PAL"];
const EGO_PALETTE_NAMES: &[&str] = &["EGOPAL.PAL", "TEMPERAT.PAL", "SNOW.PAL", "INTERIOR.PAL"];
/// Standalone `.PAL` fallback for score/multiplayer entries.  The primary
/// palette source for score-screen SHPs is the PCX background (see
/// [`SCORE_PCX_NAMES`]).  These `.PAL` names are tried only when no PCX
/// palette is available.
const SCORE_PALETTE_NAMES: &[&str] = &[
    "MULTSCOR.PAL",
    "SCORE.PAL",
    "SCORPAL1.PAL",
    "TEMPERAT.PAL",
    "SNOW.PAL",
];
/// Palettes for mission-selection map WSAs (MSA*.WSA / MSS*.WSA).
const MAP_PALETTE_NAMES: &[&str] = &[
    "MAP.PAL",
    "MAP1.PAL",
    "TEMPERAT.PAL",
    "SNOW.PAL",
];
/// PCX backgrounds whose embedded palettes are used by score/credits SHPs.
/// RA1's score screen loads ALIBACKH.PCX (Allied) or SOVBACKH.PCX (Soviet)
/// via `Load_Title_Screen` which also sets the active palette.  These PCX
/// files live in CONQUER.MIX alongside the SHPs that depend on them.
const SCORE_PCX_NAMES: &[&str] = &["ALIBACKH.PCX", "SOVBACKH.PCX"];
const DEFAULT_ANIMATION_FPS: f32 = 12.0;
const WAVEFORM_WIDTH: u32 = 512;
const WAVEFORM_HEIGHT: u32 = 160;
const TEXT_PREVIEW_MAX_LINES: usize = 18;
const TEXT_PREVIEW_MAX_CHARS: usize = 1_600;
const TMP_PREVIEW_MAX_TILES: usize = 16;
const TMP_PREVIEW_COLUMNS: usize = 4;

thread_local! {
    static THREAD_LOCAL_CACHE: RefCell<Option<super::ArchivePreloadCache>> = const { RefCell::new(None) };
    static THREAD_LOCAL_HANDLES: RefCell<Option<super::ArchiveHandleCache>> = const { RefCell::new(None) };
}

/// Static preview surfaces the content lab can offer for one catalog entry.
///
/// The lab uses this before decoding so navigation can answer a simpler
/// question than "does the parser succeed?": "if I open this resource, should
/// I expect pixels, audio, text, or some combination of those?" That makes the
/// entry list and startup selection logic deterministic without eagerly
/// decoding every file in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PreviewCapabilities {
    visual: bool,
    audio: bool,
    text: bool,
}

impl PreviewCapabilities {
    /// Returns `true` when the entry can render to pixels in the gallery.
    pub(crate) fn visual(self) -> bool {
        self.visual
    }

    /// Returns `true` when the entry has at least one validation surface.
    pub(crate) fn any(self) -> bool {
        self.visual || self.audio || self.text
    }

    /// Compact `V`/`A`/`T` badge string shown in the entry list.
    pub(crate) fn badge_string(self) -> String {
        [
            if self.visual { 'V' } else { '-' },
            if self.audio { 'A' } else { '-' },
            if self.text { 'T' } else { '-' },
        ]
        .into_iter()
        .collect()
    }

    /// Human-readable surface summary for selected-entry details.
    pub(crate) fn surface_summary(self) -> String {
        let mut parts = Vec::new();
        if self.visual {
            parts.push("visual");
        }
        if self.audio {
            parts.push("audio");
        }
        if self.text {
            parts.push("text");
        }

        if parts.is_empty() {
            "no direct validation surface".into()
        } else {
            parts.join(" + ")
        }
    }
}

/// Errors returned while reading or decoding a selected content preview.
#[derive(Debug, Error)]
pub enum PreviewLoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("content format error: {0}")]
    Formats(#[from] ic_cnc_content::cnc_formats::Error),
    #[error("render scene validation error: {0}")]
    Scene(#[from] SceneValidationError),
    #[error("bootstrap sprite conversion error: {0}")]
    Sprite(#[from] SpriteBootstrapError),
    #[error("MIX staging error: {0}")]
    MixStaging(#[from] MixStagingError),
    #[error("MEG staging error: {0}")]
    MegStaging(#[from] MegStagingError),
    #[error("WAV decode error: {0}")]
    Wav(#[from] hound::Error),
    #[error("no usable palette could be found for visual resource {entry_path}")]
    MissingPalette { entry_path: String },
    #[error("visual resource {entry_path} decoded zero frames")]
    EmptyVisual { entry_path: String },
}

/// Fully prepared preview data for one selected content entry.
///
/// A single resource can produce several validation surfaces at once. For
/// example, a VQA file can have animated frames, extracted audio, and text
/// metadata. This shape keeps those surfaces together so the runtime can build
/// one coherent validation panel.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedContentPreview {
    label: String,
    details: Vec<String>,
    visual: Option<VisualPreview>,
    audio: Option<AudioPreview>,
    text_body: Option<String>,
}

impl PreparedContentPreview {
    /// Human-readable summary shown in the text side of the content lab.
    pub(crate) fn summary_text(&self) -> String {
        let mut lines = vec![self.label.clone()];
        lines.extend(self.details.iter().cloned());
        if let Some(text_body) = &self.text_body {
            lines.push(String::new());
            lines.push("Text excerpt:".into());
            lines.extend(text_body.lines().map(|line| line.to_string()));
        }
        lines.join("\n")
    }

    /// Visual preview data, when the selected resource can render to pixels.
    pub(crate) fn visual(&self) -> Option<&VisualPreview> {
        self.visual.as_ref()
    }

    /// Audio preview data, when the selected resource can play or export audio.
    pub(crate) fn audio(&self) -> Option<&AudioPreview> {
        self.audio.as_ref()
    }

    /// Text excerpt, when the selected resource is best validated as text.
    pub(crate) fn text_body(&self) -> Option<&str> {
        self.text_body.as_deref()
    }

    /// First visual frame for tests that only need one visible image proof.
    #[cfg(test)]
    pub(crate) fn frame(&self) -> &RgbaSpriteFrame {
        self.visual
            .as_ref()
            .and_then(|visual| visual.frames.first())
            .expect("preview should expose at least one visual frame")
    }

    /// Number of visual frames prepared for the preview surface.
    #[cfg(test)]
    pub(crate) fn frame_count(&self) -> Option<usize> {
        self.visual.as_ref().map(|visual| visual.frames.len())
    }

    /// Decoded PCM samples ready for runtime playback.
    #[cfg(test)]
    pub(crate) fn audio_pcm_samples(&self) -> Option<&[i16]> {
        self.audio.as_ref().map(|audio| audio.pcm_samples.as_ref())
    }
}

/// Animated or static visual preview prepared for the content lab.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VisualPreview {
    frames: Vec<RgbaSpriteFrame>,
    frame_duration_seconds: Option<f32>,
}

impl VisualPreview {
    /// All prepared RGBA frames in playback order.
    pub(crate) fn frames(&self) -> &[RgbaSpriteFrame] {
        &self.frames
    }

    /// Optional animation cadence. `None` means the preview is static.
    pub(crate) fn frame_duration_seconds(&self) -> Option<f32> {
        self.frame_duration_seconds
    }
}

/// Audio preview prepared for playback and waveform visualization.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AudioPreview {
    pub(crate) pcm_samples: Arc<[i16]>,
    duration_seconds: f32,
    sample_rate: u32,
    channels: u16,
}

impl AudioPreview {
    /// Playback duration in seconds.
    pub(crate) fn duration_seconds(&self) -> f32 {
        self.duration_seconds
    }

    /// Decoded PCM sample rate in Hz.
    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of audio channels.
    pub(crate) fn channels(&self) -> u16 {
        self.channels
    }
}

/// Loads a preview for one selected content entry.
///
/// Returning `Ok(None)` is deliberate: the catalog can list many content
/// types before the resource lab knows how to validate them all visually.
pub(crate) fn load_preview_for_entry(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
    cache: Option<&super::ArchivePreloadCache>,
    handles: Option<&super::ArchiveHandleCache>,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    // Stash the caches in thread-locals so every load_entry_bytes() call in
    // the individual loaders can transparently benefit from preloaded archives
    // and persistent handles without threading parameters through every function.
    THREAD_LOCAL_CACHE.with_borrow_mut(|tl| *tl = cache.cloned());
    THREAD_LOCAL_HANDLES.with_borrow_mut(|tl| *tl = handles.cloned());
    let result = match entry_extension_lower(entry).as_deref() {
        Some("shp") => Ok(Some(load_shp_preview(entry, catalogs)?)),
        Some("pal") => Ok(Some(load_palette_preview(entry)?)),
        Some("aud") => Ok(Some(load_aud_preview(entry)?)),
        Some("wav") => Ok(Some(load_wav_preview(entry)?)),
        Some("wsa") => Ok(Some(load_wsa_preview(entry, catalogs)?)),
        Some("vqa") => Ok(Some(load_vqa_preview(entry)?)),
        Some("ini") | Some("yaml") | Some("xml") | Some("miniyaml") | Some("txt") => {
            Ok(Some(load_text_preview(entry)?))
        }
        Some("eng") | Some("fre") | Some("ger") => Ok(Some(load_eng_preview(entry)?)),
        Some("lut") => Ok(Some(load_lut_preview(entry)?)),
        Some("vqp") => Ok(Some(load_vqp_preview(entry)?)),
        Some("fnt") => Ok(Some(load_fnt_preview(entry)?)),
        Some("tmp") => Ok(Some(load_tmp_preview(entry, catalogs)?)),
        _ => match entry.family {
            ContentFamily::Config | ContentFamily::Document => Ok(Some(load_text_preview(entry)?)),
            _ => Ok(None),
        },
    };
    THREAD_LOCAL_CACHE.with_borrow_mut(|tl| *tl = None);
    result
}

/// Returns the validation surfaces the content lab knows how to expose for an
/// entry, based on its extension and broad family.
pub(crate) fn preview_capabilities_for_entry(entry: &ContentCatalogEntry) -> PreviewCapabilities {
    match entry_extension_lower(entry).as_deref() {
        Some("shp") | Some("pal") | Some("wsa") | Some("lut") | Some("vqp") | Some("fnt")
        | Some("tmp") => PreviewCapabilities {
            visual: true,
            audio: false,
            text: false,
        },
        Some("aud") | Some("wav") => PreviewCapabilities {
            visual: true,
            audio: true,
            text: false,
        },
        Some("vqa") => PreviewCapabilities {
            visual: true,
            audio: true,
            text: false,
        },
        Some("ini") | Some("yaml") | Some("xml") | Some("miniyaml") | Some("txt") | Some("eng")
        | Some("fre") | Some("ger") => PreviewCapabilities {
            visual: false,
            audio: false,
            text: true,
        },
        _ => match entry.family {
            ContentFamily::Config | ContentFamily::Document => PreviewCapabilities {
                visual: false,
                audio: false,
                text: true,
            },
            _ => PreviewCapabilities::default(),
        },
    }
}

/// Resolves the external palette entry that should be used for a visual
/// preview.
///
/// This early resource-lab pass uses a pragmatic classic RA heuristic rather
/// than the full theater/context model from later milestones: prefer a known
/// palette name such as `TEMPERAT.PAL`, then fall back to any available `.pal`
/// if that is all the current source set provides.
pub(crate) fn resolve_palette_entry_for_visual<'a>(
    visual_entry: &ContentCatalogEntry,
    catalogs: &'a [ContentCatalog],
) -> Option<&'a ContentCatalogEntry> {
    let preferred_names = preferred_palette_names_for_visual(visual_entry);

    for &preferred_name in preferred_names {
        if let Some(entry) = catalogs
            .iter()
            .flat_map(|catalog| catalog.entries.iter())
            .find(|entry| {
                entry.family == ContentFamily::Palette
                    && entry_file_name_upper(entry) == preferred_name
                    && entry
                        .location
                        .shares_archive_container_with(&visual_entry.location)
            })
        {
            return Some(entry);
        }

        if let Some(entry) = catalogs
            .iter()
            .flat_map(|catalog| catalog.entries.iter())
            .find(|entry| {
                entry.family == ContentFamily::Palette
                    && entry_file_name_upper(entry) == preferred_name
            })
        {
            return Some(entry);
        }
    }

    catalogs
        .iter()
        .flat_map(|catalog| catalog.entries.iter())
        .find(|entry| entry.family == ContentFamily::Palette)
}

pub(crate) fn load_entry_bytes(entry: &ContentCatalogEntry) -> Result<Vec<u8>, PreviewLoadError> {
    THREAD_LOCAL_CACHE.with_borrow(|cache| {
        THREAD_LOCAL_HANDLES.with_borrow(|handles| {
            load_entry_bytes_impl(entry, cache.as_ref(), handles.as_ref())
        })
    })
}

/// Loads entry bytes with a three-tier strategy:
///
/// 1. **RAM cache** (`ArchivePreloadCache`) — fastest, used when `preload_archives` is on.
/// 2. **Persistent handle** (`ArchiveHandleCache`) — index parsed once, subsequent reads
///    are a cheap seek + read. This is the default fast path.
/// 3. **Disk re-open** — fallback when neither cache is available.
pub(crate) fn load_entry_bytes_cached(
    entry: &ContentCatalogEntry,
    cache: Option<&super::ArchivePreloadCache>,
    handles: Option<&super::ArchiveHandleCache>,
) -> Result<Vec<u8>, PreviewLoadError> {
    load_entry_bytes_impl(entry, cache, handles)
}

fn load_entry_bytes_impl(
    entry: &ContentCatalogEntry,
    cache: Option<&super::ArchivePreloadCache>,
    handles: Option<&super::ArchiveHandleCache>,
) -> Result<Vec<u8>, PreviewLoadError> {
    match &entry.location {
        ContentEntryLocation::Filesystem { absolute_path } => Ok(fs::read(absolute_path)?),
        ContentEntryLocation::MixMember {
            archive_path,
            archive_index,
            parent_indices,
            ..
        } => {
            use ic_cnc_content::cnc_formats::mix::MixArchiveReader;

            // ── Nested MIX path (parent_indices non-empty) ──────────
            //
            // For entries like MAIN.MIX::MOVIES1.MIX::PROLOG.VQA, the
            // expensive step is reading the intermediate archive (369 MB
            // MOVIES1.MIX) from the outer archive. Cache those bytes so
            // browsing multiple entries from the same intermediate MIX
            // only pays the read cost once.
            if !parent_indices.is_empty() {
                return load_nested_mix_entry(
                    archive_path, parent_indices, *archive_index, &entry.relative_path,
                    cache, handles,
                );
            }

            // ── Flat MIX path (no nesting) ──────────────────────────

            // Tier 1: RAM cache (full archive in memory).
            let cached_bytes = cache
                .and_then(|c| c.archives.lock().ok())
                .and_then(|map| map.get(archive_path).cloned());
            if let Some(bytes) = cached_bytes {
                let cursor = std::io::Cursor::new(bytes.as_ref());
                let mut reader = MixArchiveReader::open(cursor)?;
                return read_mix_chain(&mut reader, parent_indices, *archive_index, &entry.relative_path);
            }

            // Tier 2: Persistent file handle (index parsed once).
            if let Some(handle_cache) = handles {
                if let Ok(handle) = handle_cache.get_or_open_mix(archive_path) {
                    let mut reader = handle.lock().unwrap_or_else(|e| e.into_inner());
                    return read_mix_chain(&mut reader, parent_indices, *archive_index, &entry.relative_path);
                }
            }

            // Tier 3: Fallback — open, parse, read, close.
            let file = fs::File::open(archive_path)?;
            let mut reader = MixArchiveReader::open(std::io::BufReader::new(file))?;
            read_mix_chain(&mut reader, parent_indices, *archive_index, &entry.relative_path)
        }
        ContentEntryLocation::MegMember {
            archive_path,
            archive_index,
            ..
        } => {
            use ic_cnc_content::cnc_formats::meg::MegArchiveReader;

            // Tier 1: RAM cache.
            let cached_bytes = cache
                .and_then(|c| c.archives.lock().ok())
                .and_then(|map| map.get(archive_path).cloned());
            if let Some(bytes) = cached_bytes {
                let cursor = std::io::Cursor::new(bytes.as_ref());
                let mut reader = MegArchiveReader::open(cursor)?;
                return reader
                    .read_by_index(*archive_index)?
                    .ok_or_else(|| PreviewLoadError::EmptyVisual {
                        entry_path: entry.relative_path.clone(),
                    });
            }

            // Tier 2: Persistent file handle.
            if let Some(handle_cache) = handles {
                if let Ok(handle) = handle_cache.get_or_open_meg(archive_path) {
                    let mut reader = handle.lock().unwrap_or_else(|e| e.into_inner());
                    return reader
                        .read_by_index(*archive_index)?
                        .ok_or_else(|| PreviewLoadError::EmptyVisual {
                            entry_path: entry.relative_path.clone(),
                        });
                }
            }

            // Tier 3: Fallback.
            let file = fs::File::open(archive_path)?;
            let mut reader = MegArchiveReader::open(std::io::BufReader::new(file))?;
            reader
                .read_by_index(*archive_index)?
                .ok_or_else(|| PreviewLoadError::EmptyVisual {
                    entry_path: entry.relative_path.clone(),
                })
        }
    }
}

/// Maximum size of an intermediate MIX archive during nested chain traversal.
///
/// Prevents a crafted archive from causing unbounded memory allocation.
/// RA1's movie MIX archives can reach ~370 MB (MOVIES1.MIX); 512 MB
/// accommodates any legitimate archive while blocking degenerate inputs.
const MAX_NESTED_MIX_BYTES: usize = 512 * 1024 * 1024;

/// Maximum nesting depth for `read_mix_chain`.
///
/// Matches `mount_nested_mix_members` in catalog.rs. Prevents stack
/// exhaustion or unbounded allocation chains from crafted archives.
const MAX_MIX_CHAIN_DEPTH: usize = 3;

/// Reads an entry through a chain of nested MIX archives.
///
/// For top-level entries (`parent_indices` is empty), this is a simple
/// `read_by_index`. For nested entries, each parent index is read to obtain
/// the inner MIX bytes, which are then parsed to reach the next level.
///
/// # Safety limits
///
/// - Intermediate archives are capped at [`MAX_NESTED_MIX_BYTES`] (64 MB).
/// - Chain depth is capped at [`MAX_MIX_CHAIN_DEPTH`] (3 levels).
fn read_mix_chain<R: std::io::Read + std::io::Seek>(
    reader: &mut ic_cnc_content::cnc_formats::mix::MixArchiveReader<R>,
    parent_indices: &[usize],
    leaf_index: usize,
    entry_path: &str,
) -> Result<Vec<u8>, PreviewLoadError> {
    use ic_cnc_content::cnc_formats::mix::MixArchive;

    if parent_indices.len() > MAX_MIX_CHAIN_DEPTH {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: format!("{entry_path} (nesting depth {} exceeds limit {})", parent_indices.len(), MAX_MIX_CHAIN_DEPTH),
        });
    }

    if parent_indices.is_empty() {
        // Top-level entry — direct read.
        return reader
            .read_by_index(leaf_index)?
            .ok_or_else(|| PreviewLoadError::EmptyVisual {
                entry_path: entry_path.to_string(),
            });
    }

    // Walk through the nesting chain. Each parent index yields the bytes of
    // an inner MIX archive.
    let mut current_bytes = reader
        .read_by_index(parent_indices[0])?
        .ok_or_else(|| PreviewLoadError::EmptyVisual {
            entry_path: entry_path.to_string(),
        })?;

    if current_bytes.len() > MAX_NESTED_MIX_BYTES {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: format!("{entry_path} (intermediate MIX {} bytes exceeds {} byte limit)", current_bytes.len(), MAX_NESTED_MIX_BYTES),
        });
    }

    for &parent_idx in &parent_indices[1..] {
        let inner = MixArchive::parse(&current_bytes)?;
        current_bytes = inner
            .get_by_index(parent_idx)
            .ok_or_else(|| PreviewLoadError::EmptyVisual {
                entry_path: entry_path.to_string(),
            })?
            .to_vec();

        if current_bytes.len() > MAX_NESTED_MIX_BYTES {
            return Err(PreviewLoadError::EmptyVisual {
                entry_path: format!("{entry_path} (intermediate MIX {} bytes exceeds {} byte limit)", current_bytes.len(), MAX_NESTED_MIX_BYTES),
            });
        }
    }

    // Final level: read the leaf entry from the innermost archive.
    let inner = MixArchive::parse(&current_bytes)?;
    inner
        .get_by_index(leaf_index)
        .map(|s| s.to_vec())
        .ok_or_else(|| PreviewLoadError::EmptyVisual {
            entry_path: entry_path.to_string(),
        })
}

/// Loads a leaf entry from a nested MIX chain using seek-based I/O.
///
/// Instead of reading an entire intermediate archive into RAM (MOVIES1.MIX
/// is 369 MB), this opens a bounded `MixEntryReader` window over the nested
/// archive region and parses only its header + the target entry. Total I/O
/// is the small MIX header (a few hundred bytes) plus the leaf entry itself.
///
/// This matches how the original Red Alert engine accessed nested MIX files
/// on hardware with 16 MB of RAM — pure offset arithmetic and seeks, never
/// materialising intermediate archives.
fn load_nested_mix_entry(
    archive_path: &std::path::Path,
    parent_indices: &[usize],
    leaf_index: usize,
    entry_path: &str,
    _cache: Option<&super::ArchivePreloadCache>,
    handles: Option<&super::ArchiveHandleCache>,
) -> Result<Vec<u8>, PreviewLoadError> {
    use ic_cnc_content::cnc_formats::mix::MixArchiveReader;

    if parent_indices.len() > MAX_MIX_CHAIN_DEPTH {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: format!("{entry_path} (nesting depth {} exceeds limit {})", parent_indices.len(), MAX_MIX_CHAIN_DEPTH),
        });
    }

    // Obtain a persistent handle for the outer archive.
    if let Some(handle_cache) = handles {
        if let Ok(handle) = handle_cache.get_or_open_mix(archive_path) {
            let mut outer = handle.lock().unwrap_or_else(|e| e.into_inner());
            return read_nested_via_seek(&mut *outer, parent_indices, leaf_index, entry_path);
        }
    }

    // Fallback: open a fresh reader.
    let file = fs::File::open(archive_path)?;
    let mut outer = MixArchiveReader::open(std::io::BufReader::new(file))?;
    read_nested_via_seek(&mut outer, parent_indices, leaf_index, entry_path)
}

/// Walks the nesting chain using bounded entry readers (no bulk allocation).
///
/// For a single level of nesting (the common case), this opens a
/// `MixEntryReader` over the intermediate archive's byte range, then opens
/// a nested `MixArchiveReader` on that window. The nested reader parses
/// only the small CRC index header and seeks directly to the leaf entry.
fn read_nested_via_seek<R: std::io::Read + std::io::Seek>(
    outer: &mut ic_cnc_content::cnc_formats::mix::MixArchiveReader<R>,
    parent_indices: &[usize],
    leaf_index: usize,
    entry_path: &str,
) -> Result<Vec<u8>, PreviewLoadError> {
    use ic_cnc_content::cnc_formats::mix::MixArchiveReader;

    // Open a bounded reader over the first intermediate archive.
    let mut entry_reader = outer
        .open_entry_by_index(parent_indices[0])?
        .ok_or_else(|| PreviewLoadError::EmptyVisual {
            entry_path: entry_path.to_string(),
        })?;

    if parent_indices.len() == 1 {
        // Common case: one level of nesting (e.g. MAIN.MIX::MOVIES1.MIX::PROLOG.VQA).
        // Parse the intermediate header directly from the bounded reader
        // and read only the leaf entry bytes.
        let mut inner = MixArchiveReader::open(&mut entry_reader)?;
        return inner.read_by_index(leaf_index)?.ok_or_else(|| PreviewLoadError::EmptyVisual {
            entry_path: entry_path.to_string(),
        });
    }

    // Rare case: deeper nesting (3+ levels). We must materialise each
    // intermediate level because MixEntryReader borrows its parent reader,
    // preventing simultaneous nesting of entry readers.
    let mut current_bytes = {
        let mut buf = Vec::with_capacity(entry_reader.len() as usize);
        std::io::Read::read_to_end(&mut entry_reader, &mut buf)?;
        buf
    };

    if current_bytes.len() > MAX_NESTED_MIX_BYTES {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: format!("{entry_path} (intermediate MIX {} bytes exceeds {} byte limit)", current_bytes.len(), MAX_NESTED_MIX_BYTES),
        });
    }

    for &parent_idx in &parent_indices[1..] {
        let cursor = std::io::Cursor::new(&current_bytes);
        let mut reader = MixArchiveReader::open(cursor)?;
        current_bytes = reader
            .read_by_index(parent_idx)?
            .ok_or_else(|| PreviewLoadError::EmptyVisual {
                entry_path: entry_path.to_string(),
            })?;

        if current_bytes.len() > MAX_NESTED_MIX_BYTES {
            return Err(PreviewLoadError::EmptyVisual {
                entry_path: format!("{entry_path} (intermediate MIX {} bytes exceeds {} byte limit)", current_bytes.len(), MAX_NESTED_MIX_BYTES),
            });
        }
    }

    let cursor = std::io::Cursor::new(&current_bytes);
    let mut reader = MixArchiveReader::open(cursor)?;
    reader
        .read_by_index(leaf_index)?
        .ok_or_else(|| PreviewLoadError::EmptyVisual {
            entry_path: entry_path.to_string(),
        })
}

/// Parses an SHP file via the standard `ShpSprite::parse` path.
///
/// The underlying `cnc-formats` parser already accepts non-zero garbage
/// in the EOF sentinel and padding entries (matching original Westwood
/// tool behaviour), so no client-side patching is needed.
fn parse_shp_lenient(bytes: Vec<u8>) -> Result<ShpSprite, cnc_formats::Error> {
    ShpSprite::parse(bytes)
}

fn load_shp_preview(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    // Try TD/RA1 keyframe format first; fall back to Dune II format.
    match parse_shp_lenient(bytes.clone()) {
        Ok(shp) => load_shp_td_preview_from_parsed(shp, entry, catalogs),
        Err(td_err) => load_shp_d2_preview_from_bytes(&bytes, entry, catalogs)
            .map_err(|_| PreviewLoadError::from(td_err)),
    }
}

fn load_shp_td_preview_from_parsed(
    shp: ShpSprite,
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let (palette, palette_label) =
        palette_for_indexed_visual(entry, catalogs, shp.embedded_palette.clone())?;
    let render_palette = PaletteTextureSource::from_handoff(palette.render_handoff());
    let sheet_handoff = shp.render_handoff();

    // Degenerate SHP files (e.g. SIDEBAR.SHP from HIRES.MIX) have width=0
    // height=0 — there are no pixels to display.
    if sheet_handoff.width == 0 || sheet_handoff.height == 0 {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: entry.relative_path.clone(),
        });
    }

    let decoded_frames = shp.decode_frames()?;
    if decoded_frames.is_empty() {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: entry.relative_path.clone(),
        });
    }

    let _validated_sprite = StaticRenderSprite::new(
        SpriteSheetSource::from_handoff(sheet_handoff.clone()),
        0,
        palette.render_handoff(),
        RenderLayer::UiOverlay,
    )?;

    let frames = decoded_frames
        .into_iter()
        .map(|pixels| {
            let indexed_frame = IndexedSpriteFrame::new(
                sheet_handoff.width.into(),
                sheet_handoff.height.into(),
                pixels,
            )?;
            Ok(indexed_frame.to_rgba(&render_palette))
        })
        .collect::<Result<Vec<_>, SpriteBootstrapError>>()?;
    Ok(PreparedContentPreview {
        label: format!("Actual SHP preview: {}", entry.relative_path),
        details: vec![
            format!(
                "frames: {} at {}x{} pixels",
                sheet_handoff.frame_count, sheet_handoff.width, sheet_handoff.height
            ),
            format!("palette: {palette_label}"),
            preview_control_hint(true, false),
        ],
        visual: Some(VisualPreview {
            frames,
            frame_duration_seconds: (sheet_handoff.frame_count > 1)
                .then_some(1.0 / DEFAULT_ANIMATION_FPS),
        }),
        audio: None,
        text_body: None,
    })
}

/// Fallback SHP loader for Dune II–format sprite files.
///
/// Unlike the TD/RA1 format, SHP D2 files have a 2-byte header followed by a
/// u32 offset table and per-frame headers that carry per-frame dimensions.
/// Some RA1 game files (e.g. cursor sprites in LORES.MIX and EDITOR.MIX) are
/// stored in this format despite being distributed with other RA1 content.
fn load_shp_d2_preview_from_bytes(
    bytes: &[u8],
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let d2 = cnc_formats::shp_d2::ShpD2File::parse(bytes)?;
    if d2.frame_count() == 0 {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: entry.relative_path.clone(),
        });
    }

    // D2 SHPs have no embedded palette — always resolve from the catalog.
    let (palette, palette_label) = palette_for_indexed_visual(entry, catalogs, None)?;
    let render_palette = PaletteTextureSource::from_handoff(palette.render_handoff());

    let first = d2.frame(0).expect("frame_count > 0");
    let (w0, h0) = (first.width, first.height);

    let frames = d2
        .frames()
        .iter()
        .map(|f| {
            // Each D2 frame carries its own dimensions; the display surface
            // resizes automatically (see update_image_from_rgba_frame).
            let indexed = IndexedSpriteFrame::new(
                f.width as u32,
                f.height as u32,
                f.pixels.clone(),
            )?;
            Ok(indexed.to_rgba(&render_palette))
        })
        .collect::<Result<Vec<_>, SpriteBootstrapError>>()?;

    Ok(PreparedContentPreview {
        label: format!("Actual SHP preview: {}", entry.relative_path),
        details: vec![
            format!("frames: {} at {}x{} pixels (D2)", d2.frame_count(), w0, h0),
            format!("palette: {palette_label}"),
            preview_control_hint(true, false),
        ],
        visual: Some(VisualPreview {
            frames,
            frame_duration_seconds: (d2.frame_count() > 1)
                .then_some(1.0 / DEFAULT_ANIMATION_FPS),
        }),
        audio: None,
        text_body: None,
    })
}

fn load_palette_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let palette = Palette::parse(load_entry_bytes(entry)?)?;
    let render_palette = PaletteTextureSource::from_handoff(palette.render_handoff());
    let indexed_grid = IndexedSpriteFrame::new(
        PALETTE_PREVIEW_COLUMNS,
        PALETTE_PREVIEW_ROWS,
        (0u16..(PALETTE_PREVIEW_COLUMNS * PALETTE_PREVIEW_ROWS) as u16)
            .map(|index| index as u8)
            .collect(),
    )?;
    let frame = indexed_grid.to_rgba(&render_palette);

    Ok(PreparedContentPreview {
        label: format!("Actual PAL preview: {}", entry.relative_path),
        details: vec![
            "16x16 palette swatch grid".into(),
            format!(
                "{} colors expanded for preview",
                render_palette.color_count()
            ),
            preview_control_hint(false, false),
        ],
        visual: Some(VisualPreview {
            frames: vec![frame],
            frame_duration_seconds: None,
        }),
        audio: None,
        text_body: None,
    })
}

fn load_aud_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let aud_file = cnc_formats::aud::AudFile::parse(&bytes)?;
    let channels = aud_file.header.channel_count() as u16;
    // Use AudStream to decode all compression types correctly.
    //
    // decode_adpcm() operates on a flat byte slice and cannot handle
    // SCOMP_SOS = 99 files, which embed 8-byte chunk headers (0x0000DEAF
    // magic) between every compressed block.  decode_adpcm treats those
    // header bytes as ADPCM nibbles and produces noise bursts throughout
    // the decoded audio.  AudStream::from_payload skips chunk headers
    // before dispatching the correct decoder, so it works for SCOMP_NONE,
    // SCOMP_WESTWOOD, and SCOMP_SOS without any per-type branching here.
    let sample_limit = aud_file.header.sample_frames() * usize::from(channels);
    let mut stream = cnc_formats::aud::AudStream::from_payload(
        aud_file.header.clone(),
        Cursor::new(aud_file.compressed_data),
    );
    let mut samples: Vec<i16> = Vec::with_capacity(sample_limit);
    let mut buf = [0i16; 4096];
    loop {
        let n = stream.read_samples(&mut buf)?;
        if n == 0 {
            break;
        }
        samples.extend_from_slice(&buf[..n]);
    }
    let waveform = waveform_frame(&samples, WAVEFORM_WIDTH, WAVEFORM_HEIGHT)?;
    let duration_seconds =
        audio_duration_seconds(samples.len(), aud_file.header.sample_rate as u32, channels);

    Ok(PreparedContentPreview {
        label: format!("Actual AUD preview: {}", entry.relative_path),
        details: vec![
            format!(
                "sample rate: {} Hz | channels: {} | duration: {:.2}s",
                aud_file.header.sample_rate, channels, duration_seconds
            ),
            "waveform preview plus direct PCM playback".into(),
            preview_control_hint(false, true),
        ],
        visual: Some(VisualPreview {
            frames: vec![waveform],
            frame_duration_seconds: None,
        }),
        audio: Some(AudioPreview {
            pcm_samples: Arc::<[i16]>::from(samples),
            duration_seconds,
            sample_rate: aud_file.header.sample_rate as u32,
            channels,
        }),
        text_body: None,
    })
}

fn load_wav_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let wav_bytes = load_entry_bytes(entry)?;
    let mut reader = WavReader::new(Cursor::new(wav_bytes))?;
    let spec = reader.spec();
    let samples = reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?;
    let waveform = waveform_frame(&samples, WAVEFORM_WIDTH, WAVEFORM_HEIGHT)?;
    let duration_seconds = audio_duration_seconds(samples.len(), spec.sample_rate, spec.channels);

    Ok(PreparedContentPreview {
        label: format!("Actual WAV preview: {}", entry.relative_path),
        details: vec![
            format!(
                "sample rate: {} Hz | channels: {} | duration: {:.2}s",
                spec.sample_rate, spec.channels, duration_seconds
            ),
            "waveform preview plus direct PCM playback".into(),
            preview_control_hint(false, true),
        ],
        visual: Some(VisualPreview {
            frames: vec![waveform],
            frame_duration_seconds: None,
        }),
        audio: Some(AudioPreview {
            pcm_samples: Arc::<[i16]>::from(samples),
            duration_seconds,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        }),
        text_body: None,
    })
}

fn load_wsa_preview(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let wsa = cnc_formats::wsa::WsaFile::parse(&bytes)?;

    let embedded_palette = wsa.palette.map(|palette| palette.to_vec());
    let (palette, palette_label) = palette_for_indexed_visual(entry, catalogs, embedded_palette)?;
    let render_palette = PaletteTextureSource::from_handoff(palette.render_handoff());
    let decoded_frames = wsa.decode_frames()?;
    if decoded_frames.is_empty() {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: entry.relative_path.clone(),
        });
    }

    let frames = decoded_frames
        .into_iter()
        .map(|pixels| {
            let indexed =
                IndexedSpriteFrame::new(wsa.header.width as u32, wsa.header.height as u32, pixels)?;
            Ok(indexed.to_rgba(&render_palette))
        })
        .collect::<Result<Vec<_>, SpriteBootstrapError>>()?;

    Ok(PreparedContentPreview {
        label: format!("Actual WSA preview: {}", entry.relative_path),
        details: vec![
            format!(
                "frames: {} at {}x{} pixels",
                wsa.header.num_frames, wsa.header.width, wsa.header.height
            ),
            format!("palette: {palette_label}"),
            preview_control_hint(true, false),
        ],
        visual: Some(VisualPreview {
            frames,
            frame_duration_seconds: (wsa.header.num_frames > 1)
                .then_some(1.0 / DEFAULT_ANIMATION_FPS),
        }),
        audio: None,
        text_body: None,
    })
}

fn load_vqa_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let vqa = cnc_formats::vqa::VqaFile::parse(&bytes)?;
    let decoded_frames = vqa.decode_frames()?;
    if decoded_frames.is_empty() {
        return Err(PreviewLoadError::EmptyVisual {
            entry_path: entry.relative_path.clone(),
        });
    }

    let frames = decoded_frames
        .into_iter()
        .map(|frame| {
            rgba_frame_from_palette_indices(
                vqa.header.width as u32,
                vqa.header.height as u32,
                &frame.pixels,
                &frame.palette,
                // VQA movie frames are full-screen raster images, not sprite
                // sheets. Palette index 0 is therefore a real color entry, not
                // an implicit transparent background like many SHP/TMP-style
                // art assets. Treating it as transparent produces the exact
                // failure mode the user saw: mostly dark screen, sparse visible
                // fragments, and correct audio.
                false,
                false,
            )
        })
        .collect::<Result<Vec<_>, SpriteBootstrapError>>()?;

    let audio = if let Some(audio) = vqa.extract_audio()? {
        let channels = audio.channels as u16;
        let duration_seconds =
            audio_duration_seconds(audio.samples.len(), audio.sample_rate as u32, channels);
        Some(AudioPreview {
            pcm_samples: Arc::<[i16]>::from(audio.samples),
            duration_seconds,
            sample_rate: audio.sample_rate as u32,
            channels,
        })
    } else {
        None
    };

    let mut details = vec![
        format!(
            "frames: {} at {}x{} pixels | fps: {}",
            vqa.header.num_frames,
            vqa.header.width,
            vqa.header.height,
            vqa.header.fps.max(1)
        ),
        format!(
            "audio: {}",
            if audio.is_some() { "present" } else { "none" }
        ),
        preview_control_hint(true, audio.is_some()),
    ];
    if let Some(audio) = &audio {
        details.push(format!(
            "audio playback: {:.2}s at {} Hz, {} channels",
            audio.duration_seconds(),
            audio.sample_rate(),
            audio.channels()
        ));
    }

    Ok(PreparedContentPreview {
        label: format!("Actual VQA preview: {}", entry.relative_path),
        details,
        visual: Some(VisualPreview {
            frames,
            frame_duration_seconds: Some(1.0 / (vqa.header.fps.max(1) as f32)),
        }),
        audio,
        text_body: None,
    })
}

fn load_text_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let excerpt = trimmed_text_excerpt(&body);
    let line_count = body.lines().count();

    Ok(PreparedContentPreview {
        label: format!("Text preview: {}", entry.relative_path),
        details: vec![
            format!("{} lines | {} bytes", line_count, bytes.len()),
            "text/config validation surface".into(),
            preview_control_hint(false, false),
        ],
        visual: None,
        audio: None,
        text_body: Some(excerpt),
    })
}

fn load_eng_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let eng = cnc_formats::eng::EngFile::parse(&bytes)?;
    let excerpt = eng
        .strings
        .iter()
        .take(TEXT_PREVIEW_MAX_LINES)
        .map(|string| format!("{:03}: {}", string.index, string.as_lossy_str()))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(PreparedContentPreview {
        label: format!("String-table preview: {}", entry.relative_path),
        details: vec![
            format!("strings: {}", eng.string_count()),
            "localized text validation surface".into(),
            preview_control_hint(false, false),
        ],
        visual: None,
        audio: None,
        text_body: Some(excerpt),
    })
}

fn load_lut_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let lut = cnc_formats::lut::LutFile::parse(&bytes)?;
    let mut rgba = Vec::with_capacity(cnc_formats::lut::LUT_ENTRY_COUNT * 4);
    for lut_entry in &lut.entries {
        let intensity = lut_entry.value.saturating_mul(17);
        rgba.extend_from_slice(&[
            intensity,
            80u8.saturating_add(intensity / 2),
            255u8.saturating_sub(intensity),
            255,
        ]);
    }

    Ok(PreparedContentPreview {
        label: format!("Actual LUT preview: {}", entry.relative_path),
        details: vec![
            format!("entries: {}", lut.entry_count()),
            "64x64 Chrono Vortex lookup heatmap".into(),
            preview_control_hint(false, false),
        ],
        visual: Some(VisualPreview {
            frames: vec![RgbaSpriteFrame::from_rgba(64, 64, rgba)?],
            frame_duration_seconds: None,
        }),
        audio: None,
        text_body: None,
    })
}

fn load_vqp_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let vqp = cnc_formats::vqp::VqpFile::parse(&bytes)?;
    let table = match vqp.tables.first() {
        Some(table) => table,
        None => {
            return Ok(PreparedContentPreview {
                label: format!("Actual VQP preview: {}", entry.relative_path),
                details: vec![
                    "0 interpolation tables".into(),
                    preview_control_hint(false, false),
                ],
                visual: None,
                audio: None,
                text_body: Some("This VQP file contains no interpolation tables.".into()),
            });
        }
    };

    let mut rgba = Vec::with_capacity(256 * 256 * 4);
    for left in 0u8..=255 {
        for right in 0u8..=255 {
            let value = table.get(left, right);
            rgba.extend_from_slice(&[value, value, value, 255]);
        }
    }

    Ok(PreparedContentPreview {
        label: format!("Actual VQP preview: {}", entry.relative_path),
        details: vec![
            format!("tables: {}", vqp.num_tables),
            "256x256 interpolation-table heatmap (table 0)".into(),
            preview_control_hint(false, false),
        ],
        visual: Some(VisualPreview {
            frames: vec![RgbaSpriteFrame::from_rgba(256, 256, rgba)?],
            frame_duration_seconds: None,
        }),
        audio: None,
        text_body: None,
    })
}

fn load_fnt_preview(
    entry: &ContentCatalogEntry,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let fnt = cnc_formats::fnt::FntFile::parse(&bytes)?;
    let cell_w = fnt.header.max_width as u32;
    let cell_h = fnt.header.max_height as u32;
    let atlas_w = cell_w.max(1) * 16;
    let atlas_h = cell_h.max(1) * 16;
    let mut rgba = vec![
        0u8;
        (atlas_w as usize)
            .saturating_mul(atlas_h as usize)
            .saturating_mul(4)
    ];

    for glyph in &fnt.glyphs {
        let grid_col = (glyph.code as u32) % 16;
        let grid_row = (glyph.code as u32) / 16;
        let base_x = grid_col.saturating_mul(cell_w.max(1));
        let base_y = grid_row
            .saturating_mul(cell_h.max(1))
            .saturating_add(glyph.y_offset as u32);

        for y in 0..glyph.data_rows as u32 {
            for x in 0..glyph.width as u32 {
                let color_index = glyph.pixel(x as u8, y as u8);
                if color_index == 0 {
                    continue;
                }

                let intensity = color_index.saturating_mul(17);
                let atlas_x = base_x.saturating_add(x);
                let atlas_y = base_y.saturating_add(y);
                let pixel_index = (atlas_y as usize)
                    .saturating_mul(atlas_w as usize)
                    .saturating_add(atlas_x as usize)
                    .saturating_mul(4);
                if let Some(pixel) = rgba.get_mut(pixel_index..pixel_index + 4) {
                    pixel.copy_from_slice(&[intensity, intensity, intensity, 255]);
                }
            }
        }
    }

    Ok(PreparedContentPreview {
        label: format!("Actual FNT preview: {}", entry.relative_path),
        details: vec![
            format!(
                "glyphs: {} | atlas cell: {}x{}",
                fnt.glyphs.len(),
                cell_w,
                cell_h
            ),
            "16x16 bitmap-font atlas".into(),
            preview_control_hint(false, false),
        ],
        visual: Some(VisualPreview {
            frames: vec![RgbaSpriteFrame::from_rgba(
                atlas_w.max(1),
                atlas_h.max(1),
                rgba,
            )?],
            frame_duration_seconds: None,
        }),
        audio: None,
        text_body: None,
    })
}

fn load_tmp_preview(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let palette_entry = resolve_palette_entry_for_visual(entry, catalogs).ok_or_else(|| {
        PreviewLoadError::MissingPalette {
            entry_path: entry.relative_path.clone(),
        }
    })?;
    let palette = Palette::parse(load_entry_bytes(palette_entry)?)?;
    let palette_source = PaletteTextureSource::from_handoff(palette.render_handoff());

    if let Ok(ra_tmp) = cnc_formats::tmp::RaTmpFile::parse(&bytes) {
        let frame = build_ra_tmp_preview_frame(&ra_tmp, &palette_source)?;
        return Ok(PreparedContentPreview {
            label: format!("Actual RA TMP preview: {}", entry.relative_path),
            details: vec![
                format!(
                    "tiles: {} in a {}x{} grid",
                    ra_tmp.tiles.len(),
                    ra_tmp.header.cols(),
                    ra_tmp.header.rows()
                ),
                format!("palette: {}", palette_entry.relative_path),
                preview_control_hint(false, false),
            ],
            visual: Some(VisualPreview {
                frames: vec![frame],
                frame_duration_seconds: None,
            }),
            audio: None,
            text_body: None,
        });
    }

    let td_tmp = cnc_formats::tmp::TdTmpFile::parse(&bytes)?;
    let frame = build_td_tmp_preview_frame(&td_tmp, &palette_source)?;
    Ok(PreparedContentPreview {
        label: format!("Actual TD TMP preview: {}", entry.relative_path),
        details: vec![
            format!("tiles: {}", td_tmp.tiles.len()),
            format!("palette: {}", palette_entry.relative_path),
            preview_control_hint(false, false),
        ],
        visual: Some(VisualPreview {
            frames: vec![frame],
            frame_duration_seconds: None,
        }),
        audio: None,
        text_body: None,
    })
}

fn palette_for_indexed_visual(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
    embedded_palette: Option<Vec<u8>>,
) -> Result<(Palette, String), PreviewLoadError> {
    if let Some(embedded_palette) = embedded_palette {
        return Ok((
            Palette::parse(embedded_palette)?,
            "embedded palette".to_string(),
        ));
    }

    // Score/credits SHPs use palettes embedded in PCX backgrounds rather than
    // standalone .PAL files.  Try those before the generic .PAL fallback.
    if let Some(pcx_result) = try_pcx_palette_for_visual(entry, catalogs) {
        return Ok(pcx_result);
    }

    let palette_entry = resolve_palette_entry_for_visual(entry, catalogs).ok_or_else(|| {
        PreviewLoadError::MissingPalette {
            entry_path: entry.relative_path.clone(),
        }
    })?;
    let palette_bytes = load_entry_bytes(palette_entry)?;
    Ok((
        Palette::parse(palette_bytes)?,
        palette_entry.relative_path.clone(),
    ))
}

fn build_td_tmp_preview_frame(
    tmp: &cnc_formats::tmp::TdTmpFile<'_>,
    palette: &PaletteTextureSource,
) -> Result<RgbaSpriteFrame, SpriteBootstrapError> {
    let tile_count = tmp.tiles.len().min(TMP_PREVIEW_MAX_TILES);
    let columns = tile_count.clamp(1, TMP_PREVIEW_COLUMNS);
    let rows = tile_count.div_ceil(columns).max(1);
    let tile_w = tmp.header.icon_width as usize;
    let tile_h = tmp.header.icon_height as usize;
    let atlas_w = (columns * tile_w) as u32;
    let atlas_h = (rows * tile_h) as u32;
    let mut rgba = vec![
        0u8;
        (atlas_w as usize)
            .saturating_mul(atlas_h as usize)
            .saturating_mul(4)
    ];

    for (index, tile) in tmp.tiles.iter().take(tile_count).enumerate() {
        let col = index % columns;
        let row = index / columns;
        blit_indexed_tile(
            &mut rgba,
            tile.pixels,
            palette,
            TileBlitSpec {
                atlas_width: atlas_w as usize,
                dest_x: col * tile_w,
                dest_y: row * tile_h,
                tile_width: tile_w,
                tile_height: tile_h,
                transparent_zero: false,
            },
        );
    }

    RgbaSpriteFrame::from_rgba(atlas_w, atlas_h, rgba)
}

fn build_ra_tmp_preview_frame(
    tmp: &cnc_formats::tmp::RaTmpFile<'_>,
    palette: &PaletteTextureSource,
) -> Result<RgbaSpriteFrame, SpriteBootstrapError> {
    let tile_w = tmp.header.tile_width as usize;
    let tile_h = tmp.header.tile_height as usize;
    let grid_count = tmp.tiles.len().min(TMP_PREVIEW_MAX_TILES);
    let columns = grid_count.clamp(1, TMP_PREVIEW_COLUMNS);
    let rows = grid_count.div_ceil(columns).max(1);
    let atlas_w = (columns * tile_w) as u32;
    let atlas_h = (rows * tile_h) as u32;
    let mut rgba = vec![
        0u8;
        (atlas_w as usize)
            .saturating_mul(atlas_h as usize)
            .saturating_mul(4)
    ];

    for (index, tile_opt) in tmp.tiles.iter().take(grid_count).enumerate() {
        let Some(tile) = tile_opt else {
            continue;
        };
        let col = index % columns;
        let row = index / columns;
        blit_indexed_tile(
            &mut rgba,
            tile.pixels,
            palette,
            TileBlitSpec {
                atlas_width: atlas_w as usize,
                dest_x: col * tile_w,
                dest_y: row * tile_h,
                tile_width: tile_w,
                tile_height: tile_h,
                transparent_zero: false,
            },
        );
    }

    RgbaSpriteFrame::from_rgba(atlas_w, atlas_h, rgba)
}

struct TileBlitSpec {
    atlas_width: usize,
    dest_x: usize,
    dest_y: usize,
    tile_width: usize,
    tile_height: usize,
    transparent_zero: bool,
}

fn blit_indexed_tile(
    rgba: &mut [u8],
    pixels: &[u8],
    palette: &PaletteTextureSource,
    spec: TileBlitSpec,
) {
    for y in 0..spec.tile_height {
        for x in 0..spec.tile_width {
            let pixel_index = y.saturating_mul(spec.tile_width).saturating_add(x);
            let color_index = pixels.get(pixel_index).copied().unwrap_or(0);
            let [r, g, b] = palette.colors()[color_index as usize];
            let alpha = if spec.transparent_zero && color_index == 0 {
                0
            } else {
                255
            };
            let atlas_index = spec
                .dest_y
                .saturating_add(y)
                .saturating_mul(spec.atlas_width)
                .saturating_add(spec.dest_x.saturating_add(x))
                .saturating_mul(4);
            if let Some(pixel) = rgba.get_mut(atlas_index..atlas_index + 4) {
                pixel.copy_from_slice(&[r, g, b, alpha]);
            }
        }
    }
}

fn waveform_frame(
    samples: &[i16],
    width: u32,
    height: u32,
) -> Result<RgbaSpriteFrame, SpriteBootstrapError> {
    let mut rgba = vec![
        18u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    for alpha in rgba.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }

    let mid = (height / 2) as i32;
    for x in 0..width as usize {
        let start = x.saturating_mul(samples.len()) / (width as usize).max(1);
        let end = ((x + 1).saturating_mul(samples.len()) / (width as usize).max(1)).max(start + 1);
        let mut peak = 0i32;
        for &sample in samples.get(start..end).unwrap_or(&[]) {
            peak = peak.max((sample as i32).abs());
        }

        let amplitude = ((peak as f32 / i16::MAX as f32) * (height as f32 * 0.45)) as i32;
        draw_waveform_column(
            &mut rgba,
            width as usize,
            x,
            mid,
            amplitude.max(1),
            [104, 180, 255, 255],
        );
    }

    RgbaSpriteFrame::from_rgba(width, height, rgba)
}

fn draw_waveform_column(
    rgba: &mut [u8],
    width: usize,
    x: usize,
    mid: i32,
    amplitude: i32,
    color: [u8; 4],
) {
    let height = (rgba.len() / 4) / width;
    let start_y = (mid - amplitude).max(0) as usize;
    let end_y = (mid + amplitude).min(height as i32 - 1) as usize;
    for y in start_y..=end_y {
        let pixel_index = y.saturating_mul(width).saturating_add(x).saturating_mul(4);
        if let Some(pixel) = rgba.get_mut(pixel_index..pixel_index + 4) {
            pixel.copy_from_slice(&color);
        }
    }
}

/// Bayer 4×4 ordered dither matrix (values 0–15, canonical arrangement).
///
/// At each pixel position (x, y) the offset `BAYER_4X4[y % 4][x % 4]`
/// is subtracted from 7 to center the range, giving a bias in `[-7, +8]`.
/// Scaled to the 6-bit VGA step size (4 counts), this breaks visible
/// banding in palette gradients — the same technique used by ffplay and
/// OpenRA's software renderer.
const BAYER_4X4: [[i16; 4]; 4] = [
    [ 0,  8,  2, 10],
    [12,  4, 14,  6],
    [ 3, 11,  1,  9],
    [15,  7, 13,  5],
];

pub(crate) fn rgba_frame_from_palette_indices(
    width: u32,
    height: u32,
    pixels: &[u8],
    palette_rgb8: &[u8; 768],
    transparent_zero: bool,
    dither: bool,
) -> Result<RgbaSpriteFrame, SpriteBootstrapError> {
    let mut rgba = Vec::with_capacity(
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4),
    );
    let mut x = 0u32;
    let mut y = 0u32;
    for &index in pixels {
        let base = (index as usize).saturating_mul(3);
        let r = palette_rgb8.get(base).copied().unwrap_or(0);
        let g = palette_rgb8.get(base + 1).copied().unwrap_or(0);
        let b = palette_rgb8.get(base + 2).copied().unwrap_or(0);
        let alpha = if transparent_zero && index == 0 { 0 } else { 255 };

        let (r, g, b) = if dither {
            let bias = BAYER_4X4[(y % 4) as usize][(x % 4) as usize] - 7;
            (
                (r as i16 + bias).clamp(0, 255) as u8,
                (g as i16 + bias).clamp(0, 255) as u8,
                (b as i16 + bias).clamp(0, 255) as u8,
            )
        } else {
            (r, g, b)
        };

        rgba.extend_from_slice(&[r, g, b, alpha]);
        x += 1;
        if x >= width { x = 0; y += 1; }
    }
    RgbaSpriteFrame::from_rgba(width, height, rgba)
}

fn audio_duration_seconds(sample_count: usize, sample_rate: u32, channels: u16) -> f32 {
    if sample_rate == 0 || channels == 0 {
        return 0.0;
    }
    (sample_count as f32) / (sample_rate as f32 * channels as f32)
}

/// Loads only the first frame of a VQA video for instant display.
///
/// Uses the incremental `VqaDecoder` from cnc-formats ≥ 0.1.0-alpha.2 to
/// decode a single frame without materializing the entire movie.  The result
/// is a partial preview with one visual frame and no audio — the full decode
/// runs on a background thread and replaces this initial preview once it
/// finishes.
#[allow(dead_code)]
pub(crate) fn load_vqa_first_frame_preview(
    entry: &ContentCatalogEntry,
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let first = match super::vqa_stream::decode_vqa_first_frame(&bytes)? {
        Some(f) => f,
        None => return Ok(None),
    };
    Ok(Some(build_first_frame_preview(entry, first, true)?))
}

/// Builds a first-frame preview from an already-decoded first frame.
///
/// Separated from [`load_vqa_first_frame_preview`] so the streaming path can
/// reuse it without re-reading the file.
pub(crate) fn build_first_frame_preview(
    entry: &ContentCatalogEntry,
    first: super::vqa_stream::FirstVqaFrame,
    dither: bool,
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let rgba = rgba_frame_from_palette_indices(
        first.width as u32,
        first.height as u32,
        &first.frame.pixels,
        &first.frame.palette,
        false,
        dither,
    )?;

    let fps = first.fps;
    let has_audio = first.has_audio;

    Ok(PreparedContentPreview {
        label: format!("VQA preview (streaming): {}", entry.relative_path),
        details: vec![
            format!(
                "frames: {} at {}x{} pixels | fps: {}",
                first.num_frames, first.width, first.height, fps
            ),
            format!(
                "audio: {}",
                if has_audio {
                    "decoding in background"
                } else {
                    "none"
                }
            ),
            "First frame displayed instantly; remaining frames loading in background...".into(),
            preview_control_hint(true, has_audio),
        ],
        visual: Some(VisualPreview {
            frames: vec![rgba],
            frame_duration_seconds: Some(1.0 / (fps as f32)),
        }),
        audio: None,
        text_body: None,
    })
}

fn preview_control_hint(has_animation: bool, has_audio: bool) -> String {
    match (has_animation, has_audio) {
        (true, true) => {
            "controls: Space play/pause, Enter restart, ,/. step animation frames".into()
        }
        (true, false) => "controls: Space play/pause, ,/. step animation frames".into(),
        (false, true) => "controls: Space play/pause, Enter restart audio".into(),
        (false, false) => "controls: Up/Down to browse resources".into(),
    }
}

fn preferred_palette_names_for_visual(entry: &ContentCatalogEntry) -> &'static [&'static str] {
    let path_upper = entry.relative_path.to_ascii_uppercase();
    let name_upper = entry_file_name_upper(entry);
    if path_upper.contains("SNOW") {
        SNOW_PALETTE_NAMES
    } else if path_upper.contains("INTERIOR") {
        INTERIOR_PALETTE_NAMES
    } else if path_upper.contains("EGO") {
        EGO_PALETTE_NAMES
    } else if name_upper.starts_with("MSA")
        || name_upper.starts_with("MSS")
        || name_upper.starts_with("MAP")
    {
        // Mission-selection map animations (MSAB.WSA, MSSA.WSA, etc.)
        MAP_PALETTE_NAMES
    } else if name_upper.contains("SCORE")
        || name_upper.starts_with("HISC")
        || name_upper.starts_with("TIME")
        || name_upper.starts_with("BAR3")
        || name_upper.contains("MLTIPLYR")
        || name_upper.contains("TRAN")
        || name_upper.contains("MULTSCOR")
        || name_upper.starts_with("CREDS")
    {
        // Score/hiscore screens (HISCORE1, HISC1-HR, TIME, TIMEHR),
        // score bars (BAR3BLU, BAR3RHR, BAR3BHR, BAR3RED),
        // credits (CREDSA, CREDSAHR), multiplayer lobby, transitions.
        SCORE_PALETTE_NAMES
    } else {
        DEFAULT_PALETTE_NAMES
    }
}

pub(crate) fn entry_extension_lower(entry: &ContentCatalogEntry) -> Option<String> {
    let name = entry
        .location
        .logical_name()
        .unwrap_or(entry.relative_path.as_str())
        .replace('\\', "/");
    std::path::Path::new(&name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

fn entry_file_name_upper(entry: &ContentCatalogEntry) -> String {
    entry.location.file_name_upper()
}

/// Extracts the 256-color palette from the tail of a PCX file.
///
/// VGA PCX files store their palette in the last 769 bytes: a `0x0C` sentinel
/// followed by 768 bytes of 8-bit RGB data.  The returned `Vec<u8>` contains
/// 768 bytes in 6-bit VGA format (each channel right-shifted by 2) so the
/// result can be passed directly to [`Palette::parse`].
pub(crate) fn extract_pcx_palette(pcx_data: &[u8]) -> Option<Vec<u8>> {
    if pcx_data.len() < 769 {
        return None;
    }
    let tail = &pcx_data[pcx_data.len() - 769..];
    if tail[0] != 0x0C {
        return None;
    }
    // Convert 8-bit RGB → 6-bit VGA range expected by Palette::parse.
    Some(tail[1..].iter().map(|&b| b >> 2).collect())
}

/// Attempts to resolve a palette from a co-located PCX file for entries that
/// the RA1 engine renders with a PCX background palette (score screen, credits).
fn try_pcx_palette_for_visual(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Option<(Palette, String)> {
    let preferred = preferred_palette_names_for_visual(entry);
    if !std::ptr::eq(preferred, SCORE_PALETTE_NAMES) {
        return None;
    }
    for &pcx_name in SCORE_PCX_NAMES {
        let pcx_entry = catalogs
            .iter()
            .flat_map(|c| c.entries.iter())
            .find(|e| entry_file_name_upper(e) == pcx_name)?;
        if let Ok(pcx_bytes) = load_entry_bytes(pcx_entry) {
            if let Some(pal_bytes) = extract_pcx_palette(&pcx_bytes) {
                if let Ok(palette) = Palette::parse(pal_bytes) {
                    return Some((palette, format!("{} (embedded)", pcx_entry.relative_path)));
                }
            }
        }
    }
    None
}

fn trimmed_text_excerpt(text: &str) -> String {
    let mut excerpt_lines = Vec::new();
    let mut used_chars = 0usize;

    for line in text.lines().take(TEXT_PREVIEW_MAX_LINES) {
        if used_chars >= TEXT_PREVIEW_MAX_CHARS {
            break;
        }
        let remaining = TEXT_PREVIEW_MAX_CHARS.saturating_sub(used_chars);
        let clipped = line.chars().take(remaining).collect::<String>();
        used_chars = used_chars.saturating_add(clipped.chars().count());
        excerpt_lines.push(clipped);
    }

    if text.lines().count() > excerpt_lines.len() || text.len() > used_chars {
        excerpt_lines.push("...".into());
    }

    excerpt_lines.join("\n")
}
