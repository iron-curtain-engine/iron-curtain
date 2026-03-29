// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Scrollable thumbnail gallery and focused inspector for the content lab.
//!
//! The first gallery pass proved that we could render many resources on one
//! screen, but it stretched every preview to the same rectangle. That is fast
//! to prototype and bad for validation: a stretched VQA frame or SHP sprite no
//! longer tells the reader what the original asset actually looks like.
//!
//! This module owns two complementary presentation surfaces:
//! - a scrollable wall of static thumbnails with filename captions
//! - a larger focused preview pane for the currently selected resource
//!
//! Both surfaces use "contain" sizing rather than distortion: keep the source
//! aspect ratio, fit within the available box, and center the result. That is
//! closer to how a real asset browser or player should treat mixed-format media
//! on modern widescreen displays.

use std::path::Path;

use bevy::prelude::*;
use bevy::ui::widget::ImageNode;

use super::preview::{load_preview_for_entry, rgba_frame_to_image, ContentPreviewTracker};
use super::state::{ContentGalleryWindow, ContentLabState, ContentWindowUiRoot};

const GALLERY_LEFT: f32 = 332.0;
const GALLERY_RIGHT: f32 = 388.0;
const GALLERY_TOP: f32 = 16.0;
const GALLERY_BOTTOM: f32 = 16.0;
const GALLERY_TILE_WIDTH: f32 = 270.0;
const GALLERY_TILE_IMAGE_HEIGHT: f32 = 164.0;
const GALLERY_TILE_PADDING: f32 = 10.0;
const GALLERY_TILE_GAP: f32 = 16.0;
const GALLERY_IMAGE_INSET: f32 = 8.0;
const GALLERY_LABEL_TEXT_SIZE: f32 = 14.0;
const INSPECTOR_WIDTH: f32 = 356.0;
const INSPECTOR_HEIGHT: f32 = 308.0;
const INSPECTOR_IMAGE_MAX_WIDTH: f32 = INSPECTOR_WIDTH - 32.0;
const INSPECTOR_IMAGE_MAX_HEIGHT: f32 = INSPECTOR_HEIGHT - 88.0;
const INSPECTOR_TITLE_TEXT_SIZE: f32 = 15.0;

/// Width/height pair that keeps a decoded preview inside one UI box without
/// distorting the original image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainedImageSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl ContainedImageSize {
    /// Computes a "contain" fit for one source image inside one destination
    /// rectangle.
    ///
    /// The result preserves aspect ratio. It uses as much of the available box
    /// as possible without cropping or stretching the source.
    pub(crate) fn for_source(
        source_width: u32,
        source_height: u32,
        max_width: f32,
        max_height: f32,
    ) -> Self {
        if source_width == 0 || source_height == 0 || max_width <= 0.0 || max_height <= 0.0 {
            return Self {
                width: max_width.max(1.0),
                height: max_height.max(1.0),
            };
        }

        let x_scale = max_width / source_width as f32;
        let y_scale = max_height / source_height as f32;
        let scale = x_scale.min(y_scale);

        Self {
            width: (source_width as f32 * scale).max(1.0),
            height: (source_height as f32 * scale).max(1.0),
        }
    }
}

/// Root panel that owns the visible thumbnail tiles.
#[derive(Component)]
pub(crate) struct ContentGalleryRoot;

/// Stable root for the larger selected-resource inspector/player pane.
#[derive(Component)]
pub(crate) struct ContentInspectorRoot;

/// Marker for one spawned gallery tile.
#[derive(Component)]
pub(crate) struct ContentGalleryTile;

/// Marker for the selected inspector image that transport systems mutate as
/// animations advance.
#[derive(Component)]
pub(crate) struct ContentInspectorImage;

/// Marker applied to all transient entities that are rebuilt on selection
/// changes.
#[derive(Component)]
pub(crate) struct ContentGalleryTransient;

/// Tracks transient thumbnail images created for the visible gallery window.
///
/// The selected entry keeps its own decoded frames in `ContentPreviewTracker`
/// because those handles advance during playback. Non-selected thumbnails are
/// static first-frame images, so the gallery owns and cleans them up.
#[derive(Resource, Debug, Default)]
pub(crate) struct ContentGalleryTracker {
    current_signature: Option<ContentGallerySignature>,
    thumbnail_handles: Vec<Handle<Image>>,
}

impl ContentGalleryTracker {
    fn clear(&mut self, images: &mut Assets<Image>) {
        for handle in self.thumbnail_handles.drain(..) {
            images.remove(handle.id());
        }
        self.current_signature = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContentGallerySignature {
    catalog_index: usize,
    selected_window_index: usize,
    entry_indices: Vec<usize>,
}

struct TileUiSpec {
    image_handle: Option<Handle<Image>>,
    image_size: Option<ContainedImageSize>,
    selected: bool,
    caption: String,
    status: String,
}

struct InspectorUiSpec {
    image_handle: Option<Handle<Image>>,
    image_size: Option<ContainedImageSize>,
    caption: String,
    subtitle: String,
}

/// Spawns the stable gallery and inspector roots.
///
/// Their children are rebuilt later by `refresh_content_gallery`, but the root
/// nodes stay stable so the rest of the UI layout can reason about fixed left,
/// center, and right content zones.
pub(crate) fn setup_content_gallery_ui(
    mut commands: Commands,
    ui_root_query: Query<Entity, With<ContentWindowUiRoot>>,
) {
    let Some(ui_root) = ui_root_query.single().ok() else {
        return;
    };

    commands.entity(ui_root).with_children(|parent| {
        parent.spawn((
            ContentGalleryRoot,
            Node {
                position_type: PositionType::Absolute,
                top: px(GALLERY_TOP),
                left: px(GALLERY_LEFT),
                right: px(GALLERY_RIGHT),
                bottom: px(GALLERY_BOTTOM),
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                align_items: AlignItems::FlexStart,
                row_gap: px(GALLERY_TILE_GAP),
                column_gap: px(GALLERY_TILE_GAP),
                padding: UiRect::all(px(8)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.07, 0.09, 0.78)),
        ));

        parent.spawn((
            ContentInspectorRoot,
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                right: px(16),
                width: px(INSPECTOR_WIDTH),
                height: px(INSPECTOR_HEIGHT),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(12)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.08, 0.10, 0.84)),
        ));
    });
}

/// Rebuilds the visible thumbnail wall and focused inspector when selection or
/// source-window state changes.
#[allow(
    clippy::too_many_arguments,
    reason = "Bevy system functions naturally accumulate several ECS resources and queries; keeping the gallery refresh in one system keeps the selection-to-UI rebuild path readable."
)]
pub(crate) fn refresh_content_gallery(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    state: Res<ContentLabState>,
    mut preview_tracker: ResMut<ContentPreviewTracker>,
    mut gallery_tracker: ResMut<ContentGalleryTracker>,
    archive_cache: Res<super::ArchivePreloadCache>,
    handle_cache: Res<super::ArchiveHandleCache>,
    gallery_root_query: Query<Entity, With<ContentGalleryRoot>>,
    inspector_root_query: Query<Entity, With<ContentInspectorRoot>>,
    existing_transient_query: Query<Entity, With<ContentGalleryTransient>>,
) {
    let Some(gallery_root) = gallery_root_query.single().ok() else {
        return;
    };
    let Some(inspector_root) = inspector_root_query.single().ok() else {
        return;
    };

    let gallery_window = state.gallery_window();
    let next_signature = Some(signature_for_gallery_state(&state, gallery_window.as_ref()));
    if gallery_tracker.current_signature == next_signature {
        return;
    }

    for entity in &existing_transient_query {
        commands.entity(entity).despawn();
    }
    gallery_tracker.clear(&mut images);
    preview_tracker.selected_image_entity = None;

    let Some(gallery_window) = gallery_window else {
        spawn_empty_gallery_state(&mut commands, gallery_root, inspector_root, &state);
        gallery_tracker.current_signature = next_signature;
        return;
    };

    let tile_specs = build_tile_specs(
        &gallery_window,
        state.catalogs(),
        &mut images,
        &preview_tracker,
        &mut gallery_tracker,
        &archive_cache,
        &handle_cache,
    );
    let inspector_spec = build_inspector_spec(state.selected_entry(), &preview_tracker);

    commands.entity(gallery_root).with_children(|parent| {
        for spec in tile_specs {
            let tile_background = if spec.selected {
                Color::srgba(0.20, 0.16, 0.08, 0.96)
            } else {
                Color::srgba(0.11, 0.12, 0.14, 0.90)
            };
            let tile_border = if spec.selected {
                Color::srgb(0.92, 0.78, 0.28)
            } else {
                Color::srgba(0.34, 0.37, 0.40, 0.92)
            };

            parent
                .spawn((
                    ContentGalleryTransient,
                    ContentGalleryTile,
                    Node {
                        width: px(GALLERY_TILE_WIDTH),
                        min_height: px(GALLERY_TILE_IMAGE_HEIGHT + 64.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8),
                        padding: UiRect::all(px(GALLERY_TILE_PADDING)),
                        border: UiRect::all(px(if spec.selected { 3.0 } else { 1.0 })),
                        ..default()
                    },
                    BackgroundColor(tile_background),
                    BorderColor::all(tile_border),
                ))
                .with_children(|tile_parent| {
                    tile_parent
                        .spawn((
                            Node {
                                width: percent(100),
                                height: px(GALLERY_TILE_IMAGE_HEIGHT),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                overflow: Overflow::clip(),
                                padding: UiRect::all(px(GALLERY_IMAGE_INSET / 2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.94)),
                        ))
                        .with_children(|image_parent| {
                            if let Some(image_handle) = spec.image_handle.clone() {
                                image_parent.spawn((
                                    ImageNode::new(image_handle),
                                    Node {
                                        width: px(spec.image_size.map_or(96.0, |size| size.width)),
                                        height: px(spec
                                            .image_size
                                            .map_or(96.0, |size| size.height)),
                                        ..default()
                                    },
                                ));
                            } else {
                                image_parent.spawn((
                                    Text::new(spec.status.clone()),
                                    TextFont {
                                        font_size: GALLERY_LABEL_TEXT_SIZE,
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.84, 0.46, 0.40)),
                                    Node {
                                        max_width: percent(100),
                                        ..default()
                                    },
                                ));
                            }
                        });

                    tile_parent.spawn((
                        Text::new(format!("{}\n{}", spec.caption, spec.status)),
                        TextFont {
                            font_size: GALLERY_LABEL_TEXT_SIZE,
                            ..default()
                        },
                        TextColor(Color::srgb(0.90, 0.91, 0.92)),
                        Node {
                            width: percent(100),
                            ..default()
                        },
                    ));
                });
        }
    });

    let mut selected_image_entity = None;
    commands.entity(inspector_root).with_children(|parent| {
        parent
            .spawn((
                ContentGalleryTransient,
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                },
            ))
            .with_children(|panel_parent| {
                panel_parent.spawn((
                    Text::new(format!(
                        "Focused Preview\n{}\n{}",
                        inspector_spec.caption, inspector_spec.subtitle
                    )),
                    TextFont {
                        font_size: INSPECTOR_TITLE_TEXT_SIZE,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.93, 0.94)),
                    Node {
                        width: percent(100),
                        ..default()
                    },
                ));

                panel_parent
                    .spawn((
                        Node {
                            width: percent(100),
                            height: px(INSPECTOR_IMAGE_MAX_HEIGHT + 16.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            overflow: Overflow::clip(),
                            padding: UiRect::all(px(8)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.94)),
                    ))
                    .with_children(|image_parent| {
                        if let Some(image_handle) = inspector_spec.image_handle {
                            let image_entity = image_parent
                                .spawn((
                                    ContentInspectorImage,
                                    ImageNode::new(image_handle),
                                    Node {
                                        width: px(inspector_spec
                                            .image_size
                                            .map_or(128.0, |size| size.width)),
                                        height: px(inspector_spec
                                            .image_size
                                            .map_or(128.0, |size| size.height)),
                                        ..default()
                                    },
                                ))
                                .id();
                            selected_image_entity = Some(image_entity);
                        } else {
                            image_parent.spawn((
                                Text::new("No visual surface for the selected resource."),
                                TextFont {
                                    font_size: GALLERY_LABEL_TEXT_SIZE,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.84, 0.46, 0.40)),
                            ));
                        }
                    });
            });
    });

    preview_tracker.selected_image_entity = selected_image_entity;
    gallery_tracker.current_signature = next_signature;
}

fn signature_for_gallery_state(
    state: &ContentLabState,
    gallery_window: Option<&ContentGalleryWindow>,
) -> ContentGallerySignature {
    match gallery_window {
        Some(gallery_window) => ContentGallerySignature {
            catalog_index: gallery_window.catalog_index,
            selected_window_index: gallery_window.selected_window_index,
            entry_indices: gallery_window.entry_indices.clone(),
        },
        None => ContentGallerySignature {
            catalog_index: state.selected_catalog_index(),
            selected_window_index: 0,
            entry_indices: Vec::new(),
        },
    }
}

fn spawn_empty_gallery_state(
    commands: &mut Commands,
    gallery_root: Entity,
    inspector_root: Entity,
    state: &ContentLabState,
) {
    let source_message = if state.is_loading() {
        let mut lines = vec![
            "Scanning Red Alert / Remastered content...".to_string(),
            "The window is open; the catalog is still building in the background.".to_string(),
            String::new(),
            "Configured source roots:".to_string(),
        ];
        for source in state.configured_sources() {
            lines.push(format!(
                "- {}: {}",
                source.display_name,
                source.path.display()
            ));
        }
        lines.join("\n")
    } else {
        state
            .catalogs()
            .get(state.selected_catalog_index())
            .map(|catalog| {
                format!(
                    "No previewable visual resources were found in the current source.\n\nSource: {}\nPath: {}\nStatus: {}\n\nUse Q / E to switch source roots.\nIf this path is wrong on your machine, set:\nIC_RA1_SAMPLE_DISC_ROOT\nIC_RA1_SAMPLE_RAR\nIC_RA1_SAMPLE_PALETTES\nIC_REMASTERED_ROOT",
                    catalog.source.display_name,
                    catalog.source.path.display(),
                    if catalog.available { "available" } else { "missing" },
                )
            })
            .unwrap_or_else(|| {
                "No content sources are configured.\n\nSet IC_RA1_SAMPLE_DISC_ROOT or IC_REMASTERED_ROOT before starting the content lab.".into()
            })
    };

    commands.entity(gallery_root).with_children(|parent| {
        parent
            .spawn((
                ContentGalleryTransient,
                Node {
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(px(24)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.09, 0.78)),
            ))
            .with_children(|message_parent| {
                message_parent.spawn((
                    Text::new(source_message),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.93, 0.94)),
                    Node {
                        max_width: px(780.0),
                        ..default()
                    },
                ));
            });
    });

    commands.entity(inspector_root).with_children(|parent| {
        parent
            .spawn((
                ContentGalleryTransient,
                Node {
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(px(12)),
                    ..default()
                },
            ))
            .with_children(|message_parent| {
                message_parent.spawn((
                    Text::new(if state.is_loading() {
                        "Focused Preview\nWaiting for the content scan to finish."
                    } else {
                        "Focused Preview\nNo visual selection is available yet."
                    }),
                    TextFont {
                        font_size: INSPECTOR_TITLE_TEXT_SIZE,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.93, 0.94)),
                ));
            });
    });
}

fn build_tile_specs(
    gallery_window: &ContentGalleryWindow,
    catalogs: &[super::catalog::ContentCatalog],
    images: &mut Assets<Image>,
    preview_tracker: &ContentPreviewTracker,
    gallery_tracker: &mut ContentGalleryTracker,
    archive_cache: &super::ArchivePreloadCache,
    handle_cache: &super::ArchiveHandleCache,
) -> Vec<TileUiSpec> {
    let Some(catalog) = catalogs.get(gallery_window.catalog_index) else {
        return Vec::new();
    };

    gallery_window
        .entry_indices
        .iter()
        .enumerate()
        .filter_map(|(window_index, entry_index)| {
            let entry = catalog.entries.get(*entry_index)?;
            let selected = window_index == gallery_window.selected_window_index;

            let (image_handle, image_size) = if selected {
                (
                    preview_tracker.current_image_handle(),
                    preview_tracker.frame_dimensions().map(|(width, height)| {
                        ContainedImageSize::for_source(
                            width,
                            height,
                            tile_media_max_width(),
                            GALLERY_TILE_IMAGE_HEIGHT - GALLERY_IMAGE_INSET,
                        )
                    }),
                )
            } else {
                build_static_thumbnail(entry, catalogs, images, gallery_tracker, archive_cache, handle_cache)
            };

            Some(TileUiSpec {
                image_handle,
                image_size,
                selected,
                caption: tile_caption(entry),
                status: tile_status(entry),
            })
        })
        .collect()
}

fn build_static_thumbnail(
    entry: &super::catalog::ContentCatalogEntry,
    catalogs: &[super::catalog::ContentCatalog],
    images: &mut Assets<Image>,
    gallery_tracker: &mut ContentGalleryTracker,
    archive_cache: &super::ArchivePreloadCache,
    handle_cache: &super::ArchiveHandleCache,
) -> (Option<Handle<Image>>, Option<ContainedImageSize>) {
    // Heavy formats (VQA, WSA, AUD) are too expensive to decode
    // synchronously for gallery thumbnails.  The selected entry uses
    // the background preview tracker instead.
    if super::preview::should_background_load_preview(entry) {
        return (None, None);
    }
    match load_preview_for_entry(entry, catalogs, Some(archive_cache), Some(handle_cache)) {
        Ok(Some(preview)) => preview.visual().map_or((None, None), |visual| {
            let Some(first_frame) = visual.frames().first() else {
                return (None, None);
            };
            let image_handle = images.add(rgba_frame_to_image(first_frame));
            gallery_tracker.thumbnail_handles.push(image_handle.clone());
            (
                Some(image_handle),
                Some(ContainedImageSize::for_source(
                    first_frame.width(),
                    first_frame.height(),
                    tile_media_max_width(),
                    GALLERY_TILE_IMAGE_HEIGHT - GALLERY_IMAGE_INSET,
                )),
            )
        }),
        Ok(None) | Err(_) => (None, None),
    }
}

fn build_inspector_spec(
    selected_entry: Option<&super::catalog::ContentCatalogEntry>,
    preview_tracker: &ContentPreviewTracker,
) -> InspectorUiSpec {
    InspectorUiSpec {
        image_handle: preview_tracker.current_image_handle(),
        image_size: preview_tracker.frame_dimensions().map(|(width, height)| {
            ContainedImageSize::for_source(
                width,
                height,
                INSPECTOR_IMAGE_MAX_WIDTH,
                INSPECTOR_IMAGE_MAX_HEIGHT,
            )
        }),
        caption: selected_entry
            .map(tile_caption)
            .unwrap_or_else(|| "No selected resource".into()),
        subtitle: selected_entry
            .map(tile_status)
            .unwrap_or_else(|| "none".into()),
    }
}

fn tile_media_max_width() -> f32 {
    GALLERY_TILE_WIDTH - (GALLERY_TILE_PADDING * 2.0) - GALLERY_IMAGE_INSET
}

fn tile_caption(entry: &super::catalog::ContentCatalogEntry) -> String {
    entry
        .location
        .logical_name()
        .map(ToOwned::to_owned)
        .or_else(|| {
            Path::new(&entry.relative_path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| entry.relative_path.clone())
}

fn tile_status(entry: &super::catalog::ContentCatalogEntry) -> String {
    entry.family.to_string()
}
