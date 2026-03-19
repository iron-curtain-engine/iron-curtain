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
