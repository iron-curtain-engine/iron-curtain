// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Mutable UI/navigation state for the content lab.
//!
//! The catalog scanner is intentionally Bevy-free, but the running content lab
//! stores its current selection as a Bevy `Resource` so keyboard-input systems,
//! preview systems, and text-panel refresh systems can all coordinate through
//! one canonical state object.
//!
//! The GUI is still intentionally text-heavy. That is a feature here: this
//! window is a validation tool for "can we decode and use this resource?",
//! not a final player-facing UI skin. The panels are split so a maintainer can
//! inspect source summary, entry list, selected-entry details, and runtime
//! playback status without scrolling one monolithic text block.

use bevy::app::AppExit;
use bevy::prelude::*;

use super::catalog::{
    default_local_source_roots, ContentCatalog, ContentCatalogEntry, ContentFamily,
    ContentSupportLevel,
};
use super::preview_decode::preview_capabilities_for_entry;

const HEADER_TEXT_SIZE: f32 = 18.0;
const PANEL_TEXT_SIZE: f32 = 16.0;
const PANEL_WIDTH: f32 = 300.0;
const HEADER_HEIGHT: f32 = 112.0;
const SOURCE_TOP: f32 = 144.0;
const SOURCE_HEIGHT: f32 = 596.0;
const ENTRY_BOTTOM: f32 = 16.0;
const ENTRY_HEIGHT: f32 = 0.0;
const SELECTION_WIDTH: f32 = 340.0;
const SELECTION_TOP: f32 = 340.0;
const SELECTION_HEIGHT: f32 = 364.0;
pub(crate) const GALLERY_COLUMNS: usize = 2;
pub(crate) const GALLERY_VISIBLE_ROWS: usize = 3;
pub(crate) const GALLERY_VISIBLE_SLOTS: usize = GALLERY_COLUMNS * GALLERY_VISIBLE_ROWS;
const PAGE_STEP: isize = GALLERY_VISIBLE_SLOTS as isize;
pub(crate) const ESCAPE_EXIT_CONFIRMATION_WINDOW_SECS: f64 = 1.0;

/// A deterministic window of gallery entries that currently fit on screen.
///
/// The gallery does not try to instantiate preview surfaces for the entire
/// catalog at once. Instead it exposes a stable slice around the current
/// selection so the Bevy UI can render one "page" of thumbnails while arrow
/// keys move through the full logical set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentGalleryWindow {
    pub(crate) catalog_index: usize,
    pub(crate) selected_window_index: usize,
    pub(crate) entry_indices: Vec<usize>,
}

/// Tracks the "press Esc twice to leave fullscreen" shortcut state.
///
/// The content lab starts in fullscreen mode, so we avoid binding a single
/// `Esc` press to immediate shutdown. This tiny state machine keeps the rule
/// explicit and testable: the first press arms the shortcut, and a second
/// press inside the confirmation window exits the app.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub(crate) struct EscapeExitShortcut {
    last_escape_press_secs: Option<f64>,
}

impl EscapeExitShortcut {
    /// Records an `Esc` press and returns `true` only when it confirms exit.
    pub(crate) fn register_press(&mut self, now_secs: f64) -> bool {
        let should_exit = self.last_escape_press_secs.is_some_and(|last_secs| {
            now_secs >= last_secs && now_secs - last_secs <= ESCAPE_EXIT_CONFIRMATION_WINDOW_SECS
        });

        self.last_escape_press_secs = if should_exit { None } else { Some(now_secs) };
        should_exit
    }
}

/// Mutable UI/navigation state for the content-lab overlay.
#[derive(Resource, Debug, Clone)]
pub struct ContentLabState {
    catalogs: Vec<ContentCatalog>,
    selected_catalog: usize,
    selected_entry: usize,
    dirty: bool,
    preview_summary: String,
    playback_summary: String,
}

impl ContentLabState {
    /// Builds content-lab state from the hard-coded local developer roots.
    pub fn from_default_local_sources() -> Self {
        let catalogs = default_local_source_roots()
            .into_iter()
            .map(ContentCatalog::scan)
            .collect();
        Self::new(catalogs)
    }

    /// Creates state from prebuilt catalogs.
    pub fn new(catalogs: Vec<ContentCatalog>) -> Self {
        let mut state = Self {
            catalogs,
            selected_catalog: 0,
            selected_entry: 0,
            dirty: true,
            preview_summary: "No preview has been loaded yet.".into(),
            playback_summary: "No active preview runtime.".into(),
        };
        state.select_first_gallery_entry();
        state
    }

    /// Immutable view of the scanned catalogs.
    pub(crate) fn catalogs(&self) -> &[ContentCatalog] {
        &self.catalogs
    }

    /// Zero-based currently selected catalog index.
    pub fn selected_catalog_index(&self) -> usize {
        self.selected_catalog
    }

    /// Zero-based currently selected entry index within the selected catalog.
    pub fn selected_entry_index(&self) -> usize {
        self.selected_entry
    }

    /// Returns the currently selected entry, if the indices are valid.
    pub(crate) fn selected_entry(&self) -> Option<&ContentCatalogEntry> {
        self.selected_catalog()
            .and_then(|catalog| catalog.entries.get(self.selected_entry))
    }

    /// Returns the current selection as `(catalog_index, entry_index)`.
    pub(crate) fn selected_location(&self) -> Option<(usize, usize)> {
        self.selected_entry()
            .map(|_| (self.selected_catalog, self.selected_entry))
    }

    /// Moves to the next or previous source root.
    pub fn cycle_catalog(&mut self, delta: isize) {
        if self.catalogs.is_empty() {
            return;
        }

        let len = self.catalogs.len() as isize;
        let next = (self.selected_catalog as isize + delta).rem_euclid(len) as usize;
        self.selected_catalog = next;
        self.selected_entry = self.first_gallery_entry_index(next).unwrap_or(0);
        self.dirty = true;
    }

    /// Moves to the next or previous entry within the selected catalog.
    pub fn move_selection(&mut self, delta: isize) {
        let Some(gallery_indices) = self.selected_gallery_entry_indices() else {
            self.selected_entry = 0;
            return;
        };
        if gallery_indices.is_empty() {
            self.selected_entry = 0;
            return;
        }

        let current_gallery_position = gallery_indices
            .iter()
            .position(|&index| index == self.selected_entry)
            .unwrap_or(0) as isize;
        let len = gallery_indices.len() as isize;
        let next_gallery_position = (current_gallery_position + delta).rem_euclid(len) as usize;
        self.selected_entry = gallery_indices[next_gallery_position];
        self.dirty = true;
    }

    /// Jumps up or down by one larger page-sized step in the entry list.
    pub fn page_selection(&mut self, delta: isize) {
        self.move_selection(delta.saturating_mul(PAGE_STEP));
    }

    /// Jumps to the first entry in the selected catalog.
    pub fn move_selection_to_start(&mut self) {
        if let Some(first_index) = self.first_gallery_entry_index(self.selected_catalog) {
            self.selected_entry = first_index;
            self.dirty = true;
        }
    }

    /// Jumps to the last entry in the selected catalog.
    pub fn move_selection_to_end(&mut self) {
        if let Some(last_index) = self
            .selected_gallery_entry_indices()
            .and_then(|indices| indices.last().copied())
        {
            self.selected_entry = last_index;
            self.dirty = true;
        }
    }

    /// Returns the currently visible window of gallery entries.
    ///
    /// The gallery only renders entries that have a direct visual surface:
    /// sprites, palettes, waveforms, first video frames, and similar resource
    /// types. Pure text resources stay accessible through the diagnostics
    /// panels, but they do not consume one of the finite thumbnail slots.
    pub(crate) fn gallery_window(&self) -> Option<ContentGalleryWindow> {
        let gallery_indices = self.selected_gallery_entry_indices()?;
        if gallery_indices.is_empty() {
            return None;
        }

        let selected_gallery_position = gallery_indices
            .iter()
            .position(|&index| index == self.selected_entry)
            .unwrap_or(0);
        let total_rows = gallery_indices.len().div_ceil(GALLERY_COLUMNS);
        let selected_row = selected_gallery_position / GALLERY_COLUMNS;
        let max_start_row = total_rows.saturating_sub(GALLERY_VISIBLE_ROWS);
        let start_row = selected_row
            .saturating_sub(GALLERY_VISIBLE_ROWS / 2)
            .min(max_start_row);
        let start_index = start_row * GALLERY_COLUMNS;
        let end_index = (start_index + GALLERY_VISIBLE_SLOTS).min(gallery_indices.len());

        Some(ContentGalleryWindow {
            catalog_index: self.selected_catalog,
            selected_window_index: selected_gallery_position.saturating_sub(start_index),
            entry_indices: gallery_indices[start_index..end_index].to_vec(),
        })
    }

    /// Updates the preview-summary section of the right-hand panel.
    pub(crate) fn set_preview_summary(&mut self, summary: impl Into<String>) {
        let summary = summary.into();
        if self.preview_summary != summary {
            self.preview_summary = summary;
            self.dirty = true;
        }
    }

    /// Updates the runtime-status section of the right-hand panel.
    pub(crate) fn set_playback_summary(&mut self, summary: impl Into<String>) {
        let summary = summary.into();
        if self.playback_summary != summary {
            self.playback_summary = summary;
            self.dirty = true;
        }
    }

    /// Combined text dump kept for tests and quick debugging.
    pub fn render_text(&self) -> String {
        [
            self.render_header_text(),
            self.render_source_text(),
            self.render_entry_list_text(),
            self.render_selection_text(),
        ]
        .join("\n\n")
    }

    fn render_header_text(&self) -> String {
        [
            "Iron Curtain Content Lab".to_string(),
            "Arrow keys: browse gallery   PageUp/PageDown: jump   Home/End: edges".to_string(),
            "Q / E: switch source roots".to_string(),
            "Space: play/pause   Enter: restart   , / . : animation frame step".to_string(),
            "Focused preview/player: right side, aspect-preserving".to_string(),
            "Esc twice: exit fullscreen content lab".to_string(),
            "Goal: render one scrollable wall of real RA / Remastered resources and validate playback."
                .to_string(),
        ]
        .join("\n")
    }

    fn render_source_text(&self) -> String {
        if self.catalogs.is_empty() {
            return "No content sources are configured.".into();
        }

        let catalog = &self.catalogs[self.selected_catalog.min(self.catalogs.len() - 1)];
        let previewable_count = catalog
            .entries
            .iter()
            .filter(|entry| is_previewable_entry(entry))
            .count();

        let mut lines = vec![
            format!(
                "Source {}/{}: {}",
                self.selected_catalog + 1,
                self.catalogs.len(),
                catalog.source.display_name
            ),
            format!("Path: {}", catalog.source.path.display()),
            format!(
                "Status: {}",
                if catalog.available {
                    "available"
                } else {
                    "missing"
                }
            ),
            format!(
                "Entries: {} | previewable now: {} | size: {}",
                catalog.entries.len(),
                previewable_count,
                human_bytes(catalog.total_bytes)
            ),
            format!(
                "Support counts: now {} | planned {} | external-only {}",
                catalog.entry_count_for_support(ContentSupportLevel::SupportedNow),
                catalog.entry_count_for_support(ContentSupportLevel::Planned),
                catalog.entry_count_for_support(ContentSupportLevel::ExternalOnly),
            ),
        ];

        if !catalog.notes.is_empty() {
            lines.push(format!("Notes: {}", catalog.notes.join(" | ")));
        }

        lines.push(String::new());
        lines.push("Family counts:".into());
        for family in [
            ContentFamily::WestwoodArchive,
            ContentFamily::RemasteredArchive,
            ContentFamily::SpriteSheet,
            ContentFamily::Palette,
            ContentFamily::Audio,
            ContentFamily::Video,
            ContentFamily::Config,
            ContentFamily::Image,
            ContentFamily::Document,
            ContentFamily::ExternalArchive,
            ContentFamily::Executable,
            ContentFamily::Other,
        ] {
            let count = catalog.entry_count_for_family(family);
            if count > 0 {
                lines.push(format!("  - {}: {}", family, count));
            }
        }

        lines.join("\n")
    }

    fn render_entry_list_text(&self) -> String {
        let mut lines = vec![
            "Visible Gallery Window:".into(),
            "badges: V=visual, A=audio, T=text".into(),
        ];

        let Some(gallery_window) = self.gallery_window() else {
            lines.push("  - this source has no thumbnailable resources yet".into());
            return lines.join("\n");
        };

        let catalog = &self.catalogs[gallery_window.catalog_index];
        for (window_index, entry_index) in gallery_window.entry_indices.iter().enumerate() {
            let entry = &catalog.entries[*entry_index];
            let marker = if window_index == gallery_window.selected_window_index {
                ">"
            } else {
                " "
            };
            let capabilities = preview_capabilities_for_entry(entry);
            lines.push(format!(
                "{marker} [{}] {} [{} / {} / {}]",
                capabilities.badge_string(),
                entry.relative_path,
                entry.family,
                entry.support,
                human_bytes(entry.size_bytes)
            ));
        }

        lines.join("\n")
    }

    fn render_selection_text(&self) -> String {
        let mut lines = vec!["Selected Entry:".to_string()];

        if let Some(selected) = self.selected_entry() {
            let capabilities = preview_capabilities_for_entry(selected);
            lines.push(format!("Logical path: {}", selected.relative_path));
            lines.push(format!("Origin: {}", selected.describe_origin()));
            lines.push(format!("Family: {}", selected.family));
            lines.push(format!("Support: {}", selected.support));
            lines.push(format!("Size: {}", human_bytes(selected.size_bytes)));
            lines.push(format!(
                "Validation surfaces: {} ({})",
                capabilities.surface_summary(),
                capabilities.badge_string()
            ));
        } else {
            lines.push("No entry is currently selected.".into());
        }

        lines.push(String::new());
        lines.push("Preview:".into());
        for line in self.preview_summary.lines() {
            lines.push(format!("  {line}"));
        }

        lines.push(String::new());
        lines.push("Runtime status:".into());
        for line in self.playback_summary.lines() {
            lines.push(format!("  {line}"));
        }

        lines.join("\n")
    }

    fn select_first_gallery_entry(&mut self) {
        for (catalog_index, catalog) in self.catalogs.iter().enumerate() {
            if let Some(entry_index) = catalog.entries.iter().position(is_gallery_entry) {
                self.selected_catalog = catalog_index;
                self.selected_entry = entry_index;
                return;
            }
        }
    }

    fn first_gallery_entry_index(&self, catalog_index: usize) -> Option<usize> {
        self.catalogs
            .get(catalog_index)?
            .entries
            .iter()
            .position(is_gallery_entry)
    }

    fn selected_catalog(&self) -> Option<&ContentCatalog> {
        self.catalogs.get(self.selected_catalog)
    }

    fn selected_gallery_entry_indices(&self) -> Option<Vec<usize>> {
        Some(
            self.selected_catalog()?
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| is_gallery_entry(entry).then_some(index))
                .collect(),
        )
    }
}

#[derive(Component)]
pub(crate) struct ContentWindowHeaderText;

#[derive(Component)]
pub(crate) struct ContentWindowSourceText;

#[derive(Component)]
pub(crate) struct ContentWindowEntryListText;

#[derive(Component)]
pub(crate) struct ContentWindowSelectionText;

/// Builds the split content-lab overlay panels.
///
/// Bevy's UI system is also ECS-driven: each panel is a normal entity with a
/// `Text` component and a `Node` layout component. Keeping the panels separate
/// matters here because this tool needs distinct "what source is mounted?",
/// "what entry is selected?", and "is the preview actually playing?" surfaces.
pub(crate) fn setup_content_window_ui(mut commands: Commands, state: Res<ContentLabState>) {
    let panel_bg = Color::srgba(0.06, 0.08, 0.10, 0.84);
    let panel_text = Color::srgb(0.90, 0.91, 0.92);

    commands.spawn((
        ContentWindowHeaderText,
        Text::new(state.render_header_text()),
        TextFont {
            font_size: HEADER_TEXT_SIZE,
            ..default()
        },
        TextColor(panel_text),
        BackgroundColor(panel_bg),
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            width: px(PANEL_WIDTH),
            height: px(HEADER_HEIGHT),
            padding: UiRect::all(px(12)),
            ..default()
        },
    ));

    commands.spawn((
        ContentWindowSourceText,
        Text::new(state.render_source_text()),
        TextFont {
            font_size: PANEL_TEXT_SIZE,
            ..default()
        },
        TextColor(panel_text),
        BackgroundColor(panel_bg),
        Node {
            position_type: PositionType::Absolute,
            top: px(SOURCE_TOP),
            left: px(16),
            width: px(PANEL_WIDTH),
            height: px(SOURCE_HEIGHT),
            padding: UiRect::all(px(12)),
            ..default()
        },
    ));

    commands.spawn((
        ContentWindowEntryListText,
        Text::new(state.render_entry_list_text()),
        TextFont {
            font_size: PANEL_TEXT_SIZE,
            ..default()
        },
        TextColor(panel_text),
        BackgroundColor(panel_bg),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(ENTRY_BOTTOM),
            left: px(16),
            width: px(PANEL_WIDTH),
            height: px(ENTRY_HEIGHT),
            padding: UiRect::all(px(12)),
            display: Display::None,
            ..default()
        },
    ));

    commands.spawn((
        ContentWindowSelectionText,
        Text::new(state.render_selection_text()),
        TextFont {
            font_size: PANEL_TEXT_SIZE,
            ..default()
        },
        TextColor(panel_text),
        BackgroundColor(panel_bg),
        Node {
            position_type: PositionType::Absolute,
            top: px(SELECTION_TOP),
            right: px(16),
            width: px(SELECTION_WIDTH),
            height: px(SELECTION_HEIGHT),
            padding: UiRect::all(px(12)),
            ..default()
        },
    ));
}

/// Keyboard navigation for the content browser.
///
/// `ButtonInput<KeyCode>` is the Bevy resource that reports keyboard
/// transitions for the current frame. This system only updates navigation
/// state; preview decode and panel refresh happen in later systems.
pub(crate) fn handle_content_window_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ContentLabState>,
) {
    if keyboard.just_pressed(KeyCode::KeyQ) {
        state.cycle_catalog(-1);
    }
    if keyboard.just_pressed(KeyCode::KeyE) {
        state.cycle_catalog(1);
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        state.move_selection(-1);
    }
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        state.move_selection(1);
    }
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        state.move_selection(-(GALLERY_COLUMNS as isize));
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        state.move_selection(GALLERY_COLUMNS as isize);
    }
    if keyboard.just_pressed(KeyCode::PageUp) {
        state.page_selection(-1);
    }
    if keyboard.just_pressed(KeyCode::PageDown) {
        state.page_selection(1);
    }
    if keyboard.just_pressed(KeyCode::Home) {
        state.move_selection_to_start();
    }
    if keyboard.just_pressed(KeyCode::End) {
        state.move_selection_to_end();
    }
}

/// Exits the fullscreen content lab only after a deliberate double `Esc`.
///
/// In fullscreen tools a single `Esc` is easy to hit accidentally while
/// browsing. This system keeps the exit gesture separate from the rest of the
/// navigation input and writes `AppExit::Success` only when the second press
/// lands inside the confirmation window.
pub(crate) fn handle_content_window_exit_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut shortcut: ResMut<EscapeExitShortcut>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }

    if shortcut.register_press(time.elapsed_secs_f64()) {
        app_exit.write(AppExit::Success);
    }
}

/// Regenerates the visible UI text after navigation or preview-status changes.
///
/// The `dirty` flag keeps the lab from rewriting every text panel each frame
/// when nothing changed.
///
/// Bevy treats multiple `Query<&mut Text, ...>` parameters in one system as a
/// potential aliasing hazard even when each query uses a different marker
/// component. `ParamSet` is the engine's escape hatch for this pattern: it
/// tells Bevy the system will touch the queries one at a time instead of
/// holding overlapping mutable borrows to `Text` for the whole system call.
#[allow(
    clippy::type_complexity,
    reason = "Bevy system signatures with one ParamSet over several UI text queries are verbose by nature; spelling the queries inline keeps the function in the standard system form that Bevy schedules cleanly."
)]
pub(crate) fn refresh_content_window_text(
    mut state: ResMut<ContentLabState>,
    mut text_queries: ParamSet<(
        Query<&mut Text, With<ContentWindowHeaderText>>,
        Query<&mut Text, With<ContentWindowSourceText>>,
        Query<&mut Text, With<ContentWindowEntryListText>>,
        Query<&mut Text, With<ContentWindowSelectionText>>,
    )>,
) {
    if !state.dirty {
        return;
    }

    let header = state.render_header_text();
    let source = state.render_source_text();
    let entries = state.render_entry_list_text();
    let selection = state.render_selection_text();

    {
        let mut header_query = text_queries.p0();
        for mut text in &mut header_query {
            text.0 = header.clone();
        }
    }
    {
        let mut source_query = text_queries.p1();
        for mut text in &mut source_query {
            text.0 = source.clone();
        }
    }
    {
        let mut entry_query = text_queries.p2();
        for mut text in &mut entry_query {
            text.0 = entries.clone();
        }
    }
    {
        let mut selection_query = text_queries.p3();
        for mut text in &mut selection_query {
            text.0 = selection.clone();
        }
    }
    state.dirty = false;
}

pub(crate) fn is_previewable_entry(entry: &ContentCatalogEntry) -> bool {
    preview_capabilities_for_entry(entry).any()
}

pub(crate) fn is_gallery_entry(entry: &ContentCatalogEntry) -> bool {
    preview_capabilities_for_entry(entry).visual()
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}
