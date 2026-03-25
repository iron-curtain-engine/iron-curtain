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
    let _ = sender.send(VqaStreamBatch {
        frames: batch_frames,
        audio_samples: batch_audio,
        audio_sample_rate: sample_rate,
        audio_channels: channels,
        done: true,
    });

    Ok(())
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
