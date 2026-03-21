// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ic-game — runnable Iron Curtain game client bootstrap
//!
//! This local slice is still intentionally narrow, but it has moved beyond the
//! earliest "open one window and draw one sprite" proof. The crate now hosts
//! the first real-data content lab:
//!
//! - scan configured Red Alert / Remastered roots
//! - mount loose files plus `.mix` / `.meg` archive members into one catalog
//! - decode classic art/audio/video/text resources through `ic-cnc-content`
//! - present the selected resource through Bevy UI plus one world-space
//!   preview surface
//!
//! That keeps `ic-game` focused on visible integration proof for `G2` without
//! pretending to be a playable game loop yet.

pub mod config;
pub mod content_window;
pub mod demo;

/// Launch options combining TOML config with CLI overrides.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub config: config::GameConfig,
    /// When `true`, entire archive files (MIX/MEG) are loaded into RAM during
    /// the catalog scan so that subsequent entry extraction is a memcpy rather
    /// than disk I/O. Useful on machines with plenty of RAM browsing large
    /// Red Alert installs.
    pub preload_archives: bool,
}

impl LaunchOptions {
    /// Loads config from TOML, then applies CLI argument overrides.
    pub fn from_env() -> Self {
        let config = config::GameConfig::load();
        let mut opts = Self {
            preload_archives: config.performance.preload_archives,
            config,
        };

        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--preload-archives" => opts.preload_archives = true,
                "--windowed" => opts.config.display.mode = "windowed".into(),
                "--fullscreen" => opts.config.display.mode = "borderless-fullscreen".into(),
                _ => {}
            }
        }
        opts
    }
}

/// Runs the current bootstrap client.
///
/// This launches the fullscreen content lab: a Bevy app that keeps the
/// synthetic demo scene as background context but mirrors the selected real
/// resource onto the main preview surface, so engine work can prove media
/// coverage against actual Red Alert / Remastered data instead of only against
/// synthetic fixtures.
pub fn run() -> Result<(), demo::DemoSceneError> {
    let options = LaunchOptions::from_env();
    content_window::run_content_window_client(options)
}

#[cfg(test)]
mod tests;
