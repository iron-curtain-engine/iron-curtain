// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for explicit MiniYAML compatibility loading.

use super::*;

/// Confirms that the loader only claims the explicit `.miniyaml` extension.
///
/// This protects D003's rule that true YAML remains canonical until the engine
/// grows content-based detection for legacy MiniYAML-in-`.yaml` cases.
#[test]
fn miniyaml_loader_only_claims_explicit_extension() {
    let loader = MiniYamlLoader;
    assert_eq!(loader.extensions(), &["miniyaml"]);
}

/// Proves that the wrapper exposes the parsed MiniYAML tree returned by
/// `cnc-formats`.
///
/// The inline document keeps the compatibility proof self-contained and easy
/// to audit without fixture files.
#[test]
fn miniyaml_asset_parses_document_tree() {
    let bytes = b"Root:\n\tChild: Value\n";
    let asset = MiniYamlAsset::parse(bytes).expect("valid MiniYAML should parse");

    let root = asset.document.node("Root").expect("root node must exist");
    assert_eq!(
        root.child("Child")
            .and_then(cnc_miniyaml::MiniYamlNode::value),
        Some("Value")
    );
}
