// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! TOML-based configuration for the Iron Curtain game client.
//!
//! The loader searches for configuration in this order:
//! 1. `config/iron-curtain.toml` (user override, gitignored)
//! 2. `config/iron-curtain.default.toml` (checked-in defaults)
//! 3. Hard-coded fallback values
//!
//! Environment variables in source paths are expanded at load time using the
//! `${VAR_NAME}` syntax.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Root configuration structure matching the TOML schema.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GameConfig {
    pub display: DisplayConfig,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    pub performance: PerformanceConfig,
    pub playback: PlaybackConfig,
    pub audio: AudioConfig,
}

impl Default for GameConfig {
    #[allow(clippy::derivable_impls, reason = "explicit default keeps the structure visible alongside the TOML schema")]
    fn default() -> Self {
        Self {
            display: DisplayConfig::default(),
            sources: Vec::new(),
            performance: PerformanceConfig::default(),
            playback: PlaybackConfig::default(),
            audio: AudioConfig::default(),
        }
    }
}

/// Display / window configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub title: String,
    pub mode: String,
    pub width: u32,
    pub height: u32,
    pub clear_color: [u8; 3],
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            title: "Iron Curtain - Content Lab".into(),
            mode: "borderless-fullscreen".into(),
            width: 1280,
            height: 720,
            clear_color: [15, 20, 24],
        }
    }
}

/// One content source root entry from the `[[sources]]` array.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceConfig {
    pub name: String,
    pub path: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default = "default_rights")]
    pub rights: String,
    #[serde(default = "default_shape")]
    pub shape: String,
}

fn default_kind() -> String {
    "manual".into()
}
fn default_rights() -> String {
    "owned-proprietary".into()
}
fn default_shape() -> String {
    "directory".into()
}

/// Performance tuning.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    pub preload_archives: bool,
}

impl Default for PerformanceConfig {
    #[allow(clippy::derivable_impls, reason = "explicit default documents the production value next to the struct")]
    fn default() -> Self {
        Self {
            preload_archives: false,
        }
    }
}

/// Playback / preview surface configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PlaybackConfig {
    pub default_animation_fps: f32,
    pub preview_max_width: f32,
    pub preview_max_height: f32,
    pub movie_max_width: f32,
    pub movie_max_height: f32,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            default_animation_fps: 12.0,
            preview_max_width: 0.72,
            preview_max_height: 0.72,
            movie_max_width: 0.96,
            movie_max_height: 0.96,
        }
    }
}

/// Audio configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub master_volume: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
        }
    }
}

impl GameConfig {
    /// Loads configuration from the standard search path.
    ///
    /// Searches for `config/iron-curtain.toml` first (user override), then
    /// `config/iron-curtain.default.toml` (checked-in defaults). If neither
    /// exists, returns hard-coded defaults.
    pub fn load() -> Self {
        let base = find_config_base_dir();
        let candidates = [
            base.join("config/iron-curtain.toml"),
            base.join("config/iron-curtain.default.toml"),
        ];

        for path in &candidates {
            if let Ok(contents) = std::fs::read_to_string(path) {
                match toml::from_str::<GameConfig>(&contents) {
                    Ok(config) => {
                        eprintln!("[config] loaded: {}", path.display());
                        return config;
                    }
                    Err(error) => {
                        eprintln!(
                            "[config] failed to parse {}: {error}",
                            path.display()
                        );
                    }
                }
            }
        }

        eprintln!("[config] no config file found, using built-in defaults");
        Self::default()
    }

    /// Loads from a specific TOML string (useful for tests).
    pub fn from_toml(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }
}

/// Expands `${VAR_NAME}` references in a path string.
pub fn expand_env_vars(input: &str) -> String {
    let mut result = input.to_string();
    while let Some(start) = result.find("${") {
        let Some(end) = result[start..].find('}') else {
            break;
        };
        let end = start + end;
        let var_name = &result[start + 2..end];
        let value = std::env::var(var_name).unwrap_or_default();
        result.replace_range(start..=end, &value);
    }
    result
}

/// Resolves a source path from config, expanding env vars.
pub fn resolve_source_path(raw: &str) -> PathBuf {
    PathBuf::from(expand_env_vars(raw))
}

/// Finds the project root directory by walking up from the executable or CWD
/// looking for a `config/` directory.
fn find_config_base_dir() -> PathBuf {
    // Try CWD first (typical for `cargo run`).
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("config").is_dir() {
        return cwd;
    }

    // Try walking up from the executable location.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if d.join("config").is_dir() {
                return d;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }

    cwd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses_from_empty_toml() {
        let config = GameConfig::from_toml("").unwrap();
        assert_eq!(config.display.width, 1280);
        assert_eq!(config.display.height, 720);
        assert!(config.sources.is_empty());
        assert!(!config.performance.preload_archives);
    }

    #[test]
    fn sources_parse_from_toml() {
        let toml = r#"
            [[sources]]
            name = "Test Root"
            path = "/tmp/test"
            kind = "manual"
            rights = "open-content"
            shape = "directory"
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert_eq!(config.sources.len(), 1);
        assert_eq!(config.sources[0].name, "Test Root");
        assert_eq!(config.sources[0].rights, "open-content");
    }

    #[test]
    fn env_var_expansion_works() {
        std::env::set_var("IC_TEST_VAR_12345", "/expanded/path");
        let result = expand_env_vars("${IC_TEST_VAR_12345}/subdir");
        assert_eq!(result, "/expanded/path/subdir");
        std::env::remove_var("IC_TEST_VAR_12345");
    }

    #[test]
    fn playback_defaults_match_expected_values() {
        let config = GameConfig::from_toml("").unwrap();
        assert!((config.playback.default_animation_fps - 12.0).abs() < f32::EPSILON);
        assert!((config.playback.preview_max_width - 0.72).abs() < f32::EPSILON);
        assert!((config.playback.movie_max_width - 0.96).abs() < f32::EPSILON);
    }

    #[test]
    fn display_mode_parses() {
        let toml = r#"
            [display]
            mode = "windowed"
            width = 1920
            height = 1080
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert_eq!(config.display.mode, "windowed");
        assert_eq!(config.display.width, 1920);
    }
}
