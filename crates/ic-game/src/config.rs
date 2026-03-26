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
    pub graphics: GraphicsConfig,
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
            graphics: GraphicsConfig::default(),
            sources: Vec::new(),
            performance: PerformanceConfig::default(),
            playback: PlaybackConfig::default(),
            audio: AudioConfig::default(),
        }
    }
}

// ─── Graphics / profile config ───────────────────────────────────────────────

/// Which rendering and feature profile to use for the frontend.
///
/// Determines whether a GPU renderer is required and which visual features
/// are enabled.  The engine resolves `auto` at startup using hardware
/// capabilities, user settings, and content requirements.
///
/// See the Content Lab GPU-optional architecture proposal for the full
/// rollout plan (Phase 1 — config only; classic not yet implemented).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GraphicsProfile {
    /// Resolve automatically from hardware, user settings, and content.
    #[default]
    Auto,
    /// No GPU renderer — reduced browser and viewer, CPU-only path.
    /// **Phase 1 placeholder:** selecting this profile prints a warning and
    /// falls back to the modern frontend until the classic frontend is built.
    Classic,
    /// Modern GPU frontend, modest visuals, minimal power/heat.
    Balanced,
    /// Modern GPU frontend, richer effects (CRT, palette post-processing).
    Enhanced,
    /// Enhanced plus authoring, diagnostics, and developer features.
    Studio,
}

impl GraphicsProfile {
    /// Human-readable short label used in the F3 debug overlay.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Classic => "classic",
            Self::Balanced => "balanced",
            Self::Enhanced => "enhanced",
            Self::Studio => "studio",
        }
    }

    /// Whether this profile requires an active GPU renderer.
    pub fn requires_gpu(self) -> bool {
        !matches!(self, Self::Classic)
    }
}

/// User-level GPU policy override — takes precedence over profile resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GpuPolicy {
    /// Resolve automatically (recommended).
    #[default]
    Auto,
    /// Force classic (non-GPU) frontend regardless of profile.
    Off,
    /// Prefer modern GPU frontend.
    On,
    /// Fail startup with a clear message if GPU frontend cannot initialise.
    Require,
}

/// What to do if the requested frontend cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FallbackPolicy {
    /// Fall back to the classic frontend (default).
    #[default]
    Classic,
    /// Abort with a clear error message instead of falling back.
    Fail,
}

/// Graphics and frontend profile configuration.
///
/// Controls which rendering frontend is launched, whether the GPU is required,
/// and which visual effect buckets are enabled.
///
/// CLI flags (`--gpu`, `--graphics`, `--effects`) override these values.
/// Precedence: CLI > user TOML > content/mod metadata > engine default.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GraphicsConfig {
    /// Rendering and feature profile (auto / classic / balanced / enhanced / studio).
    pub profile: GraphicsProfile,
    /// GPU policy override (auto / off / on / require).
    pub gpu: GpuPolicy,
    /// What to do if the chosen frontend cannot start (classic / fail).
    pub fallback: FallbackPolicy,
    /// Optional visual effect buckets to enable (e.g. `["scanlines", "crt-mask"]`).
    pub effects: Vec<String>,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            profile: GraphicsProfile::Auto,
            gpu: GpuPolicy::Auto,
            fallback: FallbackPolicy::Classic,
            effects: vec!["scanlines".into()],
        }
    }
}

/// V-sync / swap-chain present mode.
///
/// Controls how rendered frames are delivered to the display.  The default
/// (`fifo-relaxed`) avoids frame-doubling stutter from simulation jitter or
/// asset decode spikes without introducing tearing during normal operation.
/// Falls back to strict FIFO if the backend doesn't support the relaxed
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum VsyncMode {
    /// Non-blocking present — never stalls on a vsync deadline.
    /// May produce visible tearing on some displays.  Use if
    /// `SurfaceError::Timeout` occurs on an integrated GPU under heavy load.
    Off,
    /// Let the driver/OS choose the best available strategy (usually FIFO).
    Auto,
    /// Strict FIFO vsync — guaranteed no tearing, at the cost of up to one
    /// frame of added latency.
    Fifo,
    /// Mailbox (triple-buffer) vsync — frames replace each other without
    /// waiting for a deadline; lower latency than `fifo` but not universally
    /// supported.
    Mailbox,
    /// Relaxed FIFO vsync — behaves like `fifo` when frames arrive on time, but
    /// presents immediately (allowing a single tear) when a frame misses its
    /// vblank deadline instead of waiting for the next one.  Eliminates the
    /// frame-doubling stutter that strict `fifo` produces under variable load
    /// (simulation spikes, asset decoding, heavy ECS ticks).  Best all-round
    /// choice; falls back to `fifo` if the backend does not support it.
    #[default]
    FifoRelaxed,
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
    /// V-sync / present mode (off / auto / fifo / mailbox / fifo-relaxed).
    pub vsync: VsyncMode,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            title: "Iron Curtain - Content Lab".into(),
            mode: "borderless-fullscreen".into(),
            width: 1280,
            height: 720,
            clear_color: [15, 20, 24],
            // FifoRelaxed: no tearing in the common case, but presents
            // immediately on a missed deadline instead of stalling — so
            // simulation spikes, asset loads, and heavy ECS ticks don't
            // cause frame-doubling stutters.  Falls back to Fifo on
            // backends that don't support it.
            vsync: VsyncMode::FifoRelaxed,
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
    /// Load entire MIX/MEG archives into RAM at startup for faster browsing.
    pub preload_archives: bool,
    /// Maximum frames per second when the window is focused.
    /// `0` = auto-detect from the display's refresh rate (recommended).
    /// Set to a fixed value (e.g. `60`) to override.
    pub fps_cap: u32,
    /// Maximum frames per second when the window loses focus.
    /// `0` = auto-detect from the display's refresh rate.
    /// Must stay above `ceil(max_tps / MAX_TICKS_PER_FRAME)` (currently 13)
    /// to prevent the simulation from falling behind — critical during
    /// multiplayer lockstep where one player lagging starves every other.
    pub unfocused_fps_cap: u32,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            preload_archives: false,
            fps_cap: 0,
            // The game loop processes up to MAX_TICKS_PER_FRAME (4) sim
            // ticks per render frame.  At the Fastest game speed (50 tps)
            // the renderer must run at least ceil(50/4) = 13 fps to avoid
            // falling behind the simulation — critical during multiplayer
            // lockstep where lagging starves every other player.
            // 15 fps covers all speed presets with headroom and still cuts
            // GPU load ~4× vs 60 fps.
            unfocused_fps_cap: 15,
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
    /// Darken every other row to simulate CRT scanlines.
    pub scanlines: bool,
    /// Apply Bayer 4×4 ordered dithering when expanding VQA palette-indexed
    /// frames to RGBA.  Breaks up gradient banding from the original 6-bit
    /// VGA palette (64 levels per channel → 8-bit).
    /// Set to `false` to display raw palette colours without any dithering.
    pub vqa_dither: bool,
    /// Apply TPDF (triangular probability density function) 1-LSB dither to
    /// decoded VQA audio samples.  IMA ADPCM's 4-bit quantisation introduces
    /// correlated error harmonics; dither whitens the noise floor.
    /// Set to `false` to pass raw decoded PCM to the audio device.
    pub audio_dither: bool,
    /// Apply a single-pole IIR high-pass filter to decoded VQA audio to
    /// suppress DC offset.  SND1 (Westwood ADPCM) initialises its predictor
    /// to mid-scale (0x80), which causes a DC step at stream start.
    /// Set to `false` to skip DC correction.
    pub audio_dc_correction: bool,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            default_animation_fps: 12.0,
            preview_max_width: 0.72,
            preview_max_height: 0.72,
            movie_max_width: 0.96,
            movie_max_height: 0.96,
            scanlines: true,
            vqa_dither: true,
            audio_dither: true,
            audio_dc_correction: true,
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
    fn vqa_dither_defaults_to_true() {
        let config = GameConfig::from_toml("").unwrap();
        assert!(config.playback.vqa_dither, "vqa_dither should default to true");
    }

    #[test]
    fn vqa_dither_can_be_disabled_via_toml() {
        let toml = r#"
            [playback]
            vqa_dither = false
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert!(!config.playback.vqa_dither, "vqa_dither should be false when set in TOML");
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

    #[test]
    fn audio_dither_defaults_to_true() {
        let config = GameConfig::from_toml("").unwrap();
        assert!(
            config.playback.audio_dither,
            "audio_dither should default to true"
        );
    }

    #[test]
    fn audio_dither_can_be_disabled_via_toml() {
        let toml = r#"
            [playback]
            audio_dither = false
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert!(
            !config.playback.audio_dither,
            "audio_dither should be false when set in TOML"
        );
    }

    #[test]
    fn audio_dc_correction_defaults_to_true() {
        let config = GameConfig::from_toml("").unwrap();
        assert!(
            config.playback.audio_dc_correction,
            "audio_dc_correction should default to true"
        );
    }

    #[test]
    fn audio_dc_correction_can_be_disabled_via_toml() {
        let toml = r#"
            [playback]
            audio_dc_correction = false
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert!(
            !config.playback.audio_dc_correction,
            "audio_dc_correction should be false when set in TOML"
        );
    }

    #[test]
    fn graphics_defaults_to_auto_profile() {
        let config = GameConfig::from_toml("").unwrap();
        assert_eq!(config.graphics.profile, GraphicsProfile::Auto);
        assert_eq!(config.graphics.gpu, GpuPolicy::Auto);
        assert_eq!(config.graphics.fallback, FallbackPolicy::Classic);
    }

    #[test]
    fn graphics_effects_default_includes_scanlines() {
        let config = GameConfig::from_toml("").unwrap();
        assert!(
            config.graphics.effects.iter().any(|e| e == "scanlines"),
            "default effects should include scanlines"
        );
    }

    #[test]
    fn graphics_profile_parses_from_toml() {
        let toml = r#"
            [graphics]
            profile = "enhanced"
            gpu = "require"
            fallback = "fail"
            effects = ["scanlines", "crt-mask"]
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert_eq!(config.graphics.profile, GraphicsProfile::Enhanced);
        assert_eq!(config.graphics.gpu, GpuPolicy::Require);
        assert_eq!(config.graphics.fallback, FallbackPolicy::Fail);
        assert_eq!(config.graphics.effects, ["scanlines", "crt-mask"]);
    }

    #[test]
    fn graphics_classic_profile_parses() {
        let toml = r#"[graphics]
profile = "classic"
gpu = "off""#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert_eq!(config.graphics.profile, GraphicsProfile::Classic);
        assert_eq!(config.graphics.gpu, GpuPolicy::Off);
    }

    #[test]
    fn graphics_profile_requires_gpu_is_correct() {
        assert!(!GraphicsProfile::Classic.requires_gpu());
        assert!(GraphicsProfile::Balanced.requires_gpu());
        assert!(GraphicsProfile::Enhanced.requires_gpu());
        assert!(GraphicsProfile::Studio.requires_gpu());
        assert!(GraphicsProfile::Auto.requires_gpu());
    }

    #[test]
    fn vsync_defaults_to_fifo_relaxed() {
        let config = GameConfig::from_toml("").unwrap();
        assert_eq!(config.display.vsync, VsyncMode::FifoRelaxed);
    }

    #[test]
    fn vsync_parses_all_variants_from_toml() {
        for (toml_val, expected) in [
            ("off", VsyncMode::Off),
            ("auto", VsyncMode::Auto),
            ("fifo", VsyncMode::Fifo),
            ("mailbox", VsyncMode::Mailbox),
            ("fifo-relaxed", VsyncMode::FifoRelaxed),
        ] {
            let toml = format!("[display]\nvsync = \"{toml_val}\"");
            let config = GameConfig::from_toml(&toml).unwrap();
            assert_eq!(config.display.vsync, expected, "failed for vsync = {toml_val:?}");
        }
    }

    #[test]
    fn fps_cap_defaults() {
        let config = GameConfig::from_toml("").unwrap();
        assert_eq!(config.performance.fps_cap, 0, "fps_cap should default to 0 (auto)");
        assert_eq!(config.performance.unfocused_fps_cap, 15, "unfocused_fps_cap should default to 15");
    }

    #[test]
    fn fps_cap_parses_from_toml() {
        let toml = r#"
            [performance]
            fps_cap = 60
            unfocused_fps_cap = 15
        "#;
        let config = GameConfig::from_toml(toml).unwrap();
        assert_eq!(config.performance.fps_cap, 60);
        assert_eq!(config.performance.unfocused_fps_cap, 15);
    }
}
