// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the first real-content browser bootstrap.

use super::*;
use ic_cnc_content::source::{ContentSourceKind, SourceRightsClass};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::content_window::gallery::ContainedImageSize;
use crate::content_window::state::GALLERY_VISIBLE_SLOTS;

/// Proves that scanning a directory source catalogs supported RA-style files,
/// records deterministic relative paths, and summarizes family/support counts.
///
/// The fixture is created in a temporary directory so the test does not depend
/// on machine-specific Red Alert installs.
#[test]
fn directory_source_scan_indexes_entries_and_counts_by_family() {
    let fixture = TestDir::new("content_window_catalog");
    fixture.write_file("MAIN.MIX", b"mix");
    fixture.write_file("MOVIES/SIZZLE.VQA", b"vqa");
    fixture.write_file("PALETTES/TEMPERAT.PAL", b"pal");
    fixture.write_file("RULES/SCG01EA.INI", b"ini");

    let source = ContentSourceRoot::directory(
        "Fixture CD",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);

    assert!(catalog.available);
    assert_eq!(catalog.entries.len(), 4);
    assert_eq!(
        catalog.entry_count_for_family(ContentFamily::WestwoodArchive),
        1
    );
    assert_eq!(catalog.entry_count_for_family(ContentFamily::Video), 1);
    assert_eq!(catalog.entry_count_for_family(ContentFamily::Palette), 1);
    assert_eq!(catalog.entry_count_for_family(ContentFamily::Config), 1);
    assert_eq!(
        catalog.entry_count_for_support(ContentSupportLevel::SupportedNow),
        4
    );
    assert_eq!(
        catalog.entry_count_for_support(ContentSupportLevel::Planned),
        0
    );
    assert_eq!(catalog.entries[0].relative_path, "MAIN.MIX");
    assert_eq!(catalog.entries[1].relative_path, "MOVIES/SIZZLE.VQA");
}

/// Proves that a single-file source such as the provided `.rar` is still shown
/// in the content window instead of being silently ignored.
#[test]
fn single_file_source_scan_creates_one_archive_entry() {
    let fixture = TestDir::new("content_window_archive");
    let archive_path = fixture.write_file("RedAlert1_AlliedDisc.rar", b"rar");

    let source = ContentSourceRoot::single_file(
        "Fixture Archive",
        archive_path,
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);

    assert!(catalog.available);
    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(catalog.entries[0].family, ContentFamily::ExternalArchive);
    assert_eq!(
        catalog.entries[0].support,
        ContentSupportLevel::ExternalOnly,
    );
}

/// Proves that scanning a directory with a real `.mix` archive mounts the
/// archive members as normal catalog entries instead of stopping at the outer
/// container file.
///
/// The fixture uses filenames from the built-in MIX name corpus so the content
/// lab can recover stable logical names even though classic MIX archives store
/// only CRC directory keys on disk.
#[test]
fn directory_source_scan_mounts_mix_members_with_builtin_names() {
    let fixture = TestDir::new("content_window_mix_mount");
    fixture.write_file(
        "MAIN.MIX",
        &build_named_mix(&[
            ("1TNK.SHP", &test_shp_bytes()),
            (
                "TEMPERAT.PAL",
                &test_palette_bytes([40, 42, 44], [50, 52, 54]),
            ),
        ]),
    );

    let source = ContentSourceRoot::directory(
        "Fixture MIX Root",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);

    assert!(catalog.available);
    assert_eq!(
        catalog.entry_count_for_family(ContentFamily::WestwoodArchive),
        1
    );
    assert_eq!(
        catalog.entry_count_for_family(ContentFamily::SpriteSheet),
        1
    );
    assert_eq!(catalog.entry_count_for_family(ContentFamily::Palette), 1);
    assert!(
        catalog
            .entries
            .iter()
            .any(|entry| entry.relative_path == "MAIN.MIX::1TNK.SHP"),
        "the mounted MIX sprite member should be visible as a logical catalog entry",
    );
    assert!(
        catalog
            .entries
            .iter()
            .any(|entry| entry.relative_path == "MAIN.MIX::TEMPERAT.PAL"),
        "the mounted MIX palette member should be visible as a logical catalog entry",
    );
}

/// Proves that Remastered `.meg` archives also mount their logical filenames
/// into the same catalog surface the content lab uses for loose files.
#[test]
fn directory_source_scan_mounts_meg_members_with_logical_filenames() {
    let fixture = TestDir::new("content_window_meg_mount");
    fixture.write_file(
        "REDALERT.MEG",
        &build_meg(&[(
            "DATA/ART/PALETTES/TEMPERAT.PAL",
            &test_palette_bytes([12, 24, 36], [40, 20, 10]),
        )]),
    );

    let source = ContentSourceRoot::directory(
        "Fixture MEG Root",
        fixture.path().to_path_buf(),
        ContentSourceKind::Steam,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);

    assert!(catalog.available);
    assert_eq!(
        catalog.entry_count_for_family(ContentFamily::RemasteredArchive),
        1
    );
    assert!(
        catalog
            .entries
            .iter()
            .any(|entry| entry.relative_path == "REDALERT.MEG::DATA/ART/PALETTES/TEMPERAT.PAL"),
        "the mounted MEG member should keep its logical filename in the catalog",
    );
}

/// Proves that the text formatter exposes the source label, counts, and the
/// selected gallery-entry details the content lab needs to display.
#[test]
fn content_window_text_reports_selected_source_and_entry() {
    let fixture = TestDir::new("content_window_text");
    fixture.write_file("MAIN.MIX", b"mix");
    fixture.write_file("README.TXT", b"txt");
    fixture.write_file(
        "TEMPERAT.PAL",
        &test_palette_bytes([12, 24, 36], [40, 20, 10]),
    );

    let source = ContentSourceRoot::directory(
        "Fixture CD",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);
    let state = ContentLabState::new(vec![catalog]);

    let text = state.render_text();

    assert!(text.contains("Fixture CD"));
    assert!(text.contains("supported now"));
    assert!(text.contains("TEMPERAT.PAL"));
    assert!(text.contains("Preview:"));
}

/// Proves that the content-lab window uses a desktop-safe fallback size before
/// Bevy's fullscreen mode claims the monitor.
///
/// The fallback size is still kept as a safe default for platforms that may
/// briefly construct the window before the fullscreen request is honored.
#[test]
fn content_window_starts_in_borderless_fullscreen_mode() {
    let window = content_lab_window();

    assert_eq!(window.title, "Iron Curtain - Content Lab");
    assert_eq!(window.resolution.physical_width(), 1280);
    assert_eq!(window.resolution.physical_height(), 720);
    assert_eq!(
        window.mode,
        WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
    );
    assert!(!window.resizable);
}

/// Proves that the fullscreen content lab does not exit on a single accidental
/// `Esc` press.
///
/// The helper tracks the first press timestamp and only returns `true` once a
/// second press arrives inside the allowed confirmation window.
#[test]
fn escape_exit_shortcut_requires_two_presses() {
    let mut shortcut = super::state::EscapeExitShortcut::default();

    assert!(!shortcut.register_press(1.0));
    assert!(shortcut.register_press(1.5));
}

/// Proves that stale `Esc` presses do not keep an exit armed forever.
///
/// If the second press arrives after the confirmation timeout, the shortcut
/// should treat it as a new first press and require another confirmation.
#[test]
fn escape_exit_shortcut_resets_after_timeout() {
    let mut shortcut = super::state::EscapeExitShortcut::default();
    let timeout = super::state::ESCAPE_EXIT_CONFIRMATION_WINDOW_SECS;

    assert!(!shortcut.register_press(5.0));
    assert!(!shortcut.register_press(5.0 + timeout + 0.1));
    assert!(shortcut.register_press(5.0 + timeout + 0.6));
}

/// Proves that content-lab startup lands on the first previewable entry
/// instead of making the user scroll past archives before anything real can be
/// rendered.
#[test]
fn content_lab_prefers_the_first_previewable_entry_on_startup() {
    let fixture = TestDir::new("content_window_initial_preview");
    fixture.write_file("MAIN.MIX", b"mix");
    fixture.write_file("CONQUER_OUTPUT/1TNK.SHP", &test_shp_bytes());
    fixture.write_file("README.TXT", b"txt");

    let source = ContentSourceRoot::directory(
        "Fixture CD",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let state = ContentLabState::new(vec![ContentCatalog::scan(source)]);

    assert_eq!(state.selected_catalog_index(), 0);
    assert_eq!(state.selected_entry_index(), 0);
    assert_eq!(
        state
            .selected_entry()
            .expect("the previewable entry should exist")
            .relative_path,
        "CONQUER_OUTPUT/1TNK.SHP"
    );
}

/// Proves that gallery navigation skips catalog entries that have no visual
/// thumbnail surface.
///
/// The content lab now uses one on-screen grid as the primary browse surface,
/// so arrow-key navigation must land only on entries that can actually occupy a
/// gallery slot instead of stopping on raw archive containers or pure text
/// files.
#[test]
fn gallery_navigation_skips_non_visual_entries() {
    let fixture = TestDir::new("gallery_skip_non_visual");
    fixture.write_file("README.TXT", b"text");
    fixture.write_file("1TNK.SHP", b"shp");
    fixture.write_file("RULES.INI", b"ini");
    fixture.write_file("TEMPERAT.PAL", b"pal");

    let mut state = ContentLabState::new(vec![ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Source",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ))]);

    assert_eq!(
        state
            .selected_entry()
            .expect("the first gallery entry should be selected")
            .relative_path,
        "1TNK.SHP"
    );

    state.move_selection(1);

    assert_eq!(
        state
            .selected_entry()
            .expect("the next gallery entry should be selected")
            .relative_path,
        "TEMPERAT.PAL"
    );
}

/// Proves that the gallery window keeps the selected row visible once the
/// selection moves beyond the first screenful of thumbnails.
#[test]
fn gallery_window_scrolls_with_the_selected_row() {
    let fixture = TestDir::new("gallery_window_scroll");
    for entry_index in 0..8 {
        fixture.write_file(&format!("ENTRY{entry_index:02}.SHP"), b"shp");
    }

    let mut state = ContentLabState::new(vec![ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Source",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ))]);

    state.move_selection(6);

    let gallery_window = state
        .gallery_window()
        .expect("the gallery window should exist for visual entries");

    assert_eq!(gallery_window.entry_indices.len(), GALLERY_VISIBLE_SLOTS);
    assert_eq!(gallery_window.entry_indices[0], 2);
    assert_eq!(
        gallery_window.entry_indices[gallery_window.selected_window_index],
        6
    );
}

/// Proves that wide resources are letterboxed into a taller inspector box
/// instead of being stretched to fill both axes.
#[test]
fn contained_image_size_preserves_aspect_ratio_for_wide_assets() {
    let contained = ContainedImageSize::for_source(320, 100, 200.0, 120.0);

    assert_eq!(contained.width, 200.0);
    assert_eq!(contained.height, 62.5);
}

/// Proves that tall resources are pillarboxed into a wider box instead of
/// being distorted to the box aspect ratio.
#[test]
fn contained_image_size_preserves_aspect_ratio_for_tall_assets() {
    let contained = ContainedImageSize::for_source(80, 240, 180.0, 150.0);

    assert_eq!(contained.width, 50.0);
    assert_eq!(contained.height, 150.0);
}

/// Proves that classic SHP previews prefer `TEMPERAT.PAL` over other available
/// palettes when no more specific theater hint is present.
#[test]
fn sprite_preview_prefers_temperat_palette_when_external_palette_is_needed() {
    let fixture = TestDir::new("content_window_palette_choice");
    fixture.write_file("CONQUER_OUTPUT/1TNK.SHP", &test_shp_bytes());
    fixture.write_file(
        "LOCAL_OUTPUT/SNOW.PAL",
        &test_palette_bytes([10, 12, 14], [20, 22, 24]),
    );
    fixture.write_file(
        "LOCAL_OUTPUT/TEMPERAT.PAL",
        &test_palette_bytes([40, 42, 44], [50, 52, 54]),
    );

    let sprite_catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Sprites",
        fixture.path().join("CONQUER_OUTPUT"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let palette_catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Palettes",
        fixture.path().join("LOCAL_OUTPUT"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let sprite_entry = sprite_catalog
        .entries
        .first()
        .expect("sprite fixture should contain one SHP");
    let catalogs = [sprite_catalog.clone(), palette_catalog];

    let palette_entry = super::preview::resolve_palette_entry_for_sprite(sprite_entry, &catalogs)
        .expect("a palette should be resolved");

    assert_eq!(palette_entry.relative_path, "TEMPERAT.PAL");
}

/// Proves that the content-lab can decode a selected SHP plus an external
/// palette into an actual RGBA preview frame without touching Bevy runtime
/// state.
#[test]
fn sprite_preview_loads_real_shp_and_palette_bytes() {
    let fixture = TestDir::new("content_window_sprite_preview");
    let shp_path = fixture.write_file("CONQUER_OUTPUT/1TNK.SHP", &test_shp_bytes());
    fixture.write_file(
        "LOCAL_OUTPUT/TEMPERAT.PAL",
        &test_palette_bytes([32, 16, 16], [16, 32, 16]),
    );

    let sprite_catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Sprites",
        fixture.path().join("CONQUER_OUTPUT"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let palette_catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Palettes",
        fixture.path().join("LOCAL_OUTPUT"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let entry = ContentCatalogEntry {
        relative_path: "CONQUER_OUTPUT/1TNK.SHP".into(),
        location: ContentEntryLocation::filesystem(shp_path),
        size_bytes: 0,
        family: ContentFamily::SpriteSheet,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview =
        super::preview::load_preview_for_entry(&entry, &[sprite_catalog, palette_catalog])
            .expect("preview loading should succeed")
            .expect("sprite sheets are previewable");

    assert_eq!(preview.frame().width(), 2);
    assert_eq!(preview.frame().height(), 2);
    assert!(preview.summary_text().contains("Actual SHP preview"));
    assert!(preview.summary_text().contains("TEMPERAT.PAL"));
    assert!(
        preview
            .frame()
            .rgba8_pixels()
            .chunks_exact(4)
            .any(|pixel| pixel[3] == 255),
        "the loaded SHP preview should contain visible opaque pixels",
    );
}

/// Proves that mounted `.mix` members feed the same preview loader as loose
/// files, including palette resolution across sibling mounted members.
#[test]
fn sprite_preview_loads_real_shp_and_palette_bytes_from_mix_members() {
    let fixture = TestDir::new("content_window_mix_preview");
    fixture.write_file(
        "MAIN.MIX",
        &build_named_mix(&[
            ("1TNK.SHP", &test_shp_bytes()),
            (
                "TEMPERAT.PAL",
                &test_palette_bytes([32, 16, 16], [16, 32, 16]),
            ),
        ]),
    );

    let catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture MIX Root",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.relative_path == "MAIN.MIX::1TNK.SHP")
        .cloned()
        .expect("the mounted MIX sprite member should exist");

    let preview = super::preview::load_preview_for_entry(&entry, &[catalog])
        .expect("archive-member preview loading should succeed")
        .expect("sprite sheets are previewable");

    assert_eq!(preview.frame().width(), 2);
    assert_eq!(preview.frame().height(), 2);
    assert!(preview.summary_text().contains("Actual SHP preview"));
    assert!(preview.summary_text().contains("TEMPERAT.PAL"));
    assert!(
        preview
            .frame()
            .rgba8_pixels()
            .chunks_exact(4)
            .any(|pixel| pixel[3] == 255),
        "the mounted SHP preview should contain visible opaque pixels",
    );
}

/// Proves that selecting a `.pal` entry builds a visible 16x16 swatch grid so
/// the content lab can show actual palette resources, not only sprite sheets.
#[test]
fn palette_preview_builds_a_visible_swatch_grid() {
    let fixture = TestDir::new("content_window_palette_preview");
    let palette_path = fixture.write_file(
        "LOCAL_OUTPUT/TEMPERAT.PAL",
        &test_palette_bytes([12, 24, 36], [40, 20, 10]),
    );
    let entry = ContentCatalogEntry {
        relative_path: "LOCAL_OUTPUT/TEMPERAT.PAL".into(),
        location: ContentEntryLocation::filesystem(palette_path),
        size_bytes: 0,
        family: ContentFamily::Palette,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview = super::preview::load_preview_for_entry(&entry, &[])
        .expect("palette preview loading should succeed")
        .expect("palettes are previewable");

    assert_eq!(preview.frame().width(), 16);
    assert_eq!(preview.frame().height(), 16);
    assert!(preview.summary_text().contains("Actual PAL preview"));
    assert!(preview.summary_text().contains("16x16 palette swatch grid"));
}

/// Proves that `.aud` resources are turned into a waveform preview plus
/// decoded PCM samples, so the runtime can play them without first wrapping
/// them in a synthetic WAV container.
#[test]
fn aud_preview_builds_waveform_and_direct_pcm_payload() {
    let fixture = TestDir::new("content_window_aud_preview");
    let aud_path = fixture.write_file(
        "SOUNDS/SPEECH.AUD",
        &ic_cnc_content::cnc_formats::aud::build_aud(
            &[0, 1200, -1200, 2400, -2400, 800, -800, 0],
            22050,
            false,
        ),
    );
    let entry = ContentCatalogEntry {
        relative_path: "SOUNDS/SPEECH.AUD".into(),
        location: ContentEntryLocation::filesystem(aud_path),
        size_bytes: 0,
        family: ContentFamily::Audio,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview = super::preview::load_preview_for_entry(&entry, &[])
        .expect("AUD preview loading should succeed")
        .expect("AUD resources are previewable");

    assert_eq!(preview.frame().width(), 512);
    assert_eq!(preview.frame().height(), 160);
    assert!(preview.summary_text().contains("Actual AUD preview"));
    assert!(preview.summary_text().contains("waveform preview"));
    assert!(preview.summary_text().contains("direct PCM playback"));
    let samples = preview
        .audio_pcm_samples()
        .expect("AUD preview should expose decoded PCM samples");
    let audio = preview
        .audio()
        .expect("AUD preview should expose runtime audio metadata");
    assert!(
        !samples.is_empty(),
        "the decoded payload should contain playable PCM samples",
    );
    assert_eq!(audio.sample_rate(), 22_050);
    assert_eq!(audio.channels(), 1);
    assert!(audio.duration_seconds() > 0.0);
}

/// Proves that `.wsa` animations decode into multiple preview frames and use
/// the same external-palette resolution path as other indexed visuals.
#[test]
fn wsa_preview_decodes_multiple_frames_with_external_palette() {
    let fixture = TestDir::new("content_window_wsa_preview");
    let frame_a = [0u8, 1, 2, 0];
    let frame_b = [0u8, 2, 1, 0];
    let wsa_path = fixture.write_file(
        "MOVIES/ANTS.WSA",
        &ic_cnc_content::cnc_formats::wsa::encode_frames(&[&frame_a, &frame_b], 2, 2)
            .expect("synthetic WSA fixture should encode"),
    );
    fixture.write_file(
        "PALETTES/TEMPERAT.PAL",
        &test_palette_bytes([20, 30, 40], [45, 15, 10]),
    );

    let visual_catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Videos",
        fixture.path().join("MOVIES"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let palette_catalog = ContentCatalog::scan(ContentSourceRoot::directory(
        "Fixture Palettes",
        fixture.path().join("PALETTES"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let entry = ContentCatalogEntry {
        relative_path: "MOVIES/ANTS.WSA".into(),
        location: ContentEntryLocation::filesystem(wsa_path),
        size_bytes: 0,
        family: ContentFamily::Video,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview =
        super::preview::load_preview_for_entry(&entry, &[visual_catalog, palette_catalog])
            .expect("WSA preview loading should succeed")
            .expect("WSA resources are previewable");

    assert_eq!(preview.frame().width(), 2);
    assert_eq!(preview.frame().height(), 2);
    assert_eq!(preview.frame_count(), Some(2));
    assert!(preview.summary_text().contains("Actual WSA preview"));
    assert!(preview.summary_text().contains("TEMPERAT.PAL"));
}

/// Proves that text/config assets surface an excerpt in the diagnostics panel
/// instead of pretending every resource must decode to pixels.
#[test]
fn text_preview_surfaces_config_excerpt() {
    let fixture = TestDir::new("content_window_text_preview");
    let ini_path = fixture.write_file(
        "RULES/SCG01EA.INI",
        b"[Basic]\nName=Test Mission\nPlayer=England\n",
    );
    let entry = ContentCatalogEntry {
        relative_path: "RULES/SCG01EA.INI".into(),
        location: ContentEntryLocation::filesystem(ini_path),
        size_bytes: 0,
        family: ContentFamily::Config,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview = super::preview::load_preview_for_entry(&entry, &[])
        .expect("text preview loading should succeed")
        .expect("config files are previewable");

    assert!(preview.visual().is_none());
    assert!(preview.audio().is_none());
    assert!(preview
        .text_body()
        .expect("text preview should expose an excerpt")
        .contains("Name=Test Mission"));
    assert!(preview.summary_text().contains("Text preview"));
}

/// Proves that VQA previews surface both animation frames and decoded audio so
/// the content lab can validate classic cutscene-style resources in one panel.
#[test]
fn vqa_preview_surfaces_video_frames_and_audio() {
    let fixture = TestDir::new("content_window_vqa_preview");
    let vqa_path = fixture.write_file("MOVIES/INTRO.VQA", &test_vqa_bytes());
    let entry = ContentCatalogEntry {
        relative_path: "MOVIES/INTRO.VQA".into(),
        location: ContentEntryLocation::filesystem(vqa_path),
        size_bytes: 0,
        family: ContentFamily::Video,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview = super::preview::load_preview_for_entry(&entry, &[])
        .expect("VQA preview loading should succeed")
        .expect("VQA resources are previewable");

    assert_eq!(preview.frame().width(), 4);
    assert_eq!(preview.frame().height(), 2);
    assert_eq!(preview.frame_count(), Some(2));
    assert!(preview.audio_pcm_samples().is_some());
    assert!(preview.summary_text().contains("Actual VQA preview"));
    assert!(preview.summary_text().contains("audio: present"));
}

/// Proves that the static capability map used for startup selection and entry
/// badges treats audio, animation, and text formats as previewable.
#[test]
fn preview_capability_badges_cover_audio_video_and_text_assets() {
    let aud_entry = ContentCatalogEntry {
        relative_path: "SOUNDS/VOICE.AUD".into(),
        location: ContentEntryLocation::filesystem(PathBuf::from("/tmp/VOICE.AUD")),
        size_bytes: 0,
        family: ContentFamily::Audio,
        support: ContentSupportLevel::SupportedNow,
    };
    let vqa_entry = ContentCatalogEntry {
        relative_path: "MOVIES/INTRO.VQA".into(),
        location: ContentEntryLocation::filesystem(PathBuf::from("/tmp/INTRO.VQA")),
        size_bytes: 0,
        family: ContentFamily::Video,
        support: ContentSupportLevel::SupportedNow,
    };
    let ini_entry = ContentCatalogEntry {
        relative_path: "RULES/SCG01EA.INI".into(),
        location: ContentEntryLocation::filesystem(PathBuf::from("/tmp/SCG01EA.INI")),
        size_bytes: 0,
        family: ContentFamily::Config,
        support: ContentSupportLevel::SupportedNow,
    };

    assert_eq!(
        super::preview::preview_capabilities_for_entry(&aud_entry).badge_string(),
        "VA-"
    );
    assert_eq!(
        super::preview::preview_capabilities_for_entry(&vqa_entry).badge_string(),
        "VA-"
    );
    assert_eq!(
        super::preview::preview_capabilities_for_entry(&ini_entry).badge_string(),
        "--T"
    );
}

fn test_shp_bytes() -> Vec<u8> {
    ic_cnc_content::cnc_formats::shp::encode_frames(&[&[0u8, 1, 2, 0]], 2, 2)
        .expect("synthetic SHP fixture should encode")
}

fn test_palette_bytes(primary: [u8; 3], secondary: [u8; 3]) -> Vec<u8> {
    let mut bytes = vec![0u8; ic_cnc_content::cnc_formats::pal::PALETTE_BYTES];
    set_palette_entry(&mut bytes, 1, primary);
    set_palette_entry(&mut bytes, 2, secondary);
    bytes
}

fn test_palette_rgb8() -> [u8; 768] {
    let mut bytes = [0u8; 768];
    set_palette_entry(&mut bytes, 1, [80, 16, 16]);
    set_palette_entry(&mut bytes, 2, [16, 80, 16]);
    bytes
}

fn test_vqa_bytes() -> Vec<u8> {
    let frames = vec![
        vec![0u8, 1, 2, 0, 0, 1, 2, 0],
        vec![0u8, 2, 1, 0, 0, 2, 1, 0],
    ];
    let audio_samples = [0i16, 1500, -1500, 3000, -3000, 750, -750, 0];
    let audio = ic_cnc_content::cnc_formats::vqa::VqaAudioInput {
        samples: &audio_samples,
        sample_rate: 22050,
        channels: 1,
    };

    ic_cnc_content::cnc_formats::vqa::encode_vqa(
        &frames,
        &test_palette_rgb8(),
        4,
        2,
        Some(&audio),
        &ic_cnc_content::cnc_formats::vqa::VqaEncodeParams::default(),
    )
    .expect("synthetic VQA fixture should encode")
}

fn build_named_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let payload_size = files
        .iter()
        .map(|(_, bytes)| bytes.len() as u32)
        .sum::<u32>();

    bytes.extend_from_slice(&(files.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&payload_size.to_le_bytes());

    let mut current_offset = 0u32;
    for (name, payload) in files {
        bytes.extend_from_slice(
            &ic_cnc_content::cnc_formats::mix::crc(name)
                .to_raw()
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&current_offset.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        current_offset = current_offset.saturating_add(payload.len() as u32);
    }

    for (_, payload) in files {
        bytes.extend_from_slice(payload);
    }

    bytes
}

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
    let mut current_offset = data_start as u32;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, payload) in files {
        offsets.push(current_offset);
        current_offset = current_offset.saturating_add(payload.len() as u32);
    }

    for (index, (_, payload)) in files.iter().enumerate() {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(index as u32).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&offsets[index].to_le_bytes());
        bytes.extend_from_slice(&(index as u32).to_le_bytes());
    }

    for (_, payload) in files {
        bytes.extend_from_slice(payload);
    }

    bytes
}

fn set_palette_entry(bytes: &mut [u8], index: usize, rgb: [u8; 3]) {
    let base = index * 3;
    bytes[base..base + 3].copy_from_slice(&rgb);
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ic_game_{label}_{unique}"));
        fs::create_dir_all(&path).expect("temporary fixture directory should be created");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }

    fn write_file(&self, relative_path: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent directory should be created");
        }
        fs::write(&path, bytes).expect("fixture file should be written");
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
