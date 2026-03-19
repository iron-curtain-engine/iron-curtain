// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for `.vqa` wrapper metadata and loader registration.

use super::*;

/// Confirms that the loader claims the expected `.vqa` extension.
///
/// This protects the Bevy asset-routing contract for legacy video content.
#[test]
fn vqa_loader_extensions_match_expected_format() {
    let loader = VqaLoader;
    assert_eq!(loader.extensions(), &["vqa"]);
}

/// Proves that the wrapper preserves VQA header metadata and chunk summaries
/// while leaving the full parser view reopenable on demand.
///
/// The clip is encoded inline so the test documents the contract without an
/// opaque binary fixture.
#[test]
fn vqa_video_parses_header_and_chunk_directory() {
    let pixels = vec![0u8, 1, 2, 3, 4, 5, 6, 7];
    let palette = [0u8; 768];
    // Encoding a tiny synthetic clip keeps the proof deterministic and makes it
    // obvious where the expected metadata comes from.
    let bytes = cnc_vqa::encode_vqa(
        &[pixels],
        &palette,
        4,
        2,
        None,
        &cnc_vqa::VqaEncodeParams::default(),
    )
    .expect("test VQA should encode");

    let vqa = VqaVideo::parse(bytes).expect("encoded VQA should parse");

    assert_eq!(vqa.header.num_frames, 1);
    assert!(vqa.chunks.iter().any(|chunk| chunk.fourcc == *b"VQHD"));
    assert!(vqa.file().is_ok());
}
