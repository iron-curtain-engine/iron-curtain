// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Bevy-facing wrapper for explicit MiniYAML documents.
//!
//! D003 keeps real YAML canonical for Iron Curtain. This module therefore owns
//! only the legacy MiniYAML path used for C&C/OpenRA compatibility and exposes
//! it as an opt-in asset loader for `.miniyaml` files.
//!
//! From Bevy's point of view, `MiniYamlAsset` is the parsed typed document and
//! `MiniYamlLoader` is the adapter from source bytes to that document.

use bevy::asset::{io::Reader, Asset, AssetLoader, LoadContext};
use bevy::reflect::TypePath;
use cnc_formats::miniyaml as cnc_miniyaml;
use thiserror::Error;

/// Errors returned while loading MiniYAML through Bevy.
#[derive(Debug, Error)]
pub enum MiniYamlLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("MiniYAML parse error: {0}")]
    Parse(#[from] cnc_formats::Error),
}

/// Bevy asset that stores a parsed MiniYAML document tree.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct MiniYamlAsset {
    /// Parsed document tree produced by the clean-room MiniYAML parser.
    pub document: cnc_miniyaml::MiniYamlDoc,
}

impl MiniYamlAsset {
    /// Parses raw bytes into a MiniYAML document tree.
    ///
    /// This wrapper exists for the explicit legacy-compatibility path only.
    /// Invalid MiniYAML input is rejected by the clean-room parser instead of
    /// being silently treated as ordinary YAML.
    pub fn parse(bytes: &[u8]) -> Result<Self, cnc_formats::Error> {
        Ok(Self {
            document: cnc_miniyaml::MiniYamlDoc::parse(bytes)?,
        })
    }
}

/// Bevy loader for explicit `.miniyaml` assets.
///
/// The loader derives `TypePath` so Bevy can register and identify it in the
/// asset system.
#[derive(Default, TypePath)]
pub struct MiniYamlLoader;

impl AssetLoader for MiniYamlLoader {
    type Asset = MiniYamlAsset;
    type Settings = ();
    type Error = MiniYamlLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // The loader only needs one top-level parsed document, so the
        // `LoadContext` stays unused for now.
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(MiniYamlAsset::parse(&bytes)?)
    }

    fn extensions(&self) -> &[&str] {
        // D003 keeps real YAML canonical. We only claim the explicit MiniYAML
        // extension here until content-based `.yaml` auto-detection is wired.
        &["miniyaml"]
    }
}

#[cfg(test)]
mod tests;
