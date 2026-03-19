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

/// Builds a valid archive that stores two physical entries with the same CRC.
///
/// This is the edge case the importer cares about most: CRC lookup by filename
/// can only return one of the payloads, but archive extraction for IC-managed
/// storage must be able to preserve every physical SubBlock exactly as shipped.
fn duplicate_crc_mix_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    let first_payload = *b"AAAA";
    let second_payload = *b"BBBB";
    let crc = cnc_mix::crc("DUPLICATE.BIN").to_raw();

    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&8u32.to_le_bytes());

    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(first_payload.len() as u32).to_le_bytes());

    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes.extend_from_slice(&(first_payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(second_payload.len() as u32).to_le_bytes());

    bytes.extend_from_slice(&first_payload);
    bytes.extend_from_slice(&second_payload);
    bytes
}

/// Builds a malformed archive whose SubBlock points past the archive body.
///
/// The wrapper should surface the parser's rejection instead of trying to keep
/// a half-valid asset alive for the importer.
fn corrupt_mix_bytes_with_invalid_offset() -> Vec<u8> {
    let mut bytes = Vec::new();
    let payload = *b"DATA";

    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
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

/// Proves that the importer-facing extraction plan enumerates archive entries
/// with the metadata later import stages need.
///
/// `G1.2` is not only about raw payload bytes. The importer also needs stable
/// per-entry offsets, sizes, CRCs, and physical archive indices so it can
/// record provenance and retry individual extraction failures.
#[test]
fn mix_archive_builds_importer_entry_plan() {
    let archive = MixArchive::parse(test_mix_bytes()).expect("valid MIX should parse");
    let plan = archive
        .staged_entries()
        .expect("stored bytes should still reopen cleanly");

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].archive_index, 0);
    assert_eq!(plan[0].offset, 0);
    assert_eq!(plan[0].size, 4);
    assert_eq!(plan[0].logical_name, None);
}

/// Proves that extraction can preserve duplicate-CRC entries by physical index.
///
/// Filename lookup alone is insufficient here because binary search returns
/// only one of the duplicate entries. The importer must be able to stage every
/// physical payload without guessing which one the archive meant.
#[test]
fn mix_archive_extracts_duplicate_crc_entries_by_archive_index() {
    let archive =
        MixArchive::parse(duplicate_crc_mix_bytes()).expect("valid duplicate archive should parse");
    let staged = archive
        .extract_all_for_staging()
        .expect("duplicate physical entries should still stage");

    assert_eq!(staged.len(), 2);
    assert_eq!(staged[0].entry.archive_index, 0);
    assert_eq!(staged[0].bytes, b"AAAA");
    assert_eq!(staged[1].entry.archive_index, 1);
    assert_eq!(staged[1].bytes, b"BBBB");
}

/// Confirms that bad importer requests produce a specific staging error.
///
/// The importer will sometimes act on cached plan rows. If a stale row points
/// to a missing index, the wrapper should return a precise error instead of a
/// silent `None`.
#[test]
fn mix_archive_reports_out_of_range_staging_requests() {
    let archive = MixArchive::parse(test_mix_bytes()).expect("valid MIX should parse");
    let error = archive
        .extract_entry_for_staging(9)
        .expect_err("invalid staging index should fail");

    assert_eq!(
        error,
        MixStagingError::EntryOutOfRange {
            archive_index: 9,
            entry_count: 1,
        }
    );
}

/// Confirms that corrupt SubBlock offsets are rejected before staging begins.
///
/// This is the minimum corruption proof for `G1.2`: the importer never gets a
/// chance to write nonsense into IC-managed storage because invalid archive
/// layouts fail at parse/validation time.
#[test]
fn mix_archive_rejects_corrupt_offsets_before_staging() {
    let error = MixArchive::parse(corrupt_mix_bytes_with_invalid_offset())
        .expect_err("corrupt MIX should fail");

    assert!(
        matches!(error, cnc_formats::Error::InvalidOffset { .. }),
        "expected InvalidOffset, got: {error:?}",
    );
}
