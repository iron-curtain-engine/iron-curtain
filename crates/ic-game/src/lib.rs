// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ic-game — runnable Iron Curtain game client bootstrap
//!
//! This first local slice is intentionally narrow: it proves the repo can open
//! a Bevy window and draw one palette-expanded SHP sprite using the current
//! `ic-cnc-content` and `ic-render` foundations.

pub mod demo;

/// Runs the current bootstrap client.
///
/// This is intentionally small for the first visible `G2` step: build one
/// validated demo scene, configure Bevy, and open a window that displays that
/// scene. The full game loop, map loading, input orchestration, and sim
/// integration come later when the surrounding crates exist.
pub fn run() -> Result<(), demo::DemoSceneError> {
    demo::run_demo_client()
}

#[cfg(test)]
mod tests;
