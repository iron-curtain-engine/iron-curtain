// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ic-cnc-content — Iron Curtain C&C content integration
//!
//! This crate is the Bevy-facing wrapper around the standalone
//! [`cnc-formats`](https://github.com/iron-curtain-engine/cnc-formats) crate.
//! It owns engine integration concerns such as Bevy `AssetLoader`s and any
//! future GPL-only extensions, while keeping clean-room binary parsing in the
//! sibling permissively licensed crate as required by D076.
//!
//! If you are new to Bevy, the important idea here is:
//! - an `Asset` is a typed value Bevy stores and gives back by `Handle<T>`
//! - an `AssetLoader` is the adapter that turns source bytes into that asset
//! - a `Plugin` is a reusable unit of app setup applied to a Bevy `App`
//!
//! This crate mostly exists to provide those Bevy-facing adapters around the
//! engine-agnostic parsing code in `cnc-formats`.

use bevy::app::{App, Plugin};
use bevy::asset::AssetApp;

pub mod aud;
pub mod miniyaml;
pub mod mix;
pub mod pal;
pub mod shp;
pub mod source;
pub mod vqa;

/// Re-export of the clean-room parser crate that powers the wrapper assets.
///
/// Keeping this visible lets downstream engine code use the same parser types
/// when it needs deeper format access than the Bevy-facing wrappers expose.
pub use cnc_formats;

/// Registers all legacy C&C content `AssetLoader`s with the Bevy app.
///
/// This plugin is the crate's main integration seam: engine code depends on it
/// to make `.mix`, `.shp`, `.pal`, `.aud`, `.vqa`, and explicit `.miniyaml`
/// assets available through Bevy's asset system.
pub struct IcCncContentPlugin;

impl Plugin for IcCncContentPlugin {
    fn build(&self, app: &mut App) {
        // `init_asset::<T>()` registers a typed asset store. A later system can
        // hold a `Handle<T>` and ask Bevy for the loaded value of `T`.
        //
        // `init_asset_loader::<L>()` registers the code Bevy should call when a
        // file with that loader's supported extension is requested.
        app.init_asset::<mix::MixArchive>()
            .init_asset_loader::<mix::MixLoader>()
            .init_asset::<shp::ShpSprite>()
            .init_asset_loader::<shp::ShpLoader>()
            .init_asset::<pal::Palette>()
            .init_asset_loader::<pal::PalLoader>()
            .init_asset::<aud::AudAudio>()
            .init_asset_loader::<aud::AudLoader>()
            .init_asset::<miniyaml::MiniYamlAsset>()
            .init_asset_loader::<miniyaml::MiniYamlLoader>()
            .init_asset::<vqa::VqaVideo>()
            .init_asset_loader::<vqa::VqaLoader>();
    }
}

#[cfg(test)]
mod tests;
