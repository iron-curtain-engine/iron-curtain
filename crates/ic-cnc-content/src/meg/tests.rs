// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for MEG archive wrapping and staging behavior.

use super::*;

/// Builds the smallest useful legacy-format `.meg` archive for wrapper tests.
///
/// The fixture uses the classic Petroglyph layout:
/// - 8-byte header (`num_filenames`, `num_files`)
/// - length-prefixed filename table
/// - 20-byte file record table
/// - concatenated payload bytes
///
/// This test helper stays inline because the whole point is to make the
/// archive layout readable in isolation, not hide it behind a binary blob.
fn test_meg_bytes() -> Vec<u8> {
    build_meg(&[("DATA/ART/UNITS/1TNK.SHP", b"MEG!")])
}

/// Builds a simple legacy-format MEG archive from `(filename, data)` pairs.
fn build_meg(files: &[(&str, &[u8])]) -> Vec<u8> {
    const FILE_RECORD_SIZE: usize = 20;

    let count = files.len() as u32;
    let mut bytes = Vec::new();

    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());

    for (name, _) in files {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }

    let data_start = bytes.len() + files.len() * FILE_RECORD_SIZE;
    let mut offsets = Vec::with_capacity(files.len());
    let mut current_offset = data_start as u32;
    for (_, data) in files {
        offsets.push(current_offset);
        current_offset = current_offset.saturating_add(data.len() as u32);
    }

    for (index, (_, data)) in files.iter().enumerate() {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(index as u32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&offsets[index].to_le_bytes());
        bytes.extend_from_slice(&(index as u32).to_le_bytes());
    }

    for (_, data) in files {
        bytes.extend_from_slice(data);
    }

    bytes
}

/// Confirms that the Bevy loader claims both supported Petroglyph extensions.
///
/// `.meg` is the common Remastered archive extension and `.pgm` reuses the
/// same container format for packaged maps.
#[test]
fn meg_loader_extensions_match_expected_formats() {
    let loader = MegLoader;
    assert_eq!(loader.extensions(), &["meg", "pgm"]);
}

/// Proves that the wrapper preserves directory metadata and filename-based
/// payload lookup from the clean-room parser.
#[test]
fn meg_archive_parses_and_exposes_entries() {
    let archive = MegArchive::parse(test_meg_bytes()).expect("valid MEG should parse");

    assert_eq!(archive.file_count(), 1);
    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.entries()[0].name, "DATA/ART/UNITS/1TNK.SHP");
    assert_eq!(
        archive
            .get("data/art/units/1tnk.shp")
            .expect("MEG lookup should stay case-insensitive"),
        Some(b"MEG!".to_vec())
    );
}

/// Proves that importer staging can enumerate and extract MEG members by
/// physical archive index.
///
/// Unlike classic MIX files, MEG stores real filenames on disk, so the staged
/// entry surface must preserve both the stable physical index and the original
/// logical name.
#[test]
fn meg_archive_builds_importer_entry_plan_and_extracts_payload() {
    let archive = MegArchive::parse(build_meg(&[
        ("DATA/ART/UNITS/1TNK.SHP", b"SHP!"),
        ("DATA/ART/PALETTES/TEMPERAT.PAL", b"PAL!"),
    ]))
    .expect("valid MEG should parse");

    let plan = archive
        .staged_entries()
        .expect("stored bytes should still reopen cleanly");
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].archive_index, 0);
    assert_eq!(plan[0].logical_name, "DATA/ART/UNITS/1TNK.SHP");
    assert_eq!(plan[1].archive_index, 1);
    assert_eq!(plan[1].logical_name, "DATA/ART/PALETTES/TEMPERAT.PAL");

    let staged = archive
        .extract_entry_for_staging(1)
        .expect("valid archive index should extract");
    assert_eq!(staged.entry.archive_index, 1);
    assert_eq!(staged.entry.logical_name, "DATA/ART/PALETTES/TEMPERAT.PAL");
    assert_eq!(staged.bytes, b"PAL!");
}

/// Confirms that stale importer rows fail with a specific out-of-range error
/// instead of silently returning no payload.
#[test]
fn meg_archive_reports_out_of_range_staging_requests() {
    let archive = MegArchive::parse(test_meg_bytes()).expect("valid MEG should parse");
    let error = archive
        .extract_entry_for_staging(9)
        .expect_err("invalid staging index should fail");

    assert_eq!(
        error,
        MegStagingError::EntryOutOfRange {
            archive_index: 9,
            entry_count: 1,
        }
    );
}
