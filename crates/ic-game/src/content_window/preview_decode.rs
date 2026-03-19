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
const DEFAULT_ANIMATION_FPS: f32 = 12.0;
const WAVEFORM_WIDTH: u32 = 512;
const WAVEFORM_HEIGHT: u32 = 160;
const TEXT_PREVIEW_MAX_LINES: usize = 18;
const TEXT_PREVIEW_MAX_CHARS: usize = 1_600;
const TMP_PREVIEW_MAX_TILES: usize = 16;
const TMP_PREVIEW_COLUMNS: usize = 4;

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
) -> Result<Option<PreparedContentPreview>, PreviewLoadError> {
    match entry_extension_lower(entry).as_deref() {
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
    }
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
    match &entry.location {
        ContentEntryLocation::Filesystem { absolute_path } => Ok(fs::read(absolute_path)?),
        ContentEntryLocation::MixMember {
            archive_path,
            archive_index,
            ..
        } => {
            let archive = ic_cnc_content::mix::MixArchive::parse(fs::read(archive_path)?)?;
            Ok(archive.extract_entry_for_staging(*archive_index)?.bytes)
        }
        ContentEntryLocation::MegMember {
            archive_path,
            archive_index,
            ..
        } => {
            let archive = ic_cnc_content::meg::MegArchive::parse(fs::read(archive_path)?)?;
            Ok(archive.extract_entry_for_staging(*archive_index)?.bytes)
        }
    }
}

fn load_shp_preview(
    entry: &ContentCatalogEntry,
    catalogs: &[ContentCatalog],
) -> Result<PreparedContentPreview, PreviewLoadError> {
    let bytes = load_entry_bytes(entry)?;
    let shp = ShpSprite::parse(bytes)?;
    let (palette, palette_label) =
        palette_for_indexed_visual(entry, catalogs, shp.embedded_palette.clone())?;
    let render_palette = PaletteTextureSource::from_handoff(palette.render_handoff());
    let sheet_handoff = shp.render_handoff();

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
    let stereo = aud_file.header.is_stereo();
    let channels = if stereo { 2u16 } else { 1u16 };
    let max_samples = (aud_file.header.uncompressed_size as usize) / 2;
    let samples = cnc_formats::aud::decode_adpcm(aud_file.compressed_data, stereo, max_samples);
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
                true,
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
            peak = peak.max(sample.abs() as i32);
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

fn rgba_frame_from_palette_indices(
    width: u32,
    height: u32,
    pixels: &[u8],
    palette_rgb8: &[u8; 768],
    transparent_zero: bool,
) -> Result<RgbaSpriteFrame, SpriteBootstrapError> {
    let mut rgba = Vec::with_capacity(
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4),
    );
    for &index in pixels {
        let base = (index as usize).saturating_mul(3);
        let r = palette_rgb8.get(base).copied().unwrap_or(0);
        let g = palette_rgb8.get(base + 1).copied().unwrap_or(0);
        let b = palette_rgb8.get(base + 2).copied().unwrap_or(0);
        let alpha = if transparent_zero && index == 0 {
            0
        } else {
            255
        };
        rgba.extend_from_slice(&[r, g, b, alpha]);
    }
    RgbaSpriteFrame::from_rgba(width, height, rgba)
}

fn audio_duration_seconds(sample_count: usize, sample_rate: u32, channels: u16) -> f32 {
    if sample_rate == 0 || channels == 0 {
        return 0.0;
    }
    (sample_count as f32) / (sample_rate as f32 * channels as f32)
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
    if path_upper.contains("SNOW") {
        SNOW_PALETTE_NAMES
    } else if path_upper.contains("INTERIOR") {
        INTERIOR_PALETTE_NAMES
    } else if path_upper.contains("EGO") {
        EGO_PALETTE_NAMES
    } else {
        DEFAULT_PALETTE_NAMES
    }
}

fn entry_extension_lower(entry: &ContentCatalogEntry) -> Option<String> {
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
