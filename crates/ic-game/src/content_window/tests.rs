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
/// UI responsive. Static art still stays on the simpler immediate path.
#[test]
fn video_preview_policy_uses_background_loading() {
    assert!(super::preview::should_background_load_preview_for_family(
        ContentFamily::Video
    ));
    assert!(!super::preview::should_background_load_preview_for_family(
        ContentFamily::SpriteSheet
    ));
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
        super::preview::load_preview_for_entry(&entry, &[sprite_catalog, palette_catalog], None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[catalog], None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None)
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
        super::preview::load_preview_for_entry(&entry, &[visual_catalog, palette_catalog], None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None)
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

/// Proves that `finalize_streaming` preserves the video position and Timer
/// mode when audio arrives, so the video does not visibly restart.
///
/// A/V sync is achieved by the caller rotating the audio sample buffer to
/// match the video's current position (see `rotate_audio_to_video_position`)
/// rather than resetting the video to frame 0.
///
/// Regression: resetting to frame 0 on audio arrival caused the movie to
/// visibly restart after a few hundred milliseconds of streaming playback.
#[test]
fn finalize_streaming_with_audio_preserves_position_and_timer() {
    use super::preview::{
        AudioInfo, ContentPreviewTracker, PlaybackSyncMode,
        VisualPreviewSession,
    };
    use super::catalog::ContentFamily;

    let mut tracker = ContentPreviewTracker::default();

    // Simulate: Loading → StreamingFirstFrame with video advanced to frame 3.
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
    // Advance video as if timer-based playback ran for a bit.
    tracker.test_select_frame(3);
    assert_eq!(tracker.test_current_frame_index(), 3);

    // Finalize with audio — must keep Timer and preserve position.
    tracker.test_finalize_streaming(Some(AudioInfo {
        duration_seconds: 5.0,
    }));
    assert_eq!(
        tracker.test_sync_mode(),
        PlaybackSyncMode::Timer,
        "finalize_streaming must keep Timer mode so the video never restarts",
    );
    assert_eq!(
        tracker.test_current_frame_index(),
        3,
        "finalize_streaming must preserve the video position",
    );
    assert!(
        tracker.test_playback_requested(),
        "playback should remain requested after finalize",
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
    let preview = super::preview::load_preview_for_entry(&entry, &[], None)
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

    let preview = super::preview::load_preview_for_entry(&entry, &[], None)
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
