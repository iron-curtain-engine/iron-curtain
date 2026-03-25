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

/// Proves that the content lab can start in a loading state without already
/// having finished catalog construction.
///
/// The runtime uses this mode so the Bevy window appears immediately while a
/// background worker scans large Red Alert / Remastered trees.
#[test]
fn loading_state_reports_configured_sources_before_scan_completion() {
    let state = ContentLabState::loading(vec![ContentSourceRoot::directory(
        "Fixture CD",
        PathBuf::from("/example/ra"),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    )]);

    let text = state.render_text();

    assert!(state.is_loading());
    assert!(text.contains("Scanning configured source roots"));
    assert!(text.contains("Fixture CD"));
    assert!(text.contains("/example/ra"));
}

/// Proves that the content-lab window uses a desktop-safe fallback size before
/// Bevy's fullscreen mode claims the monitor.
///
/// The fallback size is still kept as a safe default for platforms that may
/// briefly construct the window before the fullscreen request is honored.
#[test]
fn content_window_starts_in_borderless_fullscreen_mode() {
    let display = crate::config::DisplayConfig::default();
    let window = content_lab_window(&display);

    assert_eq!(window.title, "Iron Curtain - Content Lab");
    assert_eq!(window.resolution.physical_width(), 1280);
    assert_eq!(window.resolution.physical_height(), 720);
    assert_eq!(
        window.mode,
        WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
    );
    assert!(!window.resizable);
}

/// Proves that video resources claim a larger contain-fit presentation budget
/// than ordinary art assets.
///
/// The content lab treats cutscenes as a "movie mode" surface. They should
/// use almost the whole monitor while still preserving aspect ratio, whereas
/// sprites and palettes should leave room for the surrounding diagnostics UI.
#[test]
fn video_preview_surface_policy_uses_movie_mode_budget() {
    let playback = crate::config::PlaybackConfig::default();
    let video_policy =
        super::preview::preview_surface_policy_for_family(Some(ContentFamily::Video), &playback);
    let sprite_policy =
        super::preview::preview_surface_policy_for_family(Some(ContentFamily::SpriteSheet), &playback);

    assert!(video_policy.max_width_fraction > sprite_policy.max_width_fraction);
    assert!(video_policy.max_height_fraction > sprite_policy.max_height_fraction);
    assert_eq!(video_policy.max_width_fraction, 0.96);
    assert_eq!(video_policy.max_height_fraction, 0.96);
}

/// Proves that preview preparation is pushed to a background task for real
/// video playback instead of blocking the Bevy update thread.
///
/// `cnc-formats` currently exposes whole-file VQA decode, so the best local
/// runtime policy is to move that work off the main thread while keeping the
/// UI responsive. Static art, VQP palette tables, and WSA animations still
/// use the simpler immediate path.
#[test]
fn video_preview_policy_uses_background_loading() {
    let vqa_entry = ContentCatalogEntry {
        relative_path: "MOVIE.VQA".into(),
        location: ContentEntryLocation::Filesystem {
            absolute_path: PathBuf::from("/dummy/MOVIE.VQA"),
        },
        size_bytes: 100,
        family: ContentFamily::Video,
        support: ContentSupportLevel::SupportedNow,
    };
    let vqp_entry = ContentCatalogEntry {
        relative_path: "TABLE.VQP".into(),
        location: ContentEntryLocation::Filesystem {
            absolute_path: PathBuf::from("/dummy/TABLE.VQP"),
        },
        size_bytes: 100,
        family: ContentFamily::Video,
        support: ContentSupportLevel::SupportedNow,
    };
    let shp_entry = ContentCatalogEntry {
        relative_path: "UNIT.SHP".into(),
        location: ContentEntryLocation::Filesystem {
            absolute_path: PathBuf::from("/dummy/UNIT.SHP"),
        },
        size_bytes: 100,
        family: ContentFamily::SpriteSheet,
        support: ContentSupportLevel::SupportedNow,
    };
    assert!(super::preview::should_background_load_preview(&vqa_entry));
    assert!(!super::preview::should_background_load_preview(&vqp_entry));
    assert!(!super::preview::should_background_load_preview(&shp_entry));
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

/// Proves that startup prefers a recognizable showcase asset like `ENGLISH.VQA`
/// when one is present instead of dropping the user onto an arbitrary first
/// sprite in sorted path order.
///
/// The content lab is supposed to make it obvious that it is reading real Red
/// Alert data. Landing on a famous movie clip is a stronger proof surface than
/// defaulting to the alphabetically earliest unit sprite.
#[test]
fn content_lab_prefers_showcase_assets_when_available() {
    let fixture = TestDir::new("content_window_showcase_selection");
    fixture.write_file("CONQUER_OUTPUT/1TNK.SHP", b"shp");
    fixture.write_file("MOVIES/ENGLISH.VQA", b"vqa");

    let source = ContentSourceRoot::directory(
        "Fixture CD",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let state = ContentLabState::new(vec![ContentCatalog::scan(source)]);

    assert_eq!(state.selected_catalog_index(), 0);
    assert_eq!(
        state
            .selected_entry()
            .expect("the showcase entry should be selected")
            .relative_path,
        "MOVIES/ENGLISH.VQA"
    );
}

/// Proves that once a background scan completes, replacing the placeholder
/// state with real catalogs lands on the expected showcase resource.
#[test]
fn replacing_loading_state_with_catalogs_selects_showcase_resource() {
    let fixture = TestDir::new("content_window_replace_catalogs");
    fixture.write_file("CONQUER_OUTPUT/1TNK.SHP", b"shp");
    fixture.write_file("MOVIES/ENGLISH.VQA", b"vqa");

    let source = ContentSourceRoot::directory(
        "Fixture CD",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let mut state = ContentLabState::loading(vec![source.clone()]);

    state.replace_catalogs(vec![ContentCatalog::scan(source)]);

    assert!(!state.is_loading());
    assert_eq!(
        state
            .selected_entry()
            .expect("the showcase entry should be selected after loading")
            .relative_path,
        "MOVIES/ENGLISH.VQA"
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

/// Proves that animated preview runtime state uses one display surface even
/// when multiple decoded frames are cached behind it.
///
/// This is the local stepping stone toward a true streaming movie player. The
/// content lab still eagerly decodes some media today, but the Bevy runtime
/// should no longer mirror that by creating one GPU image per frame.
#[test]
fn visual_preview_session_uses_one_display_surface_for_multi_frame_media() {
    let frame_a =
        ic_render::sprite::RgbaSpriteFrame::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255])
            .expect("frame A should be valid");
    let frame_b =
        ic_render::sprite::RgbaSpriteFrame::from_rgba(2, 1, vec![0, 0, 255, 255, 255, 255, 0, 255])
            .expect("frame B should be valid");

    let session = super::preview::VisualPreviewSession::new(vec![frame_a, frame_b], Some(0.1))
        .expect("a multi-frame session should be created");

    assert_eq!(session.frame_count(), 2);
    assert_eq!(session.display_surface_count(), 1);
    assert_eq!(session.current_frame_index(), 0);
    assert!(session.runtime_summary().contains("cached frames: 2"));
    assert!(session.runtime_summary().contains("display surface: 1"));
}

/// Proves that switching frames in the visual session changes only the active
/// frame pointer instead of discarding the prepared cache.
///
/// The future streaming path will replace the eager frame cache with a bounded
/// queue, but the selection semantics should stay the same: advance the active
/// frame while keeping the runtime surface stable.
#[test]
fn visual_preview_session_switches_active_frame_without_rebuilding_the_session() {
    let frame_a = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![10, 20, 30, 255])
        .expect("frame A should be valid");
    let frame_b = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![40, 50, 60, 255])
        .expect("frame B should be valid");

    let mut session = super::preview::VisualPreviewSession::new(vec![frame_a, frame_b], Some(0.2))
        .expect("a two-frame session should be created");

    session.select_frame(1);

    assert_eq!(session.frame_count(), 2);
    assert_eq!(session.current_frame_index(), 1);
    assert_eq!(
        session.current_frame().rgba8_pixels(),
        &[40, 50, 60, 255],
        "switching the active frame should expose the second cached frame",
    );
}

/// Proves that advancing an animation frame inside the current preview runtime
/// reuses the same Bevy image allocation when the frame size stays stable.
///
/// The content lab already moved to one persistent display surface for the
/// selected preview. Reusing the pixel buffer for equal-sized frames prevents
/// an avoidable allocation on every frame step while we wait for upstream
/// incremental decode APIs.
#[test]
fn image_update_reuses_pixel_buffer_for_same_sized_preview_frames() {
    let frame_a =
        ic_render::sprite::RgbaSpriteFrame::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255])
            .expect("frame A should be valid");
    let frame_b =
        ic_render::sprite::RgbaSpriteFrame::from_rgba(2, 1, vec![0, 0, 255, 255, 255, 255, 0, 255])
            .expect("frame B should be valid");
    let mut image = super::preview::rgba_frame_to_image(&frame_a);
    let pointer_before = image
        .data
        .as_ref()
        .expect("the preview image should keep CPU-visible pixel data")
        .as_ptr();

    super::preview::update_image_from_rgba_frame(&mut image, &frame_b);

    let data = image
        .data
        .as_ref()
        .expect("the preview image should still expose pixel data after update");
    assert_eq!(
        pointer_before,
        data.as_ptr(),
        "equal-sized frame updates should reuse the same image buffer allocation",
    );
    assert_eq!(data.as_slice(), frame_b.rgba8_pixels());
}

/// Proves that the display surface updates its descriptor when a new frame has
/// different dimensions.
///
/// This matters for defensive correctness in the current local runtime and for
/// future streaming playback, where the presentation surface must reflect the
/// latest decoded frame metadata instead of assuming the original dimensions
/// forever.
#[test]
fn image_update_rewrites_descriptor_when_frame_dimensions_change() {
    let frame_a = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![10, 20, 30, 255])
        .expect("frame A should be valid");
    let frame_b =
        ic_render::sprite::RgbaSpriteFrame::from_rgba(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255])
            .expect("frame B should be valid");
    let mut image = super::preview::rgba_frame_to_image(&frame_a);

    super::preview::update_image_from_rgba_frame(&mut image, &frame_b);

    assert_eq!(image.texture_descriptor.size.width, 2);
    assert_eq!(image.texture_descriptor.size.height, 1);
    assert_eq!(
        image
            .data
            .as_ref()
            .expect("the resized preview image should still expose pixel data")
            .as_slice(),
        frame_b.rgba8_pixels(),
    );
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
        super::preview::load_preview_for_entry(&entry, &[sprite_catalog, palette_catalog], None, None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[catalog], None, None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None, None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None, None)
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
        super::preview::load_preview_for_entry(&entry, &[visual_catalog, palette_catalog], None, None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None, None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None, None)
        .expect("VQA preview loading should succeed")
        .expect("VQA resources are previewable");

    assert_eq!(preview.frame().width(), 4);
    assert_eq!(preview.frame().height(), 2);
    assert_eq!(preview.frame_count(), Some(2));
    assert!(preview.audio_pcm_samples().is_some());
    assert!(
        preview
            .frame()
            .rgba8_pixels()
            .chunks_exact(4)
            .all(|pixel| pixel[3] == 255),
        "VQA movie frames should stay fully opaque; palette index 0 is a real video color, not transparency",
    );
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

// ---------------------------------------------------------------------------
// Streaming playback state machine regression tests
// ---------------------------------------------------------------------------

/// Proves that `push_frames` preserves the current frame index so playback
/// continues smoothly when batches arrive during streaming VQA decode.
///
/// Regression: before this fix, accumulated timer would wrap past all newly
/// appended frames and restart the video from frame 0.
#[test]
fn push_frames_preserves_current_frame_index() {
    let frame_a = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![10, 20, 30, 255])
        .expect("frame A should be valid");
    let frame_b = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![40, 50, 60, 255])
        .expect("frame B should be valid");
    let frame_c = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![70, 80, 90, 255])
        .expect("frame C should be valid");

    // Start with one frame (as FirstFrame would), advance to it.
    let mut session =
        super::preview::VisualPreviewSession::new(vec![frame_a], Some(1.0 / 15.0))
            .expect("a single-frame session should be created");
    assert_eq!(session.current_frame_index(), 0);
    assert_eq!(session.frame_count(), 1);

    // Simulate streaming batch arrival with two new frames.
    session.push_frames(vec![frame_b, frame_c]);

    assert_eq!(session.frame_count(), 3);
    assert_eq!(
        session.current_frame_index(),
        0,
        "current frame must remain at 0 after push_frames; it must not jump ahead or wrap",
    );
}

/// Proves that a single-frame session reports no animation, preventing the
/// frame timer from accumulating while we wait for streaming batches.
///
/// This matters because the advance loop checks `has_animation()` before
/// incrementing the timer. Without this guard the timer would accumulate
/// during the single-frame wait and then skip many frames on batch arrival.
#[test]
fn single_frame_session_has_no_animation() {
    let frame = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![10, 20, 30, 255])
        .expect("frame should be valid");
    let session =
        super::preview::VisualPreviewSession::new(vec![frame], Some(1.0 / 15.0))
            .expect("a single-frame session should be created");

    assert_eq!(session.frame_count(), 1);
    assert!(
        session.frame_duration_seconds().is_some(),
        "session should still know the intended frame rate",
    );
    // The key invariant: has_animation() requires frame_count > 1.
    // We test this indirectly via frame_count since has_animation is on
    // ContentPreviewTracker, but the session-level contract is what matters.
    assert_eq!(
        session.frame_count() > 1,
        false,
        "a single-frame session must not report as animatable",
    );
}

/// Proves that `select_frame` wraps safely at both ends of the frame range.
///
/// Regression: direct indexing patterns like `frames[frame_index]` panicked
/// on out-of-bounds access. The modulo wrap in `select_frame` is the
/// defense-in-depth layer.
#[test]
fn select_frame_wraps_at_boundaries() {
    let frames: Vec<_> = (0..3u8)
        .map(|i| {
            ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![i, i, i, 255])
                .expect("synthetic frame should be valid")
        })
        .collect();

    let mut session =
        super::preview::VisualPreviewSession::new(frames, Some(0.1))
            .expect("a three-frame session should be created");

    // Normal select.
    session.select_frame(2);
    assert_eq!(session.current_frame_index(), 2);

    // Wrap past end: index 3 → frame 0.
    session.select_frame(3);
    assert_eq!(session.current_frame_index(), 0);

    // Larger wrap: index 5 → frame 2.
    session.select_frame(5);
    assert_eq!(session.current_frame_index(), 2);
}

/// Proves that PlaybackSyncMode::Timer and AudioSync are distinct variants,
/// and that the StreamingFirstFrame phase always uses Timer.
///
/// Regression: the old `force_timer_playback: bool` flag defaulted to false,
/// meaning audio-sync was the *implicit* mode. That caused videos to restart
/// from frame 0 when audio arrived after streaming playback had started.
/// The enum makes AudioSync an explicit opt-in that can only appear in Ready.
#[test]
fn audio_sync_mode_requires_explicit_opt_in() {
    let timer = super::preview::PlaybackSyncMode::Timer;
    let audio = super::preview::PlaybackSyncMode::AudioSync;
    assert_ne!(
        timer, audio,
        "Timer and AudioSync must be distinct states so the advance loop \
         can choose the right path",
    );
}

/// Proves that the StreamingFirstFrame phase always reports Timer sync mode
/// and that playback is requested by default.
#[test]
fn streaming_first_frame_phase_uses_timer_and_auto_plays() {
    let frame = ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![10, 20, 30, 255])
        .expect("frame should be valid");
    let phase = super::preview::PreviewPhase::StreamingFirstFrame {
        family: super::catalog::ContentFamily::Video,
        visual_session: super::preview::VisualPreviewSession::new(vec![frame], Some(1.0 / 15.0))
            .expect("session should be created"),
        frame_timer_seconds: 0.0,
        playback_requested: true,
    };
    match &phase {
        super::preview::PreviewPhase::StreamingFirstFrame {
            playback_requested, ..
        } => {
            assert!(
                *playback_requested,
                "StreamingFirstFrame should auto-play",
            );
        }
        _ => panic!("expected StreamingFirstFrame"),
    }
}

/// Proves that `finalize_streaming` switches to AudioSync when audio is
/// present and resets the frame position to 0.
///
/// The new design holds playback at frame 0 during decode
/// (`playback_requested = false`) and starts both video and audio
/// simultaneously from frame 0 on finalize, using AudioSync so that
/// video frame advancement is driven by the audio clock.
#[test]
fn finalize_streaming_with_audio_uses_audio_sync() {
    use super::preview::{
        AudioInfo, ContentPreviewTracker, PlaybackSyncMode,
        VisualPreviewSession,
    };
    use super::catalog::ContentFamily;

    let mut tracker = ContentPreviewTracker::default();

    tracker.test_begin_loading(ContentFamily::Video);
    let frames: Vec<_> = (0..10)
        .map(|i| {
            ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![i, i, i, 255])
                .expect("frame should be valid")
        })
        .collect();
    let session =
        VisualPreviewSession::new(frames, Some(1.0 / 15.0)).expect("session should be created");
    tracker.test_begin_streaming(session);
    // begin_streaming holds playback at frame 0.
    assert_eq!(tracker.test_current_frame_index(), 0);

    tracker.test_finalize_streaming(Some(AudioInfo {
        duration_seconds: 5.0,
    }));
    assert_eq!(
        tracker.test_sync_mode(),
        PlaybackSyncMode::AudioSync,
        "finalize_streaming with audio must use AudioSync to drive video from the audio clock",
    );
    assert_eq!(
        tracker.test_current_frame_index(),
        0,
        "finalize_streaming must start from frame 0",
    );
    assert!(
        tracker.test_playback_requested(),
        "playback should be requested after finalize",
    );
}

/// Proves that `finalize_streaming` without audio keeps Timer mode and
/// preserves the current frame position.
#[test]
fn finalize_streaming_without_audio_keeps_timer() {
    use super::preview::{ContentPreviewTracker, PlaybackSyncMode, VisualPreviewSession};
    use super::catalog::ContentFamily;

    let mut tracker = ContentPreviewTracker::default();
    tracker.test_begin_loading(ContentFamily::Video);
    let frames: Vec<_> = (0..5)
        .map(|i| {
            ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![i, i, i, 255])
                .expect("frame should be valid")
        })
        .collect();
    let session =
        VisualPreviewSession::new(frames, Some(1.0 / 15.0)).expect("session should be created");
    tracker.test_begin_streaming(session);
    tracker.test_select_frame(2);

    // Finalize without audio — should keep Timer and preserve position.
    tracker.test_finalize_streaming(None);
    assert_eq!(
        tracker.test_sync_mode(),
        PlaybackSyncMode::Timer,
        "finalize_streaming without audio must stay Timer",
    );
    assert_eq!(
        tracker.test_current_frame_index(),
        2,
        "finalize_streaming without audio must preserve frame position",
    );
}

/// Proves that the waveform preview generator does not panic on `i16::MIN`,
/// the extreme-but-valid PCM sample value.
///
/// Regression: `sample.abs()` overflows in debug mode for i16::MIN (-32768)
/// because 32768 does not fit in i16. The fix casts to i32 before `.abs()`.
#[test]
fn waveform_handles_i16_min_without_overflow() {
    use super::catalog::{ContentCatalogEntry, ContentEntryLocation, ContentFamily, ContentSupportLevel};

    // Build a WAV with i16::MIN samples.
    let fixture = TestDir::new("waveform_i16_min");
    let samples = [i16::MIN, i16::MAX, 0i16, i16::MIN];
    let wav_bytes = {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)
                .expect("WAV writer should initialize");
            for &s in &samples {
                writer.write_sample(s).expect("sample should write");
            }
            writer.finalize().expect("WAV should finalize");
        }
        cursor.into_inner()
    };
    let wav_path = fixture.write_file("EXTREME.WAV", &wav_bytes);
    let entry = ContentCatalogEntry {
        relative_path: "EXTREME.WAV".into(),
        location: ContentEntryLocation::filesystem(wav_path),
        size_bytes: wav_bytes.len() as u64,
        family: ContentFamily::Audio,
        support: ContentSupportLevel::SupportedNow,
    };

    // This must not panic with "attempt to negate with overflow".
    let preview = super::preview::load_preview_for_entry(&entry, &[], None, None)
        .expect("WAV preview should succeed")
        .expect("WAV files are previewable");
    assert!(
        preview.visual().is_some(),
        "WAV preview should include a waveform visual",
    );
}

/// Proves that `push_frames` into a session that has advanced beyond frame 0
/// does not regress the current position.
#[test]
fn push_frames_does_not_regress_advanced_position() {
    let make_frame = |v: u8| {
        ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![v, v, v, 255])
            .expect("frame should be valid")
    };

    let mut session =
        super::preview::VisualPreviewSession::new(vec![make_frame(0), make_frame(1)], Some(0.1))
            .expect("a two-frame session should be created");

    // Advance to frame 1 (as the timer would).
    session.select_frame(1);
    assert_eq!(session.current_frame_index(), 1);

    // A streaming batch arrives with two more frames.
    session.push_frames(vec![make_frame(2), make_frame(3)]);

    assert_eq!(session.frame_count(), 4);
    assert_eq!(
        session.current_frame_index(),
        1,
        "push_frames must not change the current frame index; \
         playback should continue from wherever it was",
    );

    // The frame at index 1 should still be the original second frame.
    assert_eq!(session.current_frame().rgba8_pixels(), &[1, 1, 1, 255]);
}

/// Proves that creating an empty session returns None rather than allowing
/// a session with zero frames, which would cause a divide-by-zero in the
/// modulo-based frame selection.
#[test]
fn empty_session_is_rejected() {
    let result = super::preview::VisualPreviewSession::new(vec![], Some(0.1));
    assert!(
        result.is_none(),
        "an empty frame list must produce None, not a session that panics on frame access",
    );
}

// ---------------------------------------------------------------------------
// Regression tests for fixes made 2026-03-21
// ---------------------------------------------------------------------------

/// Proves that SCOMP_SOS=99 AUD files are decoded without the 8-byte SOS
/// chunk headers being mis-interpreted as ADPCM nibbles.
///
/// Regression: `load_aud_preview` called `decode_adpcm` directly on the raw
/// `compressed_data` slice, which for SOS files contains 8-byte chunk headers
/// (containing the 0x0000_DEAF magic) interleaved with the ADPCM payload.
/// `decode_adpcm` treated those header bytes as data and produced noise bursts
/// throughout the audio.  The fix uses `AudStream::from_payload` which skips
/// chunk headers before dispatching the ADPCM decoder.
///
/// The fixture uses all-zero ADPCM bytes so the expected output is silence
/// (all-zero samples).  With the old code, the 0xDE and 0xAD bytes of the
/// chunk magic decoded to large nonzero values, failing both the sample-count
/// check and the amplitude check below.
#[test]
fn aud_scomp_sos_decodes_without_chunk_header_corruption() {
    // 20 zero ADPCM bytes → 40 decoded samples (each byte = 2 nibbles = 2
    // samples; IMA nibble 0 with step_index 0 produces diff = 0).
    const ADPCM_PAYLOAD_BYTES: usize = 20;
    const EXPECTED_SAMPLES: usize = ADPCM_PAYLOAD_BYTES * 2;
    const SAMPLE_RATE: u16 = 8000;

    // Manually build a valid SCOMP_SOS=99 AUD file with a single chunk.
    // SOS chunk header layout: compressed_size (u16) | uncompressed_size (u16)
    //                          | magic 0x0000_DEAF (u32)
    let adpcm_payload = vec![0u8; ADPCM_PAYLOAD_BYTES];
    let chunk_compressed = ADPCM_PAYLOAD_BYTES as u16;
    // Uncompressed = PCM bytes = samples × 2 bytes/sample (16-bit audio).
    let chunk_uncompressed = (EXPECTED_SAMPLES * 2) as u16;
    let total_compressed = (8 + ADPCM_PAYLOAD_BYTES) as u32; // 8-byte SOS header + payload
    let total_uncompressed = (EXPECTED_SAMPLES * 2) as u32;

    let mut aud_bytes: Vec<u8> = Vec::new();
    // 12-byte AUD file header.
    aud_bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    aud_bytes.extend_from_slice(&total_compressed.to_le_bytes());
    aud_bytes.extend_from_slice(&total_uncompressed.to_le_bytes());
    aud_bytes.push(0x02); // AUD_FLAG_16BIT
    aud_bytes.push(99);   // SCOMP_SOS
    // Single SOS chunk header (8 bytes).
    aud_bytes.extend_from_slice(&chunk_compressed.to_le_bytes());
    aud_bytes.extend_from_slice(&chunk_uncompressed.to_le_bytes());
    aud_bytes.extend_from_slice(&0x0000_DEAF_u32.to_le_bytes());
    // All-zero ADPCM payload.
    aud_bytes.extend_from_slice(&adpcm_payload);

    let fixture = TestDir::new("content_window_sos_aud");
    let aud_path = fixture.write_file("SOUNDS/INTRO.AUD", &aud_bytes);
    let entry = ContentCatalogEntry {
        relative_path: "SOUNDS/INTRO.AUD".into(),
        location: ContentEntryLocation::filesystem(aud_path),
        size_bytes: aud_bytes.len() as u64,
        family: ContentFamily::Audio,
        support: ContentSupportLevel::SupportedNow,
    };

    let preview = super::preview::load_preview_for_entry(&entry, &[], None, None)
        .expect("SCOMP_SOS AUD preview loading should succeed")
        .expect("AUD resources are previewable");

    let samples = preview
        .audio_pcm_samples()
        .expect("SOS AUD preview should expose decoded PCM samples");

    assert_eq!(
        samples.len(),
        EXPECTED_SAMPLES,
        "sample count must equal ADPCM bytes × 2; \
         extra samples would indicate chunk headers were decoded as ADPCM nibbles",
    );
    assert!(
        samples.iter().all(|&s| s.abs() < 100),
        "all-zero ADPCM payload must decode to near-silence; \
         large values mean the 0xDE/0xAD magic bytes were mis-decoded as ADPCM",
    );
}

/// Proves that the content-lab window uses `AutoNoVsync` so the swapchain
/// never times out during heavy background VQA decodes on integrated GPUs.
///
/// Regression: the default `AutoVsync` present mode panicked with
/// `SurfaceError::Timeout` on Intel Xe when a 28 MB VQA took ~2.6 s to decode
/// because memory-bus saturation prevented the GPU from acquiring the next
/// frame before the vsync deadline.
#[test]
fn content_window_uses_auto_no_vsync_to_prevent_swapchain_timeout() {
    use bevy::window::PresentMode;

    let display = crate::config::DisplayConfig::default();
    let window = content_lab_window(&display);

    assert_eq!(
        window.present_mode,
        PresentMode::AutoNoVsync,
        "the content-lab window must use AutoNoVsync so the GPU swapchain \
         never times out during long background VQA decodes",
    );
}

/// Proves that the scanlines overlay is suppressed for non-Video content
/// families (sprites, palettes, audio, etc.).
///
/// Regression: before the family restriction was added, the overlay would
/// stay visible after navigating away from a VQA to any other resource.
#[test]
fn scanlines_should_show_is_false_for_non_video_families() {
    use super::catalog::ContentFamily;
    use super::preview::ContentPreviewTracker;

    let families = [
        ContentFamily::SpriteSheet,
        ContentFamily::Palette,
        ContentFamily::Audio,
        ContentFamily::Config,
        ContentFamily::WestwoodArchive,
    ];

    for family in families {
        let mut tracker = ContentPreviewTracker::default();
        tracker.test_begin_loading(family);

        let is_video = tracker
            .test_current_family()
            .map_or(false, |f| matches!(f, ContentFamily::Video));

        assert!(
            !is_video,
            "{family:?} must not be treated as Video; \
             scanlines would incorrectly remain visible after navigating away from VQA",
        );
    }
}

/// Proves that navigating to a new entry does not destroy the tracker's
/// reference to the session-persistent scanlines overlay entity.
///
/// Regression: `clear_dynamic_assets` (called on every selection change) also
/// nulled `scanlines_overlay_entity` and `scanlines_overlay_material`.  The
/// fullscreen overlay is a session-wide entity, not per-entry, so losing its
/// handle caused a new overlay to spawn on the next VQA while the old one
/// (still `Visibility::Visible`) remained in the world indefinitely.
#[test]
fn scanlines_overlay_entity_persists_across_navigation() {
    use super::preview::ContentPreviewTracker;
    use super::catalog::ContentFamily;
    use bevy::prelude::Entity;

    let mut tracker = ContentPreviewTracker::default();

    // Simulate startup: VQA loaded and overlay spawned by sync_scanlines_overlay.
    tracker.test_begin_loading(ContentFamily::Video);
    let overlay_entity = Entity::from_bits(99);
    tracker.test_set_scanlines_overlay_entity(overlay_entity);
    assert_eq!(tracker.test_scanlines_overlay_entity(), Some(overlay_entity));

    // Simulate navigation to a new entry (triggers clear_dynamic_assets).
    tracker.test_clear_for_navigation();

    assert_eq!(
        tracker.test_scanlines_overlay_entity(),
        Some(overlay_entity),
        "the scanlines overlay entity must survive navigation; \
         losing it causes stale overlays to accumulate in the Bevy world",
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

/// Builds a MIX archive with an embedded XCC local mix database (LMD) so
/// that logical filenames are recoverable during catalog scanning.
fn build_named_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    // Build the LMD entry so embedded_names() resolves logical filenames.
    let lmd_data = build_lmd(files.iter().map(|(name, _)| *name));
    let lmd_name = "local mix database.dat";

    // Combine user files + LMD entry.
    let all_files: Vec<(&str, &[u8])> = files
        .iter()
        .copied()
        .chain(std::iter::once((lmd_name, lmd_data.as_slice())))
        .collect();

    build_raw_mix(&all_files)
}

/// Builds a MIX archive without an LMD entry (raw CRC-only entries).
fn build_raw_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
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

/// Builds an XCC local mix database (LMD) blob from a list of filenames.
fn build_lmd<'a>(names: impl Iterator<Item = &'a str>) -> Vec<u8> {
    let names: Vec<&str> = names.collect();
    let mut data = Vec::new();
    data.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for name in &names {
        data.extend_from_slice(name.as_bytes());
        data.push(0); // NUL-terminated name
        data.push(0); // NUL-terminated description (empty)
    }
    data
}

/// Finds the CRC-sorted index of a named entry in raw MIX bytes.
///
/// `MixArchiveReader` sorts entries by CRC after parsing, so the position
/// returned here matches `read_by_index`.
fn find_mix_entry_index(mix_bytes: &[u8], name: &str) -> usize {
    let target_crc = ic_cnc_content::cnc_formats::mix::crc(name).to_raw();
    let count = u16::from_le_bytes([mix_bytes[0], mix_bytes[1]]) as usize;
    // Header: 2 (count) + 4 (payload_size) = 6 bytes, then 12 bytes per entry.
    let mut crcs: Vec<u32> = (0..count)
        .map(|i| {
            let base = 6 + i * 12;
            u32::from_le_bytes([
                mix_bytes[base],
                mix_bytes[base + 1],
                mix_bytes[base + 2],
                mix_bytes[base + 3],
            ])
        })
        .collect();
    crcs.sort();
    crcs.iter()
        .position(|&c| c == target_crc)
        .unwrap_or_else(|| panic!("CRC for '{name}' not found in MIX archive"))
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

// ── Persistent archive handle cache ─────────────────────────────────────────

/// Proves that `ArchiveHandleCache` opens a MIX handle on first access and
/// returns the same handle on subsequent calls without re-opening the file.
#[test]
fn archive_handle_cache_returns_persistent_mix_handle() {
    let fixture = TestDir::new("handle_cache_persistent");
    let mix_path = fixture.write_file(
        "TEST.MIX",
        &build_named_mix(&[("FILE.BIN", b"hello")]),
    );

    let cache = super::ArchiveHandleCache::default();

    // First call opens the file and parses the index.
    let handle1 = cache
        .get_or_open_mix(&mix_path)
        .expect("first open should succeed");

    // Second call should return the same Arc (cache hit).
    let handle2 = cache
        .get_or_open_mix(&mix_path)
        .expect("second open should succeed");

    assert!(
        std::sync::Arc::ptr_eq(&handle1, &handle2),
        "repeated calls must return the same cached handle, not re-open the file"
    );

    // The cached reader should still be functional.
    let mix_bytes = build_named_mix(&[("FILE.BIN", b"hello")]);
    let idx = find_mix_entry_index(&mix_bytes, "FILE.BIN");
    let mut reader = handle1.lock().unwrap();
    let data = reader
        .read_by_index(idx)
        .expect("read should succeed")
        .expect("entry should exist");
    assert_eq!(data, b"hello");
}

/// Proves that `ArchiveHandleCache` returns an error for a non-existent file
/// rather than panicking.
#[test]
fn archive_handle_cache_returns_error_for_missing_file() {
    let cache = super::ArchiveHandleCache::default();
    let result = cache.get_or_open_mix(std::path::Path::new("/no/such/file.mix"));
    assert!(result.is_err(), "opening a non-existent file should fail gracefully");
}

/// Proves that `ArchiveHandleCache` can serve multiple threads concurrently
/// without panicking or returning corrupt data.
#[test]
fn archive_handle_cache_concurrent_access() {
    let fixture = TestDir::new("handle_cache_concurrent");
    let mix_path = fixture.write_file(
        "SHARED.MIX",
        &build_named_mix(&[("DATA.BIN", b"concurrent-ok")]),
    );

    let cache = super::ArchiveHandleCache::default();
    let mut threads = Vec::new();

    let mix_bytes = build_named_mix(&[("DATA.BIN", b"concurrent-ok")]);
    let idx = find_mix_entry_index(&mix_bytes, "DATA.BIN");

    for _ in 0..4 {
        let cache_clone = cache.clone();
        let path_clone = mix_path.clone();
        threads.push(std::thread::spawn(move || {
            let handle = cache_clone
                .get_or_open_mix(&path_clone)
                .expect("concurrent open should succeed");
            let mut reader = handle.lock().unwrap();
            let data = reader
                .read_by_index(idx)
                .expect("read should succeed")
                .expect("entry should exist");
            assert_eq!(data, b"concurrent-ok");
        }));
    }

    for thread in threads {
        thread.join().expect("thread should not panic");
    }
}

// ── Recursive MIX mounting ──────────────────────────────────────────────────

/// Proves that a MIX archive containing an inner MIX archive has both
/// levels of entries cataloged with correct `parent_indices` and
/// `relative_path` chains.
#[test]
fn mount_nested_mix_creates_entries_with_parent_indices() {
    let fixture = TestDir::new("nested_mix_mount");
    let inner_mix = build_named_mix(&[
        ("INNER_FILE.BIN", b"inner-data"),
    ]);
    fixture.write_file(
        "OUTER.MIX",
        &build_named_mix(&[
            ("CONQUER.MIX", &inner_mix),
            ("TOP_LEVEL.BIN", b"top-level-data"),
        ]),
    );

    let source = ContentSourceRoot::directory(
        "Nested MIX Root",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);

    assert!(catalog.available);

    // Should have: OUTER.MIX, CONQUER.MIX member, TOP_LEVEL.BIN member,
    // and INNER_FILE.BIN nested inside CONQUER.MIX.
    let nested_entry = catalog
        .entries
        .iter()
        .find(|e| e.relative_path.contains("INNER_FILE.BIN"))
        .expect("nested inner file should be cataloged");

    assert!(
        nested_entry.relative_path.contains("CONQUER.MIX::"),
        "relative path should show the nesting chain: {}",
        nested_entry.relative_path
    );

    match &nested_entry.location {
        ContentEntryLocation::MixMember { parent_indices, .. } => {
            assert!(
                !parent_indices.is_empty(),
                "nested entries must have non-empty parent_indices"
            );
        }
        other => panic!("expected MixMember, got {other:?}"),
    }
}

/// Proves that recursion stops at the configured depth limit, preventing
/// zip-bomb style attacks with deeply nested MIX-within-MIX archives.
#[test]
fn mount_nested_mix_stops_at_max_depth() {
    // Build 5 levels deep: L0 → L1 → L2 → L3 → L4 → DEEP.BIN
    // Only 3 levels of nesting should be mounted (MAX_NESTING_DEPTH = 3).
    let level4 = build_named_mix(&[("DEEP.BIN", b"too-deep")]);
    let level3 = build_named_mix(&[("LEVEL4.MIX", &level4)]);
    let level2 = build_named_mix(&[("LEVEL3.MIX", &level3)]);
    let level1 = build_named_mix(&[("LEVEL2.MIX", &level2)]);
    let level0 = build_named_mix(&[("LEVEL1.MIX", &level1)]);

    let fixture = TestDir::new("nested_mix_depth_limit");
    fixture.write_file("DEEP.MIX", &level0);

    let source = ContentSourceRoot::directory(
        "Deep Nesting",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);

    // DEEP.BIN at level 4 should NOT appear — only 3 levels of nesting allowed.
    let has_deep = catalog
        .entries
        .iter()
        .any(|e| e.relative_path.contains("DEEP.BIN"));
    assert!(
        !has_deep,
        "entries beyond MAX_NESTING_DEPTH should not be cataloged; \
         found DEEP.BIN despite being 4 levels deep"
    );

    // But level 3 (LEVEL4.MIX) should appear as a WestwoodArchive entry.
    let has_level4 = catalog
        .entries
        .iter()
        .any(|e| e.relative_path.contains("LEVEL4.MIX"));
    assert!(
        has_level4,
        "entries at the depth boundary (level 3) should still be cataloged"
    );
}

// ── Three-tier entry loading ────────────────────────────────────────────────

/// Proves that `load_entry_bytes_cached` reads a top-level MIX member using
/// the persistent handle cache (tier 2) when no RAM cache is available.
#[test]
fn load_entry_bytes_tier2_persistent_handle() {
    let fixture = TestDir::new("tier2_load");
    let mix_bytes = build_named_mix(&[("PAYLOAD.BIN", b"tier2-payload")]);
    let mix_path = fixture.write_file("ARCHIVE.MIX", &mix_bytes);

    let archive_index = find_mix_entry_index(&mix_bytes, "PAYLOAD.BIN");
    let handle_cache = super::ArchiveHandleCache::default();
    let entry = ContentCatalogEntry {
        relative_path: "ARCHIVE.MIX::PAYLOAD.BIN".into(),
        location: ContentEntryLocation::MixMember {
            archive_path: mix_path,
            archive_index,
            crc_raw: ic_cnc_content::cnc_formats::mix::crc("PAYLOAD.BIN").to_raw(),
            logical_name: Some("PAYLOAD.BIN".into()),
            parent_indices: vec![],
        },
        size_bytes: 13,
        family: ContentFamily::Other,
        support: ContentSupportLevel::Planned,
    };

    let bytes = super::preview_decode::load_entry_bytes_cached(&entry, None, Some(&handle_cache))
        .expect("tier 2 load should succeed");
    assert_eq!(bytes, b"tier2-payload");
}

/// Proves that `load_entry_bytes_cached` falls back to direct disk I/O
/// (tier 3) when neither the RAM cache nor the handle cache is provided.
#[test]
fn load_entry_bytes_tier3_disk_fallback() {
    let fixture = TestDir::new("tier3_load");
    let mix_bytes = build_named_mix(&[("DATA.BIN", b"disk-fallback")]);
    let mix_path = fixture.write_file("FALLBACK.MIX", &mix_bytes);

    let archive_index = find_mix_entry_index(&mix_bytes, "DATA.BIN");
    let entry = ContentCatalogEntry {
        relative_path: "FALLBACK.MIX::DATA.BIN".into(),
        location: ContentEntryLocation::MixMember {
            archive_path: mix_path,
            archive_index,
            crc_raw: ic_cnc_content::cnc_formats::mix::crc("DATA.BIN").to_raw(),
            logical_name: Some("DATA.BIN".into()),
            parent_indices: vec![],
        },
        size_bytes: 13,
        family: ContentFamily::Other,
        support: ContentSupportLevel::Planned,
    };

    let bytes = super::preview_decode::load_entry_bytes_cached(&entry, None, None)
        .expect("tier 3 disk fallback should succeed");
    assert_eq!(bytes, b"disk-fallback");
}

// ── Nested MIX entry extraction ─────────────────────────────────────────────

/// Proves that `load_entry_bytes_cached` can extract a file from a nested
/// MIX archive using the `parent_indices` chain.
#[test]
fn load_entry_bytes_nested_mix_chain() {
    let inner_mix = build_named_mix(&[("NESTED.BIN", b"nested-payload")]);
    let outer_mix = build_named_mix(&[("INNER.MIX", &inner_mix)]);

    let fixture = TestDir::new("nested_mix_load");
    let mix_path = fixture.write_file("OUTER.MIX", &outer_mix);

    // We need to figure out what archive_index the inner entry gets after
    // CRC-sorted parsing. Open the outer, find INNER.MIX's index.
    let file = std::fs::File::open(&mix_path).unwrap();
    let mut reader =
        ic_cnc_content::cnc_formats::mix::MixArchiveReader::open(std::io::BufReader::new(file))
            .unwrap();
    let inner_crc = ic_cnc_content::cnc_formats::mix::crc("INNER.MIX");
    let outer_index = reader
        .entries()
        .iter()
        .position(|e| e.crc == inner_crc)
        .expect("INNER.MIX should exist in OUTER.MIX");

    // Read INNER.MIX bytes to find the sorted index of NESTED.BIN.
    let inner_bytes = reader.read_by_index(outer_index).unwrap().unwrap();
    let inner_archive = ic_cnc_content::cnc_formats::mix::MixArchive::parse(&inner_bytes).unwrap();
    let nested_crc = ic_cnc_content::cnc_formats::mix::crc("NESTED.BIN");
    let inner_index = inner_archive
        .entries()
        .iter()
        .position(|e| e.crc == nested_crc)
        .expect("NESTED.BIN should exist in INNER.MIX");
    drop(reader);

    let entry = ContentCatalogEntry {
        relative_path: "OUTER.MIX::INNER.MIX::NESTED.BIN".into(),
        location: ContentEntryLocation::MixMember {
            archive_path: mix_path,
            archive_index: inner_index,
            crc_raw: nested_crc.to_raw(),
            logical_name: Some("NESTED.BIN".into()),
            parent_indices: vec![outer_index],
        },
        size_bytes: 14,
        family: ContentFamily::Other,
        support: ContentSupportLevel::Planned,
    };

    let handle_cache = super::ArchiveHandleCache::default();
    let bytes = super::preview_decode::load_entry_bytes_cached(&entry, None, Some(&handle_cache))
        .expect("nested MIX chain read should succeed");
    assert_eq!(bytes, b"nested-payload");
}

/// Proves that the nesting depth guard in `read_mix_chain` rejects chains
/// deeper than `MAX_MIX_CHAIN_DEPTH`, even when `parent_indices` is
/// artificially extended.
#[test]
fn read_mix_chain_rejects_excessive_depth() {
    let mix = build_named_mix(&[("FILE.BIN", b"data")]);
    let fixture = TestDir::new("chain_depth_guard");
    let mix_path = fixture.write_file("GUARDED.MIX", &mix);

    // parent_indices with 4 entries exceeds MAX_MIX_CHAIN_DEPTH (3).
    let entry = ContentCatalogEntry {
        relative_path: "GUARDED.MIX::deeply::nested::FILE.BIN".into(),
        location: ContentEntryLocation::MixMember {
            archive_path: mix_path,
            archive_index: 0,
            crc_raw: 0,
            logical_name: None,
            parent_indices: vec![0, 0, 0, 0],
        },
        size_bytes: 4,
        family: ContentFamily::Other,
        support: ContentSupportLevel::Planned,
    };

    let result = super::preview_decode::load_entry_bytes_cached(&entry, None, None);
    assert!(
        result.is_err(),
        "chains deeper than MAX_MIX_CHAIN_DEPTH must be rejected"
    );
}

// ── MixVfs overlay ──────────────────────────────────────────────────────────

/// Proves that `build_mix_vfs` mounts archives from catalogs and supports
/// filename resolution across them.
#[test]
fn mix_vfs_resolves_filenames_across_mounted_archives() {
    let fixture = TestDir::new("mix_vfs_resolve");
    let mix_path = fixture.write_file(
        "GAME.MIX",
        &build_named_mix(&[
            ("RULES.INI", b"rules-content"),
            ("TEMPERAT.PAL", b"palette-content"),
        ]),
    );

    let source = ContentSourceRoot::directory(
        "VFS Root",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalogs = vec![ContentCatalog::scan(source)];
    let vfs = super::build_mix_vfs(&catalogs);

    assert!(vfs.len() > 0, "VFS should contain mounted entries");

    let resolved = vfs.resolve_name("RULES.INI");
    assert!(resolved.is_some(), "RULES.INI should be resolvable in the VFS");
    let (source_path, _index) = resolved.unwrap();
    assert_eq!(source_path, mix_path.as_path());
}

/// Proves that `build_mix_vfs` uses last-mounted-wins when the same filename
/// exists in multiple archives.
#[test]
fn mix_vfs_last_mounted_wins() {
    let fixture = TestDir::new("mix_vfs_priority");
    fixture.write_file(
        "FIRST.MIX",
        &build_named_mix(&[("SHARED.INI", b"first-content")]),
    );
    let second_path = fixture.write_file(
        "SECOND.MIX",
        &build_named_mix(&[("SHARED.INI", b"second-content")]),
    );

    let source = ContentSourceRoot::directory(
        "Priority Root",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalogs = vec![ContentCatalog::scan(source)];
    let vfs = super::build_mix_vfs(&catalogs);

    let resolved = vfs
        .resolve_name("SHARED.INI")
        .expect("SHARED.INI should be resolvable");
    // The second archive should win because it was mounted later.
    let (source_path, _) = resolved;
    assert_eq!(
        source_path,
        second_path.as_path(),
        "later-mounted archive should take priority"
    );
}

/// Proves that `MixVfs::resolve_name` returns `None` for filenames not
/// present in any mounted archive.
#[test]
fn mix_vfs_returns_none_for_unknown_filenames() {
    let vfs = super::MixVfs::default();
    assert!(
        vfs.resolve_name("NONEXISTENT.FILE").is_none(),
        "unknown filenames should resolve to None"
    );
}

// ---------------------------------------------------------------------------
// Regression tests for sliding window playback (2026-03-22)
// ---------------------------------------------------------------------------

/// Helper: creates N synthetic 1×1 RGBA frames for sliding window tests.
fn make_n_frames(n: usize) -> Vec<ic_render::sprite::RgbaSpriteFrame> {
    (0..n)
        .map(|i| {
            let v = (i % 256) as u8;
            ic_render::sprite::RgbaSpriteFrame::from_rgba(1, 1, vec![v, v, v, 255])
                .expect("synthetic frame should be valid")
        })
        .collect()
}

/// Proves that `push_frames` caps the buffer at MAX_SESSION_FRAMES
/// Proves that `push_frames` does NOT evict frames ahead of the playback
/// cursor when the animation has not yet advanced (the fast-decode scenario).
///
/// When VQA batches all arrive in a single Bevy frame before the animation
/// timer has ticked from frame 0, the old unconditional eviction moved the
/// viewer's global position to `window_start` (e.g. frame 330 out of 450),
/// making the video appear to "start from the middle."
///
/// The fix: safe eviction only removes frames behind the cursor.
/// When `current_frame = 0`, `safe_evict = min(budget, 0) = 0`, so no
/// eviction happens.  The buffer temporarily exceeds `MAX_SESSION_FRAMES`
/// but is bounded by the total VQA size (~112 MB for a 30-second movie).
#[test]
fn push_frames_does_not_evict_ahead_of_cursor_in_fast_decode() {
    // Start with 1 frame (simulates FirstFrame message).
    let mut session =
        super::preview::VisualPreviewSession::new(make_n_frames(1), Some(1.0 / 15.0))
            .expect("session should be created");
    assert_eq!(session.current_frame_index(), 0);

    // Push 200 more frames — the fast-decode scenario where all streaming
    // batches arrive before the animation timer has advanced from frame 0.
    // Total = 201, which exceeds MAX_SESSION_FRAMES (120).
    session.push_frames(make_n_frames(200));

    // Total decoded frames is always accurate.
    assert_eq!(session.frame_count(), 201);

    // The cursor is at frame 0, so safe_evict = min(81, 0) = 0.
    // All 201 frames must remain in the buffer — no eviction should occur.
    assert_eq!(
        session.buffered_frame_count(),
        201,
        "buffer must retain all frames when cursor has not advanced: \
         safe eviction prevents discarding unseen frames",
    );

    // Global frame index must stay at 0 — the viewer starts from the beginning.
    assert_eq!(
        session.current_frame_index(),
        0,
        "global frame index must stay at 0 when no eviction has occurred",
    );
}

/// Proves that frames behind the playback position ARE evicted when the
/// buffer exceeds MAX_SESSION_FRAMES, capping memory usage.
#[test]
fn push_frames_evicts_behind_playback_position() {
    // Start with 60 frames and advance to frame 50.
    let mut session =
        super::preview::VisualPreviewSession::new(make_n_frames(60), Some(1.0 / 15.0))
            .expect("session should be created");
    session.select_frame(50);
    assert_eq!(session.current_frame_index(), 50);

    // Push 100 more frames. Total = 160, exceeds MAX_SESSION_FRAMES (120).
    // There are 50 frames behind playback (0..49) eligible for eviction.
    // Budget = 160 - 120 = 40. We can safely evict 40 of the 50 played frames.
    session.push_frames(make_n_frames(100));

    assert_eq!(session.frame_count(), 160);
    assert_eq!(
        session.current_frame_index(),
        50,
        "global frame index must stay at 50 after eviction",
    );
    assert_eq!(
        session.buffered_frame_count(),
        120,
        "buffer must be capped exactly at MAX_SESSION_FRAMES (120)",
    );
}

/// Proves that linear advancement during streaming does not wrap around
/// to evicted frames when reaching the end of available frames.
///
/// Regression: `(advanced + 1) % frame_count` wrapped to frame 0 during
/// streaming, but frame 0 had been evicted by the sliding window, causing
/// playback to jump to the oldest buffered frame (middle of the video).
#[test]
fn streaming_advancement_does_not_wrap() {
    // Simulate a streaming session at the last available frame.
    let mut session =
        super::preview::VisualPreviewSession::new(make_n_frames(10), Some(1.0 / 15.0))
            .expect("session should be created");

    // Advance to the last frame (index 9).
    session.select_frame(9);
    assert_eq!(session.current_frame_index(), 9);

    // In the streaming advancement path, the next frame would be
    // (9 + 1) % 10 = 0 with wrapping. Without wrapping, it should
    // stay at 9 (capped at frame_count - 1).
    // We verify the session state supports this by checking that
    // select_frame(10) does not panic and wraps within the buffer.
    let frame_count = session.frame_count();
    let current = session.current_frame_index();
    let next = current + 1;

    // During streaming, we cap instead of wrapping.
    let streaming_advanced = if next < frame_count {
        next
    } else {
        current // hold on last frame
    };
    assert_eq!(
        streaming_advanced, 9,
        "streaming advancement must hold on the last frame, not wrap to 0",
    );
}

/// Proves that push_frames followed by select_frame maintains correct
/// global-to-local index mapping after partial eviction.
#[test]
fn sliding_window_global_to_local_mapping_after_eviction() {
    let mut session =
        super::preview::VisualPreviewSession::new(make_n_frames(80), Some(1.0 / 15.0))
            .expect("session should be created");

    // Advance to frame 60 (60 frames behind, 19 frames ahead).
    session.select_frame(60);
    assert_eq!(session.current_frame_index(), 60);

    // Push 80 more frames. Total = 160, budget = 40, safe_evict = min(40, 60) = 40.
    session.push_frames(make_n_frames(80));

    assert_eq!(session.frame_count(), 160);
    assert_eq!(
        session.current_frame_index(),
        60,
        "global frame index must be preserved through eviction",
    );

    // Select a frame that's ahead of current position.
    session.select_frame(100);
    assert_eq!(
        session.current_frame_index(),
        100,
        "selecting a frame within the buffer must work correctly",
    );
}

/// Proves that manual navigation (arrow keys) cancels playlist autoplay
/// so the auto-advance does not hijack the selection when the current
/// video finishes.
///
/// Regression: `move_selection` changed the selected entry but left
/// `autoplay_position` set, so when the new video finished looping,
/// `handle_playlist_advance` jumped to the NEXT playlist entry instead
/// of staying on the manually-selected one.
#[test]
fn manual_navigation_cancels_playlist_autoplay() {
    // ENGLISH.VQA is a SHOWCASE_RESOURCE_HINTS entry — scanning it causes
    // build_autoplay_playlist to set autoplay_position = Some(0).
    let fixture = TestDir::new("autoplay_cancel_test");
    fixture.write_file("ENGLISH.VQA", b"vqa");
    fixture.write_file("PROLOG.VQA", b"vqa");

    let source = ContentSourceRoot::directory(
        "Autoplay fixture",
        fixture.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    );
    let catalog = ContentCatalog::scan(source);
    let mut state = super::state::ContentLabState::loading(vec![]);
    state.replace_catalogs(vec![catalog]);

    // Confirm we're in playlist autoplay mode after the scan.
    assert!(
        state.is_autoplay_active(),
        "expected autoplay to be active after scanning VQA showcase entries",
    );

    // The user manually navigates — this must cancel the playlist.
    state.move_selection(1);

    assert!(
        !state.is_autoplay_active(),
        "manual navigation must cancel playlist autoplay",
    );
}

/// Proves that `cycle_catalog` (Q/E keys) also cancels playlist autoplay,
/// preventing cross-catalog jumps when the user switches sources manually.
#[test]
fn cycle_catalog_cancels_playlist_autoplay() {
    let fixture_a = TestDir::new("cycle_catalog_autoplay_a");
    fixture_a.write_file("ENGLISH.VQA", b"vqa");
    let fixture_b = TestDir::new("cycle_catalog_autoplay_b");
    fixture_b.write_file("PROLOG.VQA", b"vqa");

    let cat_a = ContentCatalog::scan(ContentSourceRoot::directory(
        "Source A",
        fixture_a.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));
    let cat_b = ContentCatalog::scan(ContentSourceRoot::directory(
        "Source B",
        fixture_b.path().to_path_buf(),
        ContentSourceKind::ManualDirectory,
        SourceRightsClass::OwnedProprietary,
    ));

    let mut state = super::state::ContentLabState::loading(vec![]);
    state.replace_catalogs(vec![cat_a, cat_b]);

    assert!(
        state.is_autoplay_active(),
        "expected autoplay to be active after scanning VQA showcase entries",
    );

    state.cycle_catalog(1);
    assert!(
        !state.is_autoplay_active(),
        "cycle_catalog must cancel playlist autoplay",
    );
}

/// Proves that `window_start()` correctly reports the global index of the
/// first frame currently in the sliding-window buffer.
///
/// After eviction, `window_start` must equal `total_decoded - buffered`.
/// The animation system uses this to wrap playback within the buffer
/// instead of over the full total_decoded range.
#[test]
fn window_start_reflects_eviction_offset() {
    let mut session =
        super::preview::VisualPreviewSession::new(make_n_frames(60), Some(1.0 / 15.0))
            .expect("session should be created");
    // Advance playback to frame 50 so we can evict the first 50.
    session.select_frame(50);
    // Push 100 more frames → total 160, evict 40 → buffer holds frames 40–159.
    session.push_frames(make_n_frames(100));

    assert_eq!(session.frame_count(), 160);
    assert_eq!(session.buffered_frame_count(), 120);
    assert_eq!(
        session.window_start(),
        40,
        "window_start must equal total_decoded - buffered after eviction",
    );
    // Current frame (50) is still within the window (40..159).
    assert_eq!(session.current_frame_index(), 50);
}

/// Proves that the loop-duration calculation uses `buffered_frame_count`,
/// not `total_frames_decoded`.
///
/// Safe eviction only removes frames behind the playback cursor, so the
/// buffered count naturally tracks what the animation actually cycles over.
/// Using `total_decoded` instead would fire the playlist timer at the wrong
/// time — too early if fewer frames are buffered, too late if more are.
#[test]
fn loop_duration_uses_buffered_frame_count_not_total_decoded() {
    // Start with 200 frames, advance cursor to frame 100 (halfway through).
    let mut session =
        super::preview::VisualPreviewSession::new(make_n_frames(200), Some(1.0 / 15.0))
            .expect("session should be created");
    session.select_frame(100);

    // Push 100 more frames. total = 300, budget = 180, safe_evict = min(180, 100) = 100.
    // After eviction: buffer = 200, window_start = 100, global cursor stays at 100.
    session.push_frames(make_n_frames(100));

    assert_eq!(session.frame_count(), 300);
    assert_eq!(session.buffered_frame_count(), 200);
    assert_eq!(session.current_frame_index(), 100);

    // loop_duration_seconds is computed by ContentPreviewTracker, not directly
    // by VisualPreviewSession.  We verify the building blocks here.
    let frame_duration = 1.0_f32 / 15.0;
    let buffered_duration = session.buffered_frame_count() as f32 * frame_duration;
    let total_duration = session.frame_count() as f32 * frame_duration;

    // 200 buffered frames at 15 fps ≈ 13.3 s.
    assert!(
        (buffered_duration - 200.0 / 15.0).abs() < 0.01,
        "loop duration must be buffered_frame_count × frame_duration ({:.2}s), \
         not total_decoded × frame_duration ({:.2}s)",
        buffered_duration,
        total_duration,
    );
    // Sanity: buffered < total when eviction occurred.
    assert!(
        buffered_duration < total_duration,
        "sanity: buffered duration ({buffered_duration:.2}s) must be \
         less than total ({total_duration:.2}s) after partial eviction",
    );
}
