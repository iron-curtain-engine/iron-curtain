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
    ///
    /// CLI precedence: CLI > user TOML > engine default.
    /// Supported flags:
    ///   `--preload-archives`
    ///   `--windowed` / `--fullscreen`
    ///   `--gpu off|on|auto|require`
    ///   `--graphics classic|balanced|enhanced|studio|auto`
    ///   `--vsync off|auto|fifo|mailbox|fifo-relaxed`
    ///   `--fps-cap <N>` (0 = auto-detect from display refresh rate)
    pub fn from_env() -> Self {
        let config = config::GameConfig::load();
        let mut opts = Self {
            preload_archives: config.performance.preload_archives,
            config,
        };

        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--preload-archives" => opts.preload_archives = true,
                "--windowed" => opts.config.display.mode = "windowed".into(),
                "--fullscreen" => opts.config.display.mode = "borderless-fullscreen".into(),
                "--gpu" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.config.graphics.gpu = match val.as_str() {
                            "off" => config::GpuPolicy::Off,
                            "on" => config::GpuPolicy::On,
                            "require" => config::GpuPolicy::Require,
                            _ => config::GpuPolicy::Auto,
                        };
                        i += 1;
                    }
                }
                "--graphics" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.config.graphics.profile = match val.as_str() {
                            "classic" => config::GraphicsProfile::Classic,
                            "balanced" => config::GraphicsProfile::Balanced,
                            "enhanced" => config::GraphicsProfile::Enhanced,
                            "studio" => config::GraphicsProfile::Studio,
                            _ => config::GraphicsProfile::Auto,
                        };
                        i += 1;
                    }
                }
                "--vsync" => {
                    if let Some(val) = args.get(i + 1) {
                        opts.config.display.vsync = match val.as_str() {
                            "off"          => config::VsyncMode::Off,
                            "auto"         => config::VsyncMode::Auto,
                            "fifo"         => config::VsyncMode::Fifo,
                            "mailbox"      => config::VsyncMode::Mailbox,
                            "fifo-relaxed" => config::VsyncMode::FifoRelaxed,
                            other => {
                                eprintln!("[config] unknown --vsync value {other:?}, keeping default");
                                opts.config.display.vsync
                            }
                        };
                        i += 1;
                    }
                }
                "--fps-cap" => {
                    if let Some(val) = args.get(i + 1) {
                        if let Ok(n) = val.parse::<u32>() {
                            opts.config.performance.fps_cap = n;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
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
