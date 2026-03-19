// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for MIX archive wrapping and lookup behavior.

use super::*;

/// Builds the smallest valid single-entry `.mix` archive used by wrapper tests.
///
/// The fixture is generated inline so the binary layout stays readable without
/// an opaque external asset:
/// - 2 bytes: file count
/// - 4 bytes: payload size of the archive body
/// - 12 bytes: one directory entry (`crc`, `offset`, `size`)
/// - N bytes: the file payload itself
///
/// Westwood MIX stores directory entries before the archive body, so the test
/// sets the entry offset to `0`: offsets are relative to the start of the body,
/// not to the start of the whole file.
fn test_mix_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    let payload = [0x11, 0x22, 0x33, 0x44];
    let crc = cnc_mix::crc("TEST.BIN").to_raw();

    // MIX header: one file and a four-byte archive body.
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());

    // Directory entry for TEST.BIN.
    // The payload starts at the beginning of the archive body, so its relative
    // body offset is zero.
    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());

    // Archive body: the single file payload.
    bytes.extend_from_slice(&payload);
    bytes
}

/// Confirms that the Bevy loader claims the expected `.mix` extension.
///
/// This protects the asset-routing contract for archive loads before any
/// format parsing happens.
#[test]
fn mix_loader_extensions_match_expected_format() {
    let loader = MixLoader;
    assert_eq!(loader.extensions(), &["mix"]);
}

/// Proves that the wrapper preserves both directory metadata and filename-based
/// payload lookup from `cnc-formats`.
///
/// This is the core smoke test for G1 archive integration because later import
/// work depends on both listing files and extracting their bytes.
#[test]
fn mix_archive_parses_and_exposes_entries() {
    let archive = MixArchive::parse(test_mix_bytes()).expect("valid MIX should parse");

    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries().len(), 1);
    // `get()` is the engine-facing convenience API: later systems should not
    // need to know MIX directory details just to extract one file.
    assert_eq!(
        archive
            .get("TEST.BIN")
            .expect("stored bytes stay parseable"),
        Some(vec![0x11, 0x22, 0x33, 0x44])
    );
}
