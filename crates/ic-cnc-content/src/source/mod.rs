// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Source-adapter probe contract and source-manifest snapshots for owned installs.
//!
//! `G1.1` is not the full importer yet. Its job is to define the normalized
//! handoff between:
//! - a layout-specific source adapter that knows how Steam, GOG, EA App, or a
//!   manual directory is arranged on disk
//! - the later shared importer that will copy, extract, verify, and index the
//!   data without mutating the original install
//!
//! In practical terms, a future source adapter probes one install location and
//! emits a [`SourceManifestSnapshot`]. That snapshot contains:
//! - the D069-style candidate the setup wizard can show to the player
//! - the detected artifacts and their sizes
//! - probe/integrity status for each artifact
//! - provenance tags that explain where the data came from
//!
//! This module is intentionally Bevy-free. It defines importer-facing data
//! contracts that `G1.2` and later D069/D068 flows will build on.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema version for serialized source-manifest snapshots.
///
/// Keeping an explicit version lets later importer tooling evolve the snapshot
/// shape without making old probe fixtures ambiguous.
pub const SOURCE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Distribution source that owns or provides the detected content.
///
/// This is the normalized "where did we find it?" label shown in D069 setup
/// flows. Per-source adapter code can carry richer platform details internally
/// (registry keys, app IDs, library roots), but the shared importer only needs
/// this stable classification plus the resolved path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentSourceKind {
    /// Steam library install.
    Steam,
    /// GOG install detected through Galaxy or the standard filesystem layout.
    Gog,
    /// EA App / Origin-family install.
    EaApp,
    /// OpenRA-managed content directory.
    OpenRa,
    /// User-supplied folder, disc copy, or other manually selected directory.
    ManualDirectory,
}

/// Result of probing one content source or one artifact inside it.
///
/// The same status vocabulary is used at the source level and at the per-file
/// level so repair tooling can tell whether the problem is:
/// - "nothing was found"
/// - "something was found, but with warnings"
/// - "the path exists but the data is unreadable or incompatible"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeStatus {
    /// The expected install or artifact was found and looks usable.
    Detected,
    /// The install is usable, but one or more optional or suspect items need follow-up.
    DetectedWithWarnings,
    /// The expected install or artifact was not found at this path.
    Missing,
    /// The path was readable but does not match the required game/layout.
    Incompatible,
    /// The path exists but could not be read or inspected safely.
    Unreadable,
}

/// Confidence level attached to one artifact probe result.
///
/// D069 distinguishes "basic compatibility/probe confidence" from the deeper
/// verification and indexing work that happens later during import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeConfidence {
    /// The adapter matched the artifact using strong identifiers such as a
    /// filename/layout contract plus successful header parsing.
    High,
    /// The adapter matched the artifact using a mostly reliable layout rule but
    /// without deep parsing yet.
    Medium,
    /// The adapter matched the artifact heuristically and expects later verify
    /// stages to confirm or reject it.
    Low,
}

/// How D069 intends to bring the content into IC-managed storage.
///
/// `ReferenceOnly` exists in the schema now so the source-manifest output can
/// already express the future advanced path, even though player-facing
/// reference-only workflows are deferred until `M8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentSourceImportMode {
    /// Deep-copy the source files into IC-managed storage for resilience.
    Copy,
    /// Extract playable data from archives into IC-managed storage.
    Extract,
    /// Keep references to the source install without making a portable copy.
    ReferenceOnly,
}

/// Legal / provenance class for one source.
///
/// The import pipeline preserves this information so later D068/D049 tooling
/// can distinguish owned proprietary content from open content or local custom
/// files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceRightsClass {
    /// Proprietary content the player owns locally.
    OwnedProprietary,
    /// Open or freely redistributable content such as OpenRA assets.
    OpenContent,
    /// User-authored local content with no external rights claim implied.
    LocalCustom,
}

/// High-level asset bundle the probe found at the source path.
///
/// This is intentionally a small M1/M3-focused set. The goal is to tell the
/// setup wizard what kind of playable content the source appears to provide,
/// not to encode every individual file in the install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetectedAssetSetKind {
    /// Classic Red Alert-era content built around Westwood archives and
    /// palette-indexed assets.
    RedAlertClassic,
    /// The C&C Remastered Collection HD source family.
    RemasteredCollection,
}

/// Artifact family recorded inside the source manifest snapshot.
///
/// The format importer will later use these kinds to route artifacts into the
/// correct parser/validator path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceArtifactKind {
    /// Westwood `.mix` archive.
    MixArchive,
    /// Westwood `.shp` sprite sheet.
    ShpSpriteSheet,
    /// Westwood `.pal` palette.
    Palette,
    /// Westwood `.aud` audio clip.
    AudAudio,
    /// Westwood `.vqa` video file.
    VqaVideo,
    /// Classic `.ini` rules or scenario file.
    RulesIni,
    /// OpenRA compatibility content in MiniYAML.
    MiniYaml,
    /// Petroglyph `.meg` archive used by Remastered.
    MegArchive,
    /// Remastered texture atlas stored as `.tga`.
    TextureAtlasTga,
    /// Remastered atlas metadata sidecar file.
    TextureAtlasMeta,
    /// Standard `.wav` audio shipped by Remastered.
    WavAudio,
    /// Remastered Bink2 video file.
    Bk2Video,
    /// GPU texture payload such as `.dds`.
    DdsTexture,
}

/// Current level of integrity evidence attached to one artifact.
///
/// This is intentionally narrower than full D049 verification. At `G1.1`, the
/// probe only needs to capture what kind of confidence it already has before
/// the later importer/verify stages run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactIntegrity {
    /// The adapter saw the artifact path but did not inspect the file body yet.
    Untested,
    /// The adapter could open the artifact and enumerate its top-level index.
    IndexReadable,
    /// The adapter validated header-level structure.
    HeaderValidated,
    /// The adapter successfully round-tripped through the clean-room parser.
    ParseValidated,
}

/// Summary of one playable asset bundle found at a source path.
///
/// This is what D069 setup UI needs to explain "why should I use this source?"
/// and which kinds of content will become available if the player picks it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedAssetSet {
    /// Stable classification of the detected bundle.
    pub kind: DetectedAssetSetKind,
    /// Human-readable label shown in setup/maintenance UI.
    pub display_name: String,
    /// Artifact families required for this bundle to be considered importable.
    pub required_artifacts: Vec<SourceArtifactKind>,
}

/// Normalized D069-style candidate emitted by one source adapter probe.
///
/// A source adapter resolves platform-specific details into this shared shape
/// so the setup wizard and importer can reason about Steam, GOG, EA App, and
/// manual installs consistently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentSourceCandidate {
    /// Normalized platform/distribution kind.
    pub source_kind: ContentSourceKind,
    /// Resolved root path of the detected install or content folder.
    pub path: String,
    /// High-level probe outcome for the source as a whole.
    pub probe_status: ProbeStatus,
    /// Playable asset bundles discovered at this source.
    pub detected_assets: Vec<DetectedAssetSet>,
    /// Human-readable explanation of warnings, source hints, or recovery advice.
    pub notes: Vec<String>,
    /// Suggested D069 import mode for the detected source.
    pub import_mode: ContentSourceImportMode,
    /// Rights classification preserved for provenance and publish-readiness rules.
    pub rights_class: SourceRightsClass,
}

/// One artifact recorded inside a source-manifest snapshot.
///
/// The importer uses this per-item view for later verify/index work, while the
/// D069 maintenance flows use it for repair/re-scan guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceManifestArtifact {
    /// Stable human-readable identifier used in logs and diagnostics.
    pub logical_name: String,
    /// Normalized artifact family for parser routing.
    pub source_type: SourceArtifactKind,
    /// Original location inside the owned install.
    pub path: String,
    /// Byte size when known at probe time.
    pub size_bytes: Option<u64>,
    /// Whether the importer considers this item required for the detected asset set.
    pub required: bool,
    /// Probe outcome for this one artifact.
    pub probe_status: ProbeStatus,
    /// Probe confidence before later full import/verify passes.
    pub probe_confidence: ProbeConfidence,
    /// Most specific integrity evidence the adapter established.
    pub integrity: ArtifactIntegrity,
    /// Provenance labels carried forward into import/index records.
    pub provenance_tags: Vec<String>,
    /// Additional per-item diagnostics or operator hints.
    pub notes: Vec<String>,
}

/// Serialized snapshot emitted by a source adapter probe.
///
/// This snapshot is the stable handoff artifact for `G1.1`. Future source
/// adapters can emit it for fixture tests, CLI import-plan previews, setup
/// wizard inspection, and later repair/re-scan tooling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceManifestSnapshot {
    /// Schema version for the serialized snapshot format.
    pub schema_version: u32,
    /// Top-level source candidate shown to D069-style UI.
    pub candidate: ContentSourceCandidate,
    /// Normalized per-artifact probe results for the source.
    pub artifacts: Vec<SourceManifestArtifact>,
}

impl SourceManifestSnapshot {
    /// Builds a source-manifest snapshot using the current schema version.
    pub fn new(candidate: ContentSourceCandidate, artifacts: Vec<SourceManifestArtifact>) -> Self {
        Self {
            schema_version: SOURCE_MANIFEST_SCHEMA_VERSION,
            candidate,
            artifacts,
        }
    }
}

/// Contract implemented by a layout-specific source adapter.
///
/// A Steam adapter, GOG adapter, EA App adapter, or manual-directory adapter
/// can each satisfy this trait. The shared importer never needs to know how the
/// adapter found the files; it only consumes the normalized snapshot.
pub trait ContentSourceAdapter {
    /// Classifies the platform/distribution family the adapter is probing.
    fn source_kind(&self) -> ContentSourceKind;

    /// Probes the source install and emits a normalized manifest snapshot.
    ///
    /// Returning an error means the adapter could not safely inspect the
    /// install at all. Partial or warning-bearing results should instead be
    /// encoded into [`ProbeStatus`] and the snapshot notes.
    fn probe(&self) -> Result<SourceManifestSnapshot, SourceProbeError>;
}

/// Failure mode for a source adapter probe.
#[derive(Debug, Error)]
pub enum SourceProbeError {
    /// The adapter reached the path but the layout does not match the source it
    /// claims to support.
    #[error("source layout is unsupported: {0}")]
    UnsupportedLayout(String),
    /// The adapter could not read the source path or one of its directories.
    #[error("source path could not be read: {0}")]
    UnreadablePath(String),
    /// The adapter ran but could not produce any normalized probe output.
    #[error("source probe produced no snapshot")]
    EmptyProbe,
}

#[cfg(test)]
mod tests;
