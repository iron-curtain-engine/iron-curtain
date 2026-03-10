// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ra-formats — Red Alert / C&C File Format Parsers
//!
//! This crate parses all file formats used by Red Alert, Tiberian Dawn,
//! and OpenRA mods: `.mix` archives, `.shp` sprites, `.pal` palettes,
//! `.aud` audio, `.vqa` video, YAML rules, and MiniYAML.
//!
//! ## Architecture Context
//!
//! `ra-formats` wraps the permissively-licensed `cnc-formats` crate (D076)
//! and adds EA-derived parsing logic from the GPL-licensed C&C source code.
//! This crate is GPL v3 because it contains code derived from EA's releases.
//!
//! The clean-room binary parsers live in `cnc-formats` (MIT/Apache-2.0).
//! Game-specific knowledge (e.g., RA1 unit rules, OpenRA compatibility
//! aliases) lives here in `ra-formats`.
//!
//! Design decisions: D003 (real YAML), D023 (OpenRA vocabulary aliases),
//! D025 (runtime MiniYAML loading), D026 (mod manifest), D075 (Remastered).
//!
//! See: <https://iron-curtain-engine.github.io/iron-curtain-design-docs/05-FORMATS.html>

// Re-export cnc-formats types for convenient access.
pub use cnc_formats;

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {
        // Format parsing tests will be added in M1/G1 when implementing
        // .mix, .shp, .pal, .aud parsers against the RA1 test corpus.
    }
}
