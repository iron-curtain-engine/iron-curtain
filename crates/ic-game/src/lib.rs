// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ic-game — runnable Iron Curtain game client bootstrap
//!
//! This first local slice is intentionally narrow: it proves the repo can open
//! a Bevy window and draw one palette-expanded SHP sprite using the current
//! `ic-cnc-content` and `ic-render` foundations.

pub mod content_window;
pub mod demo;

/// Runs the current bootstrap client.
///
/// This now launches the first "content lab" window: a Bevy app that still
/// draws the synthetic demo sprite in the background, but overlays a real
/// catalog of locally detected Red Alert / Remastered content roots so engine
/// work can start proving coverage against real game data.
pub fn run() -> Result<(), demo::DemoSceneError> {
    content_window::run_content_window_client()
}

#[cfg(test)]
mod tests;
