// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for `.shp` loader registration and wrapper decoding behavior.

use super::*;

/// Confirms that the loader claims the expected `.shp` extension.
///
/// This guards the asset-system registration contract for sprite sheets.
#[test]
fn shp_loader_extensions_match_expected_format() {
    let loader = ShpLoader;
    assert_eq!(loader.extensions(), &["shp"]);
}

/// Proves that the wrapper preserves SHP header metadata and delegates frame
/// decoding to `cnc-formats`.
///
/// The fixture is encoded inline so the contract remains readable without
/// external asset files.
#[test]
fn shp_sprite_parses_header_and_decodes_frames() {
    let pixels = [1u8, 2, 3, 4];
    // The parser crate can encode a tiny synthetic SHP, which keeps this test
    // deterministic and avoids checking in an opaque binary fixture.
    let bytes = cnc_shp::encode_frames(&[pixels.as_slice()], 2, 2).expect("test SHP should encode");
    let shp = ShpSprite::parse(bytes).expect("encoded SHP should parse");

    assert_eq!(shp.header.frame_count, 1);
    assert_eq!(shp.header.width, 2);
    assert_eq!(shp.header.height, 2);
    assert_eq!(
        shp.decode_frames().expect("stored bytes stay parseable"),
        vec![pixels.to_vec()]
    );
}

/// Proves that the wrapper exposes an explicit render-handoff summary for the
/// future render crate.
///
/// `G1.3` is the bridge from parsing to rendering: the render crate should be
/// able to ask for dimensions, frame counts, and embedded-palette presence
/// without depending on the raw SHP header layout.
#[test]
fn shp_sprite_reports_render_handoff_metadata() {
    let frame_a = [1u8, 2, 3, 4];
    let frame_b = [5u8, 6, 7, 8];
    let bytes = cnc_shp::encode_frames(&[frame_a.as_slice(), frame_b.as_slice()], 2, 2)
        .expect("test SHP should encode");
    let shp = ShpSprite::parse(bytes).expect("encoded SHP should parse");
    let handoff = shp.render_handoff();

    assert_eq!(handoff.width, 2);
    assert_eq!(handoff.height, 2);
    assert_eq!(handoff.frame_count, 2);
    assert!(!handoff.has_embedded_palette);
    assert_eq!(handoff.frames.len(), 2);
    assert_eq!(handoff.frames[0].frame_index, 0);
    assert_eq!(handoff.frames[0], shp.frames[0]);
}
