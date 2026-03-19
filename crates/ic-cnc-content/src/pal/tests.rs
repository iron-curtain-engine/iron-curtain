// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for palette wrapper behavior and loader registration.

use super::*;

/// Confirms the loader claims the expected `.pal` extension.
///
/// A wrong extension list would break Bevy asset routing before any palette
/// parsing logic gets a chance to run.
#[test]
fn pal_loader_extensions_match_expected_format() {
    let loader = PalLoader;
    assert_eq!(loader.extensions(), &["pal"]);
}

/// Proves that a valid Westwood palette is decoded into renderer-friendly RGB
/// values by the wrapper.
///
/// The all-zero palette is used because the expected output is unambiguous and
/// keeps the test easy to audit.
#[test]
fn palette_parses_into_color_table() {
    let bytes = vec![0u8; cnc_pal::PALETTE_BYTES];
    let palette = Palette::parse(bytes).expect("valid PAL should parse");
    // The wrapper exposes an RGB table shape the renderer can consume directly
    // instead of forcing callers to interpret the file format themselves.
    assert_eq!(palette.to_rgb8_array()[0], [0, 0, 0]);
}

/// Proves that the palette wrapper exposes explicit import/render handoff
/// metadata instead of making later crates infer palette shape by convention.
#[test]
fn palette_reports_render_handoff_metadata() {
    let bytes = vec![0u8; cnc_pal::PALETTE_BYTES];
    let palette = Palette::parse(bytes).expect("valid PAL should parse");
    let handoff = palette.render_handoff();

    assert_eq!(handoff.color_count, cnc_pal::PALETTE_SIZE);
    assert_eq!(handoff.source_bytes, cnc_pal::PALETTE_BYTES);
    assert_eq!(handoff.colors[0], [0, 0, 0]);
    assert_eq!(handoff.colors[255], [0, 0, 0]);
}
