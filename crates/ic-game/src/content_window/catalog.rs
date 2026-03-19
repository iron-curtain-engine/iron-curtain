// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Filesystem cataloging for the content-lab window.
//!
//! This module is intentionally Bevy-free. It turns a few configured local
//! roots into deterministic file inventories that the UI and preview systems
//! can consume later.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use ic_cnc_content::meg::MegArchive;
use ic_cnc_content::mix::MixArchive;
use ic_cnc_content::source::{ContentSourceKind, SourceRightsClass};

/// One configured content source root shown in the lab.
///
/// A root can be a directory tree or a single archive/container file. The
/// first pass uses explicit local presets because that keeps the implementation
/// honest and easy to inspect before the later setup/import wizard exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSourceRoot {
    pub display_name: String,
    pub path: PathBuf,
    pub source_kind: ContentSourceKind,
    pub rights_class: SourceRightsClass,
    pub root_shape: ContentRootShape,
}

impl ContentSourceRoot {
    /// Creates one directory-backed content source.
    pub fn directory(
        display_name: impl Into<String>,
        path: PathBuf,
        source_kind: ContentSourceKind,
        rights_class: SourceRightsClass,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            path,
            source_kind,
            rights_class,
            root_shape: ContentRootShape::DirectoryTree,
        }
    }

    /// Creates one single-file source such as an external archive.
    pub fn single_file(
        display_name: impl Into<String>,
        path: PathBuf,
        source_kind: ContentSourceKind,
        rights_class: SourceRightsClass,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            path,
            source_kind,
            rights_class,
            root_shape: ContentRootShape::SingleFile,
        }
    }
}

/// Physical shape of one source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRootShape {
    DirectoryTree,
    SingleFile,
}

/// High-level family shown in the content browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentFamily {
    WestwoodArchive,
    RemasteredArchive,
    Palette,
    SpriteSheet,
    Audio,
    Video,
    Config,
    Image,
    Executable,
    Document,
    ExternalArchive,
    Other,
}

impl Display for ContentFamily {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::WestwoodArchive => "westwood archive",
            Self::RemasteredArchive => "remastered archive",
            Self::Palette => "palette",
            Self::SpriteSheet => "sprite sheet",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Config => "config",
            Self::Image => "image",
            Self::Executable => "executable",
            Self::Document => "document",
            Self::ExternalArchive => "external archive",
            Self::Other => "other",
        };
        write!(f, "{label}")
    }
}

/// Current viewer/proof status for one file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContentSupportLevel {
    SupportedNow,
    Planned,
    ExternalOnly,
}

impl Display for ContentSupportLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::SupportedNow => "supported now",
            Self::Planned => "planned",
            Self::ExternalOnly => "external only",
        };
        write!(f, "{label}")
    }
}

/// One file discovered under a content source root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentCatalogEntry {
    pub relative_path: String,
    pub location: ContentEntryLocation,
    pub size_bytes: u64,
    pub family: ContentFamily,
    pub support: ContentSupportLevel,
}

impl ContentCatalogEntry {
    /// Returns a human-readable description of where this entry's bytes live.
    ///
    /// Loose files and archive members share the same catalog entry shape, so
    /// the UI needs one common formatting hook instead of inspecting enum
    /// variants directly.
    pub fn describe_origin(&self) -> String {
        self.location.describe_origin()
    }
}

/// Physical storage location for one content entry.
///
/// The content lab uses this enum so preview code can load bytes through one
/// path whether the selected resource lives as a loose file or inside an
/// archive member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentEntryLocation {
    /// Resource bytes live directly on disk as one standalone file.
    Filesystem { absolute_path: PathBuf },
    /// Resource bytes live inside a classic Westwood `.mix` archive.
    MixMember {
        archive_path: PathBuf,
        archive_index: usize,
        crc_raw: u32,
        logical_name: Option<String>,
    },
    /// Resource bytes live inside a Petroglyph `.meg` archive.
    MegMember {
        archive_path: PathBuf,
        archive_index: usize,
        logical_name: String,
    },
}

impl ContentEntryLocation {
    /// Creates the location value used for loose filesystem files.
    pub fn filesystem(absolute_path: PathBuf) -> Self {
        Self::Filesystem { absolute_path }
    }

    /// Returns the physical path that stores the entry bytes.
    ///
    /// For loose files this is the file itself. For archive members this is the
    /// outer container path that must be reopened before the payload can be
    /// extracted.
    pub fn source_path(&self) -> &Path {
        match self {
            Self::Filesystem { absolute_path } => absolute_path,
            Self::MixMember { archive_path, .. } | Self::MegMember { archive_path, .. } => {
                archive_path
            }
        }
    }

    /// Returns the logical filename for archive members when that name is
    /// known.
    pub fn logical_name(&self) -> Option<&str> {
        match self {
            Self::Filesystem { .. } => None,
            Self::MixMember { logical_name, .. } => logical_name.as_deref(),
            Self::MegMember { logical_name, .. } => Some(logical_name),
        }
    }

    /// Returns the uppercase filename component used for extension matching
    /// and palette selection heuristics.
    pub fn file_name_upper(&self) -> String {
        if let Some(logical_name) = self.logical_name() {
            return file_name_upper_from_text(logical_name);
        }

        self.source_path()
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_else(|| {
                self.source_path()
                    .display()
                    .to_string()
                    .to_ascii_uppercase()
            })
    }

    /// Returns `true` when two entries live in the same archive container.
    ///
    /// This is used by the early palette resolver so a sprite inside one
    /// archive prefers a palette from the same archive before falling back to
    /// a global cross-source search.
    pub fn shares_archive_container_with(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::MixMember {
                    archive_path: left, ..
                },
                Self::MixMember {
                    archive_path: right,
                    ..
                },
            )
            | (
                Self::MegMember {
                    archive_path: left, ..
                },
                Self::MegMember {
                    archive_path: right,
                    ..
                },
            ) => left == right,
            _ => false,
        }
    }

    /// Formats a source/origin string for the content-lab text overlay.
    pub fn describe_origin(&self) -> String {
        match self {
            Self::Filesystem { absolute_path } => absolute_path.display().to_string(),
            Self::MixMember {
                archive_path,
                archive_index,
                crc_raw,
                logical_name,
            } => {
                let logical_name = logical_name
                    .as_deref()
                    .unwrap_or("<unresolved MIX logical name>");
                format!(
                    "{} [MIX member #{archive_index}, crc {crc_raw:08X}, name {logical_name}]",
                    archive_path.display()
                )
            }
            Self::MegMember {
                archive_path,
                archive_index,
                logical_name,
            } => format!(
                "{} [MEG member #{archive_index}, name {logical_name}]",
                archive_path.display()
            ),
        }
    }

    fn sort_key(&self) -> String {
        match self {
            Self::Filesystem { absolute_path } => format!("file:{}", absolute_path.display()),
            Self::MixMember {
                archive_path,
                archive_index,
                crc_raw,
                logical_name,
            } => format!(
                "mix:{}:{archive_index}:{crc_raw:08X}:{}",
                archive_path.display(),
                logical_name.as_deref().unwrap_or("")
            ),
            Self::MegMember {
                archive_path,
                archive_index,
                logical_name,
            } => format!(
                "meg:{}:{archive_index}:{logical_name}",
                archive_path.display()
            ),
        }
    }
}

/// Deterministic file catalog for one content source root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentCatalog {
    pub source: ContentSourceRoot,
    pub available: bool,
    pub entries: Vec<ContentCatalogEntry>,
    pub notes: Vec<String>,
    pub total_bytes: u64,
    family_counts: BTreeMap<ContentFamily, usize>,
    support_counts: BTreeMap<ContentSupportLevel, usize>,
}

impl ContentCatalog {
    /// Scans the source root into a deterministic catalog.
    ///
    /// The first pass only inspects filesystem metadata. Parsing file contents
    /// belongs to the preview/player passes that sit on top of this catalog.
    pub fn scan(source: ContentSourceRoot) -> Self {
        let path = source.path.clone();
        let mut catalog = Self {
            source,
            available: false,
            entries: Vec::new(),
            notes: Vec::new(),
            total_bytes: 0,
            family_counts: BTreeMap::new(),
            support_counts: BTreeMap::new(),
        };

        match fs::metadata(&path) {
            Ok(metadata) => {
                catalog.available = true;
                if metadata.is_file() || catalog.source.root_shape == ContentRootShape::SingleFile {
                    let relative_path = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    catalog.push_entry(
                        ContentCatalogEntry {
                            relative_path: relative_path.clone(),
                            location: ContentEntryLocation::filesystem(path.clone()),
                            size_bytes: metadata.len(),
                            family: classify_family(&path),
                            support: classify_support(&path),
                        },
                        true,
                    );
                    catalog.mount_archive_members(&path, &relative_path);
                } else if metadata.is_dir() {
                    catalog.scan_directory_tree(&path);
                } else {
                    catalog
                        .notes
                        .push("source exists but is neither a regular file nor directory".into());
                }
            }
            Err(error) => {
                catalog
                    .notes
                    .push(format!("source path is unavailable: {error}"));
            }
        }

        catalog.entries.sort_by(|left, right| {
            left.relative_path
                .cmp(&right.relative_path)
                .then_with(|| left.location.sort_key().cmp(&right.location.sort_key()))
        });
        catalog
    }

    /// Number of entries currently cataloged for the requested family.
    pub fn entry_count_for_family(&self, family: ContentFamily) -> usize {
        self.family_counts.get(&family).copied().unwrap_or(0)
    }

    /// Number of entries currently cataloged at the requested support level.
    pub fn entry_count_for_support(&self, support: ContentSupportLevel) -> usize {
        self.support_counts.get(&support).copied().unwrap_or(0)
    }

    fn scan_directory_tree(&mut self, root: &Path) {
        let mut stack = vec![root.to_path_buf()];

        while let Some(directory) = stack.pop() {
            let read_dir = match fs::read_dir(&directory) {
                Ok(read_dir) => read_dir,
                Err(error) => {
                    self.notes.push(format!(
                        "could not read directory {}: {error}",
                        directory.display()
                    ));
                    continue;
                }
            };

            for child in read_dir {
                let child = match child {
                    Ok(child) => child,
                    Err(error) => {
                        self.notes
                            .push(format!("directory entry read failed: {error}"));
                        continue;
                    }
                };

                let path = child.path();
                let file_type = match child.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        self.notes.push(format!(
                            "could not read file type for {}: {error}",
                            path.display()
                        ));
                        continue;
                    }
                };

                if file_type.is_dir() {
                    stack.push(path);
                    continue;
                }

                if !file_type.is_file() {
                    continue;
                }

                let metadata = match child.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        self.notes.push(format!(
                            "could not read metadata for {}: {error}",
                            path.display()
                        ));
                        continue;
                    }
                };

                let relative_path = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                self.push_entry(
                    ContentCatalogEntry {
                        relative_path: relative_path.clone(),
                        location: ContentEntryLocation::filesystem(path.clone()),
                        size_bytes: metadata.len(),
                        family: classify_family(&path),
                        support: classify_support(&path),
                    },
                    true,
                );
                self.mount_archive_members(&path, &relative_path);
            }
        }
    }

    fn mount_archive_members(&mut self, archive_path: &Path, archive_relative_path: &str) {
        match normalized_extension(archive_path).as_deref() {
            Some("mix") => self.mount_mix_members(archive_path, archive_relative_path),
            Some("meg") | Some("pgm") => {
                self.mount_meg_members(archive_path, archive_relative_path)
            }
            _ => {}
        }
    }

    fn mount_mix_members(&mut self, archive_path: &Path, archive_relative_path: &str) {
        let bytes = match fs::read(archive_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.notes.push(format!(
                    "could not read MIX archive {}: {error}",
                    archive_path.display()
                ));
                return;
            }
        };
        let archive = match MixArchive::parse(bytes) {
            Ok(archive) => archive,
            Err(error) => {
                self.notes.push(format!(
                    "could not parse MIX archive {}: {error}",
                    archive_path.display()
                ));
                return;
            }
        };
        let builtin_names = ic_cnc_content::cnc_formats::mix::builtin_name_map();
        let staged_entries = match archive.staged_entries() {
            Ok(entries) => entries,
            Err(error) => {
                self.notes.push(format!(
                    "could not enumerate MIX archive {}: {error}",
                    archive_path.display()
                ));
                return;
            }
        };

        for staged in staged_entries {
            let logical_name = staged
                .logical_name
                .or_else(|| builtin_names.get(&staged.crc).cloned())
                .map(|name| normalize_member_path(&name));
            let logical_display_name = logical_name
                .clone()
                .unwrap_or_else(|| format!("CRC_{:08X}.BIN", staged.crc.to_raw()));
            let logical_path = Path::new(&logical_display_name);
            self.push_entry(
                ContentCatalogEntry {
                    relative_path: format!("{archive_relative_path}::{logical_display_name}"),
                    location: ContentEntryLocation::MixMember {
                        archive_path: archive_path.to_path_buf(),
                        archive_index: staged.archive_index,
                        crc_raw: staged.crc.to_raw(),
                        logical_name,
                    },
                    size_bytes: staged.size as u64,
                    family: classify_family(logical_path),
                    support: classify_support(logical_path),
                },
                false,
            );
        }
    }

    fn mount_meg_members(&mut self, archive_path: &Path, archive_relative_path: &str) {
        let bytes = match fs::read(archive_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.notes.push(format!(
                    "could not read MEG archive {}: {error}",
                    archive_path.display()
                ));
                return;
            }
        };
        let archive = match MegArchive::parse(bytes) {
            Ok(archive) => archive,
            Err(error) => {
                self.notes.push(format!(
                    "could not parse MEG archive {}: {error}",
                    archive_path.display()
                ));
                return;
            }
        };
        let staged_entries = match archive.staged_entries() {
            Ok(entries) => entries,
            Err(error) => {
                self.notes.push(format!(
                    "could not enumerate MEG archive {}: {error}",
                    archive_path.display()
                ));
                return;
            }
        };

        for staged in staged_entries {
            let logical_name = normalize_member_path(&staged.logical_name);
            let logical_path = Path::new(&logical_name);
            let family = classify_family(logical_path);
            let support = classify_support(logical_path);
            self.push_entry(
                ContentCatalogEntry {
                    relative_path: format!("{archive_relative_path}::{logical_name}"),
                    location: ContentEntryLocation::MegMember {
                        archive_path: archive_path.to_path_buf(),
                        archive_index: staged.archive_index,
                        logical_name,
                    },
                    size_bytes: staged.size,
                    family,
                    support,
                },
                false,
            );
        }
    }

    fn push_entry(&mut self, entry: ContentCatalogEntry, count_toward_total_bytes: bool) {
        if count_toward_total_bytes {
            self.total_bytes = self.total_bytes.saturating_add(entry.size_bytes);
        }
        *self.family_counts.entry(entry.family).or_insert(0) += 1;
        *self.support_counts.entry(entry.support).or_insert(0) += 1;
        self.entries.push(entry);
    }
}

/// Returns the current hard-coded local roots used by the first content lab.
///
/// The extracted palette sample is included only when it actually exists. That
/// keeps the UI from showing a noisy missing root on machines that only have
/// the base sample disc and not the extra extracted helper directory.
pub fn default_local_source_roots() -> Vec<ContentSourceRoot> {
    let mut roots = vec![
        ContentSourceRoot::directory(
            "RA1 Allied Disc Sample",
            PathBuf::from("/mnt/c/git/games/cnc-formats/samples/CD1_ALLIED_DISC"),
            ContentSourceKind::ManualDirectory,
            SourceRightsClass::OwnedProprietary,
        ),
        ContentSourceRoot::single_file(
            "RA1 Allied Disc RAR",
            PathBuf::from("/mnt/c/Users/DK/Downloads/RedAlert1_AlliedDisc.rar"),
            ContentSourceKind::ManualDirectory,
            SourceRightsClass::OwnedProprietary,
        ),
    ];

    let sample_palette_root =
        PathBuf::from("/mnt/c/git/games/cnc-formats/samples/CD1_ALLIED_DISC/extract2/LOCAL_OUTPUT");
    if sample_palette_root.is_dir() {
        roots.push(ContentSourceRoot::directory(
            "RA1 Sample Palettes",
            sample_palette_root,
            ContentSourceKind::ManualDirectory,
            SourceRightsClass::OwnedProprietary,
        ));
    }

    roots.push(ContentSourceRoot::directory(
        "C&C Remastered Collection",
        PathBuf::from("/mnt/c/Program Files (x86)/Steam/steamapps/common/CnCRemastered"),
        ContentSourceKind::Steam,
        SourceRightsClass::OwnedProprietary,
    ));

    roots
}

fn classify_family(path: &Path) -> ContentFamily {
    match normalized_extension(path).as_deref() {
        Some("mix") => ContentFamily::WestwoodArchive,
        Some("meg") | Some("pgm") => ContentFamily::RemasteredArchive,
        Some("pal") => ContentFamily::Palette,
        Some("shp") => ContentFamily::SpriteSheet,
        Some("aud") | Some("wav") => ContentFamily::Audio,
        Some("vqa") | Some("vqp") | Some("wsa") | Some("bk2") => ContentFamily::Video,
        Some("ini") | Some("yaml") | Some("miniyaml") | Some("xml") | Some("eng") | Some("fre")
        | Some("ger") | Some("csf") => ContentFamily::Config,
        Some("tga") | Some("dds") | Some("bmp") | Some("png") | Some("tmp") | Some("lut")
        | Some("fnt") => ContentFamily::Image,
        Some("exe") | Some("dll") => ContentFamily::Executable,
        Some("pdf") | Some("txt") | Some("hlp") => ContentFamily::Document,
        Some("rar") | Some("zip") | Some("7z") => ContentFamily::ExternalArchive,
        _ => ContentFamily::Other,
    }
}

fn classify_support(path: &Path) -> ContentSupportLevel {
    match normalized_extension(path).as_deref() {
        Some("mix") | Some("meg") | Some("pgm") | Some("pal") | Some("shp") | Some("aud")
        | Some("wav") | Some("wsa") | Some("vqa") | Some("vqp") | Some("ini") | Some("yaml")
        | Some("xml") | Some("miniyaml") | Some("txt") | Some("eng") | Some("fre")
        | Some("ger") | Some("lut") | Some("fnt") | Some("tmp") => {
            ContentSupportLevel::SupportedNow
        }
        Some("rar") | Some("zip") | Some("7z") => ContentSupportLevel::ExternalOnly,
        _ => ContentSupportLevel::Planned,
    }
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

fn normalize_member_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn file_name_upper_from_text(path: &str) -> String {
    let normalized = normalize_member_path(path);
    Path::new(&normalized)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_else(|| normalized.to_ascii_uppercase())
}
