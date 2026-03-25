// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Streaming VQA helpers for the content lab.
//!
//! `cnc-formats` ≥ 0.1.0-alpha.2 exposes [`VqaDecoder`] — an incremental,
//! frame-by-frame decoder backed by any `Read + Seek` source.  This module
//! provides the thin glue that the content lab's preview pipeline needs:
//!
//! - decode only the first frame from in-memory bytes for instant display
//! - stream all remaining frames + audio incrementally via a channel
//!
//! ## Memory model
//!
//! Streamed frames are sent as **palette-indexed pixels** (1 byte/pixel +
//! 768-byte palette snapshot), not pre-expanded RGBA.  This cuts per-frame
//! storage from `W×H×4` to `W×H+768` — a 4× reduction for 320×200 VQAs.
//! RGBA conversion happens lazily on the consumer side when the display
//! frame changes.

use std::io::Cursor;
use std::sync::mpsc::Sender;

use ic_cnc_content::cnc_formats::vqa::{VqaDecoder, VqaFrameBuffer};
use ic_render::sprite::RgbaSpriteFrame;

use super::preview_decode::rgba_frame_from_palette_indices;

/// Decodes only the first video frame from in-memory VQA bytes.
///
/// Opens an incremental `VqaDecoder` over the byte slice, decodes exactly one
/// frame, then drops the decoder.  This is the key enabler for instant video
/// playback: the first frame appears on screen while the full decode continues
/// on a background thread.
pub(crate) fn decode_vqa_first_frame(
    bytes: &[u8],
) -> Result<Option<FirstVqaFrame>, ic_cnc_content::cnc_formats::Error> {
    let cursor = Cursor::new(bytes);
    let mut decoder = VqaDecoder::open(cursor)?;
    let width = decoder.width();
    let height = decoder.height();
    let fps = decoder.fps();
    let num_frames = decoder.frame_count();
    let has_audio = decoder.has_audio();

    let frame = decoder.next_frame()?;
    Ok(frame.map(|decoded| FirstVqaFrame {
        frame: decoded.frame,
        width,
        height,
        fps,
        num_frames,
        has_audio,
    }))
}

/// The first decoded VQA frame together with header metadata needed to set up
/// the preview display surface.
pub(crate) struct FirstVqaFrame {
    pub frame: ic_cnc_content::cnc_formats::vqa::VqaFrame,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub num_frames: u16,
    pub has_audio: bool,
}

// ─── Indexed Frame ──────────────────────────────────────────────────────────

/// A palette-indexed video frame — 4× more compact than RGBA.
///
/// Stores `width × height` palette indices (1 byte each) plus a 768-byte
/// palette snapshot (256 RGB entries, 8-bit).  RGBA conversion is deferred
/// to display time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IndexedFrame {
    pub width: u32,
    pub height: u32,
    /// Palette-indexed pixels, row-major, `width × height` bytes.
    pub pixels: Vec<u8>,
    /// Active palette: 256 × (R, G, B) = 768 bytes, 8-bit values.
    pub palette: [u8; 768],
}

impl IndexedFrame {
    /// Convert to RGBA on demand for GPU upload.
    ///
    /// `dither` enables Bayer 4×4 ordered dithering, which breaks up
    /// gradient banding caused by the original 6-bit VGA palette.
    pub(crate) fn to_rgba(&self, dither: bool) -> Option<RgbaSpriteFrame> {
        rgba_frame_from_palette_indices(
            self.width,
            self.height,
            &self.pixels,
            &self.palette,
            false,
            dither,
        )
        .ok()
    }
}

// ─── Stream Batch ───────────────────────────────────────────────────────────

/// A batch of streamed VQA frames and/or audio sent from the background thread.
#[derive(Debug)]
pub(crate) struct VqaStreamBatch {
    /// Palette-indexed frames decoded in this batch (compact: 1 byte/pixel).
    pub frames: Vec<IndexedFrame>,
    /// PCM audio samples decoded alongside video in this batch
    /// (signed 16-bit, interleaved for stereo).
    pub audio_samples: Vec<i16>,
    /// Audio sample rate, set on the first batch that carries audio.
    pub audio_sample_rate: Option<u32>,
    /// Audio channel count, set on the first batch that carries audio.
    pub audio_channels: Option<u16>,
    /// `true` when the decoder has finished — no more batches will follow.
    pub done: bool,
}

/// How many frames to decode per batch before sending through the channel.
const FRAMES_PER_BATCH: usize = 15;

/// Audio post-processing options applied after decoding each batch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AudioPostProcess {
    /// Apply TPDF 1-LSB dither to whiten IMA ADPCM quantisation noise.
    pub dither: bool,
    /// Apply a single-pole IIR high-pass filter to remove DC offset.
    pub dc_correction: bool,
}

impl Default for AudioPostProcess {
    fn default() -> Self {
        Self { dither: true, dc_correction: true }
    }
}

/// Streams all VQA frames and audio from in-memory bytes through a channel.
///
/// The caller should have already decoded + sent frame 0 via the first-frame
/// path. This function opens a fresh decoder, skips frame 0, and streams the
/// remaining frames in batches of ~15.
///
/// Frames are sent as **palette-indexed** data (not RGBA) so the consumer
/// stores them at 1 byte/pixel.  RGBA conversion happens lazily on the
/// display side.
///
/// The final batch has `done = true`.
pub(crate) fn stream_vqa_decode(
    bytes: Vec<u8>,
    sender: Sender<VqaStreamBatch>,
    post: AudioPostProcess,
) -> Result<(), ic_cnc_content::cnc_formats::Error> {
    let cursor = Cursor::new(bytes);
    let mut decoder = VqaDecoder::open(cursor)?;
    let width = decoder.width();
    let height = decoder.height();
    let has_audio = decoder.has_audio();
    let sample_rate = decoder.audio_sample_rate().map(|sr| sr as u32);
    let channels = if has_audio {
        Some(if decoder.audio_channels().unwrap_or(1) == 0 {
            1u16
        } else {
            decoder.audio_channels().unwrap_or(1) as u16
        })
    } else {
        None
    };

    let mut frame_buffer = VqaFrameBuffer::new(width, height);
    // Scratch buffer for reading queued audio samples between frames.
    let audio_scratch_size = if has_audio { 8192 } else { 0 };
    let mut audio_scratch = vec![0i16; audio_scratch_size];

    // Audio post-processing state — persisted across batches so the HP filter
    // and PRNG carry state continuously through the full stream.
    let mut rng_state: u64 = 0xDEAD_C0DE_1337_CAFE;
    let mut hp_state = [0i32; 2];

    let mut batch_frames: Vec<IndexedFrame> = Vec::with_capacity(FRAMES_PER_BATCH);
    let mut batch_audio: Vec<i16> = Vec::new();
    let mut is_first_frame = true;

    loop {
        match decoder.next_frame_into(&mut frame_buffer) {
            Ok(Some(_index)) => {
                // Skip the first frame — already sent via the first-frame path.
                if is_first_frame {
                    is_first_frame = false;
                    if has_audio {
                        drain_queued_audio(&mut decoder, &mut audio_scratch, &mut batch_audio);
                    }
                    continue;
                }

                // Store as compact palette-indexed frame (not RGBA).
                batch_frames.push(IndexedFrame {
                    width: width as u32,
                    height: height as u32,
                    pixels: frame_buffer.pixels().to_vec(),
                    palette: *frame_buffer.palette(),
                });

                // Read any audio that was queued as a side-effect of
                // next_frame_into — no extra pumping, bounded memory.
                if has_audio {
                    drain_queued_audio(&mut decoder, &mut audio_scratch, &mut batch_audio);
                }

                // Send batch when full (video + audio collected so far).
                if batch_frames.len() >= FRAMES_PER_BATCH {
                    apply_audio_post_process(&post, &mut rng_state, &mut hp_state, &mut batch_audio);
                    let batch = VqaStreamBatch {
                        frames: std::mem::take(&mut batch_frames),
                        audio_samples: std::mem::take(&mut batch_audio),
                        audio_sample_rate: sample_rate,
                        audio_channels: channels,
                        done: false,
                    };
                    if sender.send(batch).is_err() {
                        return Ok(()); // receiver dropped, selection changed
                    }
                }
            }
            Ok(None) => {
                // Final drain of any remaining queued audio.
                if has_audio {
                    drain_queued_audio(&mut decoder, &mut audio_scratch, &mut batch_audio);
                }
                break;
            }
            Err(_) => break,
        }
    }

    // Final batch: remaining frames + remaining audio + metadata.
    apply_audio_post_process(&post, &mut rng_state, &mut hp_state, &mut batch_audio);
    let _ = sender.send(VqaStreamBatch {
        frames: batch_frames,
        audio_samples: batch_audio,
        audio_sample_rate: sample_rate,
        audio_channels: channels,
        done: true,
    });

    Ok(())
}

/// Applies enabled post-processing steps to a mutable slice of PCM samples.
///
/// DC correction runs first (removes low-frequency bias), then dither
/// (adds whitened noise to mask quantisation artefacts).  Both are no-ops on
/// empty slices so it is safe to call unconditionally.
fn apply_audio_post_process(
    post: &AudioPostProcess,
    rng: &mut u64,
    hp_state: &mut [i32; 2],
    samples: &mut Vec<i16>,
) {
    if samples.is_empty() {
        return;
    }
    if post.dc_correction {
        hp_filter(hp_state, samples);
    }
    if post.dither {
        tpdf_dither(rng, samples);
    }
}

/// Reads only already-queued audio samples from the decoder into `out`.
///
/// Uses `read_queued_audio_samples` which does NOT call `pump_once()`,
/// so no additional video frames are decoded and buffered internally.
fn drain_queued_audio<R: std::io::Read + std::io::Seek>(
    decoder: &mut VqaDecoder<R>,
    scratch: &mut [i16],
    out: &mut Vec<i16>,
) {
    loop {
        match decoder.read_queued_audio_samples(scratch) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&scratch[..n]),
            Err(_) => break,
        }
    }
}

// ─── Audio post-processing ───────────────────────────────────────────────────

/// TPDF (Triangular Probability Density Function) dither applied to the
/// decoded PCM output.
///
/// IMA ADPCM's 4-bit quantisation produces correlated error harmonics audible
/// as a faint buzz or distortion.  Adding 1 LSB of triangular-distribution
/// noise whitens the noise floor.  The dither magnitude is exactly ±1 LSB of
/// signed 16-bit PCM — inaudible on modern playback hardware but sufficient to
/// de-correlate quantisation artefacts.
///
/// Uses a cheap xorshift64 PRNG; period ~2^64, no allocation, no branches.
pub(crate) fn tpdf_dither(rng: &mut u64, samples: &mut [i16]) {
    for s in samples.iter_mut() {
        // Two independent uniform LSB-wide random bits → triangular in [-1, 0, +1].
        let r = xorshift64(rng);
        let r1 = (r & 1) as i16;
        let r2 = ((r >> 1) & 1) as i16;
        *s = s.saturating_add(r1 - r2);
    }
}

/// Single-pole IIR high-pass filter — removes DC offset from the PCM stream.
///
/// SND1 audio can carry a low-frequency DC offset that causes a brief pop at
/// stream start.  This 1-pole filter (α ≈ 0.992, corner ≈ 28 Hz at 22 kHz)
/// suppresses DC within ~1000 samples while passing all audible content.
///
/// Fixed-point 256-scaled arithmetic: no `f32`, safe on all targets.
pub(crate) fn hp_filter(state: &mut [i32; 2], samples: &mut [i16]) {
    // Fixed-point 256-scaled accumulator.
    // state[0] = x_prev scaled by 256; state[1] = y_acc scaled by 256.
    // alpha = 1 - 1/128 ≈ 0.992  →  corner ≈ 28 Hz at 22 kHz.
    // DC decays to zero output within ~1000 samples; AC passes unattenuated.
    for s in samples.iter_mut() {
        let x256 = (*s as i32) << 8;
        let y256 = x256 - state[0] + state[1] - (state[1] >> 7);
        state[0] = x256;
        state[1] = y256;
        *s = (y256 >> 8).clamp(-32768, 32767) as i16;
    }
}

#[inline]
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{hp_filter, tpdf_dither};

    /// TPDF dither must stay within ±1 LSB of the original sample value.
    #[test]
    fn tpdf_dither_stays_within_one_lsb() {
        let originals: Vec<i16> = vec![0, 1000, -1000, i16::MAX - 1, i16::MIN + 1, 128, -128];
        let mut rng = 0xDEAD_BEEF_1234_5678u64;
        for &orig in &originals {
            let mut buf = [orig];
            tpdf_dither(&mut rng, &mut buf);
            let diff = (buf[0] as i32 - orig as i32).abs();
            assert!(
                diff <= 1,
                "tpdf_dither must stay within ±1 LSB: original={orig}, dithered={}, diff={diff}",
                buf[0]
            );
        }
    }

    /// TPDF dither must not overflow at i16 boundary values.
    #[test]
    fn tpdf_dither_no_overflow_at_boundaries() {
        let mut rng = 0xCAFE_BABE_0000_0001u64;
        // Run many iterations at the extreme values to exercise saturating_add.
        let mut buf_max = [i16::MAX; 256];
        let mut buf_min = [i16::MIN; 256];
        tpdf_dither(&mut rng, &mut buf_max);
        tpdf_dither(&mut rng, &mut buf_min);
        assert!(buf_max.iter().all(|&s| s >= i16::MAX - 1));
        assert!(buf_min.iter().all(|&s| s <= i16::MIN + 1));
    }

    /// HP filter must suppress DC within a few hundred samples.
    ///
    /// A constant signal of 1000 (representing SND1 mid-scale DC) should
    /// decay toward 0 after the filter settles.
    #[test]
    fn hp_filter_removes_dc_offset() {
        let mut state = [0i32; 2];
        let dc_in = vec![1000i16; 2048];
        let mut out = dc_in.clone();
        hp_filter(&mut state, &mut out);

        // After 2000 samples the output should be very close to 0.
        let tail = out.get(1900..).unwrap_or(&[]);
        assert!(
            tail.iter().all(|&s| s.abs() < 5),
            "HP filter must suppress DC: tail samples = {:?}",
            &tail[..tail.len().min(8)]
        );
    }

    /// HP filter must pass a 1 kHz tone without significant attenuation.
    ///
    /// Generates a 1 kHz square-ish approximation (alternating +1000/-1000)
    /// and verifies the filter preserves amplitude above 90%.
    #[test]
    fn hp_filter_passes_ac_signal() {
        let mut state = [0i32; 2];
        // 22 samples per half-period ≈ 500 Hz at 22050 Hz — well above the
        // ~7 Hz HP corner frequency, so attenuation should be negligible.
        let ac_in: Vec<i16> = (0..2048).map(|i| if (i / 22) & 1 == 0 { 1000 } else { -1000 }).collect();
        let mut out = ac_in.clone();
        hp_filter(&mut state, &mut out);

        // Skip first few cycles for filter settling, then check amplitude.
        let settled = out.get(200..).unwrap_or(&[]);
        let max_amp = settled.iter().map(|&s| s.abs()).max().unwrap_or(0);
        assert!(
            max_amp >= 900,
            "HP filter must pass AC signal with >90% amplitude, got max={max_amp}"
        );
    }
}
