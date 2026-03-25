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

use std::io::Cursor;
use std::sync::mpsc::Sender;

use ic_cnc_content::cnc_formats::vqa::{VqaDecoder, VqaFrame, VqaFrameBuffer};
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
    pub frame: VqaFrame,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub num_frames: u16,
    pub has_audio: bool,
}

/// A batch of streamed VQA frames and/or audio sent from the background thread.
#[derive(Debug)]
pub(crate) struct VqaStreamBatch {
    /// RGBA frames decoded in this batch.
    pub frames: Vec<RgbaSpriteFrame>,
    /// Accumulated PCM audio samples (signed 16-bit, interleaved for stereo).
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
/// remaining frames in batches of ~15, converting each to RGBA on the fly.
/// Audio samples are collected alongside video and included in each batch.
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
    // Scratch buffer for reading audio samples between frames.
    let audio_scratch_size = if has_audio { 8192 } else { 0 };
    let mut audio_scratch = vec![0i16; audio_scratch_size];

    let mut batch_frames: Vec<RgbaSpriteFrame> = Vec::with_capacity(FRAMES_PER_BATCH);
    // All audio is accumulated here and sent only with the final batch.
    // `preview.rs` waits for `done = true` before constructing the PcmAudioSource
    // and uses `rotate_audio_to_video_position` to handle A/V phase alignment.
    let mut all_audio: Vec<i16> = Vec::new();
    let mut is_first_frame = true;

    loop {
        match decoder.next_frame_into(&mut frame_buffer) {
            Ok(Some(_index)) => {
                // Skip the first frame — already sent via the first-frame path.
                if is_first_frame {
                    is_first_frame = false;
                    // Drain audio eagerly: read_audio_samples calls pump_once
                    // until the scratch buffer fills, buffering all remaining
                    // video frames into frame_queue as a side effect.  Typically
                    // this single call reads the entire file's audio into
                    // all_audio; subsequent drain calls are no-ops.
                    if has_audio {
                        drain_audio(&mut decoder, &mut audio_scratch, &mut all_audio);
                    }
                    continue;
                }

                // Convert palette-indexed pixels to RGBA.
                if let Ok(rgba) = rgba_frame_from_palette_indices(
                    width as u32,
                    height as u32,
                    frame_buffer.pixels(),
                    frame_buffer.palette(),
                    false,
                ) {
                    batch_frames.push(rgba);
                }

                // Accumulate audio — it will be delivered in the final batch.
                if has_audio {
                    drain_audio(&mut decoder, &mut audio_scratch, &mut all_audio);
                }

                // Send a video-only batch when full.
                if batch_frames.len() >= FRAMES_PER_BATCH {
                    let batch = VqaStreamBatch {
                        frames: std::mem::take(&mut batch_frames),
                        audio_samples: Vec::new(),
                        audio_sample_rate: None,
                        audio_channels: None,
                        done: false,
                    };
                    if sender.send(batch).is_err() {
                        return Ok(()); // receiver dropped, selection changed
                    }
                }
            }
            Ok(None) => {
                if has_audio {
                    drain_audio(&mut decoder, &mut audio_scratch, &mut all_audio);
                }
                break;
            }
            Err(_) => break,
        }
    }

    // Final batch: remaining frames + all accumulated audio + metadata.
    let _ = sender.send(VqaStreamBatch {
        frames: batch_frames,
        audio_samples: all_audio,
        audio_sample_rate: sample_rate,
        audio_channels: channels,
        done: true,
    });

    Ok(())
}

/// Drains all currently queued audio samples from the decoder into `out`.
fn drain_audio<R: std::io::Read + std::io::Seek>(
    decoder: &mut VqaDecoder<R>,
    scratch: &mut [i16],
    out: &mut Vec<i16>,
) {
    loop {
        match decoder.read_audio_samples(scratch) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&scratch[..n]),
            Err(_) => break,
        }
    }
}
