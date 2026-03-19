// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the `G1.1` source probe contract and manifest snapshot schema.

use super::*;

fn classic_asset_set() -> DetectedAssetSet {
    DetectedAssetSet {
        kind: DetectedAssetSetKind::RedAlertClassic,
        display_name: "Red Alert classic assets".to_owned(),
        required_artifacts: vec![
            SourceArtifactKind::MixArchive,
            SourceArtifactKind::ShpSpriteSheet,
            SourceArtifactKind::Palette,
            SourceArtifactKind::AudAudio,
            SourceArtifactKind::VqaVideo,
            SourceArtifactKind::RulesIni,
        ],
    }
}

fn remastered_asset_set() -> DetectedAssetSet {
    DetectedAssetSet {
        kind: DetectedAssetSetKind::RemasteredCollection,
        display_name: "Command & Conquer Remastered Collection".to_owned(),
        required_artifacts: vec![
            SourceArtifactKind::MegArchive,
            SourceArtifactKind::TextureAtlasTga,
            SourceArtifactKind::TextureAtlasMeta,
            SourceArtifactKind::WavAudio,
            SourceArtifactKind::Bk2Video,
        ],
    }
}

fn artifact(
    logical_name: &str,
    source_type: SourceArtifactKind,
    path: &str,
    size_bytes: Option<u64>,
    integrity: ArtifactIntegrity,
    provenance_tags: &[&str],
) -> SourceManifestArtifact {
    SourceManifestArtifact {
        logical_name: logical_name.to_owned(),
        source_type,
        path: path.to_owned(),
        size_bytes,
        required: true,
        probe_status: ProbeStatus::Detected,
        probe_confidence: ProbeConfidence::High,
        integrity,
        provenance_tags: provenance_tags.iter().map(ToString::to_string).collect(),
        notes: Vec::new(),
    }
}

fn steam_classic_snapshot() -> SourceManifestSnapshot {
    SourceManifestSnapshot::new(
        ContentSourceCandidate {
            source_kind: ContentSourceKind::Steam,
            path: "C:/Program Files (x86)/Steam/steamapps/common/Red Alert".to_owned(),
            probe_status: ProbeStatus::Detected,
            detected_assets: vec![classic_asset_set()],
            notes: vec![
                "Steam library layout matched the expected Red Alert archive set.".to_owned(),
                "Quick Setup should default to a read-only copy import for resilience.".to_owned(),
            ],
            import_mode: ContentSourceImportMode::Copy,
            rights_class: SourceRightsClass::OwnedProprietary,
        },
        vec![
            artifact(
                "main.mix",
                SourceArtifactKind::MixArchive,
                "C:/Program Files (x86)/Steam/steamapps/common/Red Alert/main.mix",
                Some(151_552_000),
                ArtifactIntegrity::IndexReadable,
                &["owned-import", "steam", "classic-ra"],
            ),
            artifact(
                "conquer.mix",
                SourceArtifactKind::MixArchive,
                "C:/Program Files (x86)/Steam/steamapps/common/Red Alert/conquer.mix",
                Some(84_213_760),
                ArtifactIntegrity::IndexReadable,
                &["owned-import", "steam", "classic-ra"],
            ),
            artifact(
                "temperat.pal",
                SourceArtifactKind::Palette,
                "C:/Program Files (x86)/Steam/steamapps/common/Red Alert/temperat.pal",
                Some(768),
                ArtifactIntegrity::ParseValidated,
                &["owned-import", "steam", "classic-ra"],
            ),
        ],
    )
}

fn gog_classic_snapshot() -> SourceManifestSnapshot {
    SourceManifestSnapshot::new(
        ContentSourceCandidate {
            source_kind: ContentSourceKind::Gog,
            path: "C:/GOG Games/Command & Conquer Red Alert".to_owned(),
            probe_status: ProbeStatus::Detected,
            detected_assets: vec![classic_asset_set()],
            notes: vec![
                "GOG install root matched the expected Red Alert content layout.".to_owned(),
            ],
            import_mode: ContentSourceImportMode::Copy,
            rights_class: SourceRightsClass::OwnedProprietary,
        },
        vec![artifact(
            "redalert.mix",
            SourceArtifactKind::MixArchive,
            "C:/GOG Games/Command & Conquer Red Alert/redalert.mix",
            Some(142_344_192),
            ArtifactIntegrity::HeaderValidated,
            &["owned-import", "gog", "classic-ra"],
        )],
    )
}

fn ea_classic_snapshot() -> SourceManifestSnapshot {
    SourceManifestSnapshot::new(
        ContentSourceCandidate {
            source_kind: ContentSourceKind::EaApp,
            path: "C:/Program Files/EA Games/Red Alert".to_owned(),
            probe_status: ProbeStatus::DetectedWithWarnings,
            detected_assets: vec![classic_asset_set()],
            notes: vec![
                "EA App install root matched a playable classic Red Alert layout.".to_owned(),
                "One optional video asset was missing; gameplay-critical archives were still present.".to_owned(),
            ],
            import_mode: ContentSourceImportMode::Copy,
            rights_class: SourceRightsClass::OwnedProprietary,
        },
        vec![
            artifact(
                "main.mix",
                SourceArtifactKind::MixArchive,
                "C:/Program Files/EA Games/Red Alert/main.mix",
                Some(151_552_000),
                ArtifactIntegrity::IndexReadable,
                &["owned-import", "ea-app", "classic-ra"],
            ),
            SourceManifestArtifact {
                logical_name: "intro.vqa".to_owned(),
                source_type: SourceArtifactKind::VqaVideo,
                path: "C:/Program Files/EA Games/Red Alert/movies/intro.vqa".to_owned(),
                size_bytes: None,
                required: false,
                probe_status: ProbeStatus::Missing,
                probe_confidence: ProbeConfidence::High,
                integrity: ArtifactIntegrity::Untested,
                provenance_tags: vec![
                    "owned-import".to_owned(),
                    "ea-app".to_owned(),
                    "classic-ra".to_owned(),
                ],
                notes: vec!["Optional movie asset missing from this install; import can continue.".to_owned()],
            },
        ],
    )
}

fn manual_classic_snapshot() -> SourceManifestSnapshot {
    SourceManifestSnapshot::new(
        ContentSourceCandidate {
            source_kind: ContentSourceKind::ManualDirectory,
            path: "D:/Games/RedAlert".to_owned(),
            probe_status: ProbeStatus::Detected,
            detected_assets: vec![classic_asset_set()],
            notes: vec![
                "Manual directory probe matched a classic Red Alert layout.".to_owned(),
                "The source adapter should preserve the user-selected path for later re-scan flows.".to_owned(),
            ],
            import_mode: ContentSourceImportMode::Copy,
            rights_class: SourceRightsClass::OwnedProprietary,
        },
        vec![artifact(
            "rules.ini",
            SourceArtifactKind::RulesIni,
            "D:/Games/RedAlert/rules.ini",
            Some(98_304),
            ArtifactIntegrity::HeaderValidated,
            &["owned-import", "manual", "classic-ra"],
        )],
    )
}

fn steam_remastered_snapshot() -> SourceManifestSnapshot {
    SourceManifestSnapshot::new(
        ContentSourceCandidate {
            source_kind: ContentSourceKind::Steam,
            path: "C:/Program Files (x86)/Steam/steamapps/common/Command & Conquer Remastered".to_owned(),
            probe_status: ProbeStatus::Detected,
            detected_assets: vec![remastered_asset_set()],
            notes: vec![
                "Remastered should appear as a first-class source option in D069, not as a manual advanced path.".to_owned(),
                "The source install remains read-only; later import stages copy or extract into IC-managed storage.".to_owned(),
            ],
            import_mode: ContentSourceImportMode::Copy,
            rights_class: SourceRightsClass::OwnedProprietary,
        },
        vec![
            artifact(
                "redalert.meg",
                SourceArtifactKind::MegArchive,
                "C:/Program Files (x86)/Steam/steamapps/common/Command & Conquer Remastered/Data/redalert.meg",
                Some(2_147_483_648),
                ArtifactIntegrity::HeaderValidated,
                &["owned-import", "steam", "remastered"],
            ),
            artifact(
                "allied_units.tga",
                SourceArtifactKind::TextureAtlasTga,
                "C:/Program Files (x86)/Steam/steamapps/common/Command & Conquer Remastered/Data/Textures/allied_units.tga",
                Some(67_108_864),
                ArtifactIntegrity::Untested,
                &["owned-import", "steam", "remastered"],
            ),
            artifact(
                "allied_units.tga.meta",
                SourceArtifactKind::TextureAtlasMeta,
                "C:/Program Files (x86)/Steam/steamapps/common/Command & Conquer Remastered/Data/Textures/allied_units.tga.meta",
                Some(16_384),
                ArtifactIntegrity::HeaderValidated,
                &["owned-import", "steam", "remastered"],
            ),
        ],
    )
}

/// Proves that the normalized probe contract covers the primary owned-source
/// shapes called out by `G1.1`.
///
/// The contract must represent Steam, GOG, EA App, manual-directory, and
/// Remastered detections without forcing the later importer to care about each
/// platform's private layout quirks.
#[test]
fn probe_fixtures_cover_primary_owned_source_shapes() {
    let fixtures = [
        steam_classic_snapshot(),
        gog_classic_snapshot(),
        ea_classic_snapshot(),
        manual_classic_snapshot(),
        steam_remastered_snapshot(),
    ];

    let source_kinds = fixtures
        .iter()
        .map(|snapshot| snapshot.candidate.source_kind)
        .collect::<Vec<_>>();

    assert_eq!(
        source_kinds,
        vec![
            ContentSourceKind::Steam,
            ContentSourceKind::Gog,
            ContentSourceKind::EaApp,
            ContentSourceKind::ManualDirectory,
            ContentSourceKind::Steam,
        ]
    );
    assert!(fixtures
        .iter()
        .all(|snapshot| snapshot.schema_version == SOURCE_MANIFEST_SCHEMA_VERSION));
    assert!(fixtures
        .iter()
        .all(|snapshot| !snapshot.artifacts.is_empty()));
}

/// Proves that a Remastered probe stays a first-class D069 source instead of
/// collapsing into a generic manual-directory record.
///
/// This matters because the setup wizard promises a one-click "use Remastered"
/// path, and that promise requires the probe output to preserve both Steam
/// source identity and Remastered asset-family identity.
#[test]
fn remastered_fixture_preserves_first_class_probe_identity() {
    let snapshot = steam_remastered_snapshot();

    assert_eq!(snapshot.candidate.source_kind, ContentSourceKind::Steam);
    assert_eq!(
        snapshot.candidate.import_mode,
        ContentSourceImportMode::Copy
    );
    assert_eq!(
        snapshot.candidate.rights_class,
        SourceRightsClass::OwnedProprietary
    );
    assert_eq!(
        snapshot.candidate.detected_assets[0].kind,
        DetectedAssetSetKind::RemasteredCollection
    );
    assert!(snapshot
        .artifacts
        .iter()
        .any(|artifact| artifact.source_type == SourceArtifactKind::MegArchive));
}

/// Proves that a serialized source-manifest snapshot contains the schema fields
/// required by the owned-source import pipeline docs.
///
/// The test checks for path, size, source type, probe status, integrity, and
/// provenance tags because those are the fields later repair/re-scan and
/// provenance flows depend on.
#[test]
fn source_manifest_snapshot_serializes_required_probe_fields() {
    let yaml = serde_yaml::to_string(&steam_classic_snapshot())
        .expect("source manifest snapshot should serialize to YAML");

    assert!(yaml.contains("schema_version: 1"));
    assert!(yaml.contains("source_kind: Steam"));
    assert!(yaml.contains("path: C:/Program Files (x86)/Steam/steamapps/common/Red Alert/main.mix"));
    assert!(yaml.contains("size_bytes: 151552000"));
    assert!(yaml.contains("source_type: MixArchive"));
    assert!(yaml.contains("probe_status: Detected"));
    assert!(yaml.contains("integrity: IndexReadable"));
    assert!(yaml.contains("- owned-import"));
    assert!(yaml.contains("- steam"));
}
