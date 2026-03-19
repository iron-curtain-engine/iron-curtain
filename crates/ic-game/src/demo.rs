// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Demo bootstrap scene for the first visible game-client milestone.
//!
//! This module is deliberately teaching-oriented because it is the first place
//! where the repo opens a Bevy window and draws something on screen.
//!
//! The short version of the flow is:
//! - build one synthetic RA-style sprite through the real SHP/PAL wrappers
//! - convert the indexed palette frame into temporary RGBA pixels
//! - hand those pixels to Bevy as an `Image`
//! - spawn a `Camera2d` plus one `Sprite` that uses that image
//!
//! That is not the final renderer. The long-term design still calls for a more
//! faithful palette-aware sprite pipeline in `ic-render`. This module is the
//! first visible bootstrap that proves the current crate boundaries are
//! sufficient to show C&C-family content in a real window.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{Window, WindowPlugin, WindowResolution};
use ic_cnc_content::pal::Palette;
use ic_cnc_content::shp::ShpSprite;
use ic_cnc_content::IcCncContentPlugin;
use ic_render::camera::{ClassicIsometricCameraModel, GameCamera};
use ic_render::scene::{
    PaletteTextureSource, RenderLayer, Renderable, SceneValidationError, SpriteSheetSource,
    StaticRenderScene, StaticRenderSprite,
};
use ic_render::sprite::{IndexedSpriteFrame, RgbaSpriteFrame, SpriteBootstrapError};
use ic_render::IcRenderPlugin;
use thiserror::Error;

// A 720p-ish window is large enough to inspect pixel-art scaling while still
// being a safe default for ordinary desktop setups.
const BOOTSTRAP_WINDOW_WIDTH: u32 = 1280;
const BOOTSTRAP_WINDOW_HEIGHT: u32 = 720;
// The source frame is only 24x24 pixels. Scaling it up keeps the first visible
// proof readable without needing camera zoom/orbit systems yet.
const BOOTSTRAP_SPRITE_SCALE: f32 = 8.0;
const DEMO_FRAME_WIDTH: u16 = 24;
const DEMO_FRAME_HEIGHT: u16 = 24;

/// Errors returned while preparing or running the bootstrap demo.
#[derive(Debug, Error)]
pub enum DemoSceneError {
    #[error("content format error: {0}")]
    Formats(#[from] ic_cnc_content::cnc_formats::Error),
    #[error("render scene validation error: {0}")]
    Scene(#[from] SceneValidationError),
    #[error("bootstrap sprite conversion error: {0}")]
    Sprite(#[from] SpriteBootstrapError),
}

/// One fully prepared demo sprite plus the RGBA pixels used to display it.
///
/// The render-side descriptor and the temporary RGBA frame intentionally stay
/// together. `StaticRenderSprite` proves we passed through the explicit
/// parser-to-render handoff, while `RgbaSpriteFrame` is the temporary image
/// data Bevy can draw immediately today.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct BootstrapDemoScene {
    /// Validated render descriptor for the sprite shown in the bootstrap app.
    pub sprite: StaticRenderSprite,
    /// Temporary RGBA pixels used for the very first visible windowed slice.
    pub frame: RgbaSpriteFrame,
}

impl BootstrapDemoScene {
    fn image(&self) -> Image {
        // Bevy stores texture bytes in an `Image` asset. We hand it a fully
        // expanded RGBA buffer here because the future indexed-palette GPU path
        // does not exist yet in this repo.
        Image::new(
            Extent3d {
                width: self.frame.width(),
                height: self.frame.height(),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.frame.rgba8_pixels().to_vec(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
}

/// Builds the current synthetic demo scene through the real content pipeline.
///
/// The sprite is intentionally generated in memory instead of loading a checked
/// in asset file. That keeps the proof deterministic and makes the data layout
/// readable in-source while still exercising:
/// - SHP encoding/parsing
/// - SHP frame decoding
/// - PAL parsing and RGB expansion
/// - parser-to-render handoff metadata
/// - bootstrap indexed-to-RGBA conversion
pub fn build_demo_scene() -> Result<BootstrapDemoScene, DemoSceneError> {
    let frame_pixels = build_demo_frame_pixels();
    let shp_bytes = ic_cnc_content::cnc_formats::shp::encode_frames(
        &[frame_pixels.as_slice()],
        DEMO_FRAME_WIDTH,
        DEMO_FRAME_HEIGHT,
    )?;
    let shp = ShpSprite::parse(shp_bytes)?;
    let palette = Palette::parse(build_demo_palette_bytes())?;

    let sheet = SpriteSheetSource::from_handoff(shp.render_handoff());
    let sprite =
        StaticRenderSprite::new(sheet, 0, palette.render_handoff(), RenderLayer::Vehicles)?;

    let decoded_frames = shp.decode_frames()?;
    let indexed_frame = IndexedSpriteFrame::new(
        DEMO_FRAME_WIDTH as u32,
        DEMO_FRAME_HEIGHT as u32,
        decoded_frames
            .into_iter()
            .next()
            .expect("encoded test SHP always contains one frame"),
    )?;
    let render_palette = PaletteTextureSource::from_handoff(palette.render_handoff());
    let frame = indexed_frame.to_rgba(&render_palette);

    Ok(BootstrapDemoScene { sprite, frame })
}

/// Runs the bootstrap client window until the user closes it.
///
/// `App` is Bevy's top-level runtime object. It owns the ECS world, resources,
/// schedules, plugins, and the main loop. This function keeps the setup narrow
/// and explicit so it is easy to see what Bevy pieces are required for "open a
/// window and draw one sprite."
pub fn run_demo_client() -> Result<(), DemoSceneError> {
    let demo_scene = build_demo_scene()?;

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Iron Curtain - G2 Bootstrap".into(),
                        resolution: WindowResolution::new(
                            BOOTSTRAP_WINDOW_WIDTH,
                            BOOTSTRAP_WINDOW_HEIGHT,
                        ),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(IcCncContentPlugin)
        .add_plugins(IcRenderPlugin)
        .insert_resource(ClearColor(Color::srgb_u8(15, 20, 24)))
        .insert_resource(demo_scene)
        .add_systems(Startup, setup_demo_scene)
        .run();

    Ok(())
}

/// Startup system that turns the prepared demo resource into visible entities.
///
/// In Bevy, a "system" is ordinary Rust code that Bevy schedules for you. A
/// `Startup` system runs once before the first frame. This is the right place
/// for the first window proof because we only need one camera, one texture,
/// and one sprite entity.
fn setup_demo_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut static_scene: ResMut<StaticRenderScene>,
    game_camera: Res<GameCamera>,
    demo_scene: Res<BootstrapDemoScene>,
) {
    // `Camera2d` is a marker component that asks Bevy to create a standard 2D
    // camera entity. Without at least one camera, nothing is rendered.
    commands.spawn(Camera2d);

    // We keep the validated descriptor in `StaticRenderScene` so later render
    // work can build on the same resource instead of inventing a separate demo
    // path.
    static_scene.push_sprite(demo_scene.sprite.clone());

    let texture = images.add(demo_scene.image());
    let model = ClassicIsometricCameraModel::default();
    let translation = model.world_to_render(
        demo_scene.sprite.world_position(),
        &game_camera,
        demo_scene.sprite.layer(),
    );

    // `Sprite::from_image` asks Bevy's sprite renderer to draw this texture.
    // The transform is where we place and scale it in the world.
    commands.spawn((
        Sprite::from_image(texture),
        Transform::from_translation(translation).with_scale(Vec3::splat(BOOTSTRAP_SPRITE_SCALE)),
    ));
}

fn build_demo_palette_bytes() -> Vec<u8> {
    let mut bytes = vec![0u8; ic_cnc_content::cnc_formats::pal::PALETTE_BYTES];

    // Index 0 stays black and becomes transparent in the bootstrap RGBA path.
    // The other entries give the synthetic sprite a readable "vehicle body /
    // cockpit / highlight / treads" split without pretending to be final RA art.
    set_palette_entry(&mut bytes, 1, [63, 58, 14]);
    set_palette_entry(&mut bytes, 2, [48, 48, 52]);
    set_palette_entry(&mut bytes, 3, [20, 44, 63]);
    set_palette_entry(&mut bytes, 4, [50, 12, 12]);

    bytes
}

fn set_palette_entry(bytes: &mut [u8], index: usize, rgb6: [u8; 3]) {
    let base = index * 3;
    bytes[base..base + 3].copy_from_slice(&rgb6);
}

fn build_demo_frame_pixels() -> Vec<u8> {
    let width = DEMO_FRAME_WIDTH as usize;
    let height = DEMO_FRAME_HEIGHT as usize;
    let mut pixels = vec![0u8; width * height];

    // Draw a compact top-down "vehicle-like" silhouette:
    // a rectangular hull, a smaller cockpit, darker tread pixels near the
    // bottom, and a bright highlight near the front.
    for y in 5..19 {
        for x in 7..17 {
            pixels[y * width + x] = 2;
        }
    }

    for y in 7..11 {
        for x in 9..15 {
            pixels[y * width + x] = 3;
        }
    }

    for &(x, y) in &[(8, 17), (15, 17), (8, 18), (15, 18)] {
        pixels[y * width + x] = 4;
    }

    for &(x, y) in &[(10, 6), (11, 5), (12, 5), (13, 6)] {
        pixels[y * width + x] = 1;
    }

    pixels
}
