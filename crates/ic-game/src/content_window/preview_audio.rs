// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Windows-only Bevy audio bridge for decoded PCM previews.
//!
//! `preview_decode` deliberately stops at "decoded samples plus metadata" so
//! it stays testable without Bevy. This module owns the next step for the
//! content lab on Windows builds: expose those samples as a Bevy audio asset
//! that can be played by `AudioPlayer<T>`.
//!
//! Bevy's built-in `AudioSource` expects encoded file bytes such as WAV or OGG.
//! For `.aud` and VQA audio we already have decoded PCM samples, so wrapping
//! them back into a synthetic WAV file would add avoidable work and obscure the
//! real runtime path. `PcmAudioSource` keeps the content lab honest by letting
//! Bevy play the samples directly.

use std::sync::Arc;
use std::time::Duration;

use bevy::asset::Asset;
use bevy::audio::{AddAudioSource, Decodable, Source};
use bevy::prelude::App;
use bevy::reflect::TypePath;

use super::preview_decode::AudioPreview;

/// Registers the content-lab PCM source with the Bevy app.
///
/// Bevy only knows how to spawn `AudioPlayer<T>` entities for types that were
/// explicitly registered as decodable assets. This keeps the registration local
/// to the content-lab client instead of pretending raw PCM is a global engine
/// asset type already.
pub(crate) fn register_preview_audio_source(app: &mut App) {
    app.add_audio_source::<PcmAudioSource>();
}

/// In-memory PCM clip that Bevy can play through `AudioPlayer<PcmAudioSource>`.
///
/// The sample buffer is shared with an `Arc` so the content preview metadata
/// and each new playback decoder can reference the same decoded clip without
/// copying the full audio payload.
#[derive(Asset, TypePath, Debug, Clone)]
pub(crate) struct PcmAudioSource {
    samples: Arc<[i16]>,
    sample_rate: u32,
    channels: u16,
    duration: Duration,
}

impl PcmAudioSource {
    /// Builds the runtime audio asset from a decoded preview payload.
    pub(crate) fn from_preview_audio(audio: &AudioPreview) -> Self {
        Self::new(
            Arc::clone(&audio.pcm_samples),
            audio.sample_rate(),
            audio.channels(),
        )
    }

    pub(crate) fn new(samples: Arc<[i16]>, sample_rate: u32, channels: u16) -> Self {
        assert!(sample_rate != 0, "preview PCM sample rate must not be zero");
        assert!(channels != 0, "preview PCM channel count must not be zero");

        let duration_ns = 1_000_000_000u64
            .saturating_mul(samples.len() as u64)
            .checked_div(sample_rate as u64)
            .unwrap_or(0)
            .checked_div(channels as u64)
            .unwrap_or(0);
        let duration = Duration::new(
            duration_ns / 1_000_000_000,
            (duration_ns % 1_000_000_000) as u32,
        );

        Self {
            samples,
            sample_rate,
            channels,
            duration,
        }
    }
}

impl Decodable for PcmAudioSource {
    type DecoderItem = i16;
    type Decoder = PcmAudioDecoder;

    fn decoder(&self) -> Self::Decoder {
        PcmAudioDecoder {
            samples: Arc::clone(&self.samples),
            position: 0,
            sample_rate: self.sample_rate,
            channels: self.channels,
            duration: self.duration,
        }
    }
}

/// Iterator/source wrapper Bevy hands to rodio for playback.
///
/// The decoder is intentionally tiny because the expensive work already
/// happened in `preview_decode`: it only walks an interleaved PCM slice and
/// reports the clip metadata rodio needs for timing and channel layout.
#[derive(Debug, Clone)]
pub(crate) struct PcmAudioDecoder {
    samples: Arc<[i16]>,
    position: usize,
    sample_rate: u32,
    channels: u16,
    duration: Duration,
}

impl Iterator for PcmAudioDecoder {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.samples.get(self.position).copied()?;
        self.position += 1;
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.samples.len().saturating_sub(self.position);
        (remaining, Some(remaining))
    }
}

impl Source for PcmAudioDecoder {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.duration)
    }
}
