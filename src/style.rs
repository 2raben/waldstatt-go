//! The look shared by every view and screen: the palette, the rivals' colours,
//! the font, simple shapes drawn in code, and text helpers.
//!
//! Bevy's built-in font has no umlauts, so Fira Sans (SIL Open Font License,
//! `assets/fonts/`) is compiled in and replaces it as the default font.

use bevy::asset::{AssetId, RenderAssetUsages};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::rules::RivalId;
use crate::rules::board::{MAX_RIVALS, RADIUS, to_side};

pub mod palette {
    use bevy::prelude::Color;

    pub const BACKGROUND: Color = Color::srgb(0.06, 0.07, 0.06);
    pub const PANEL: Color = Color::srgba(0.05, 0.07, 0.06, 0.86);
    pub const TEXT: Color = Color::srgb(0.95, 0.93, 0.86);
    pub const MUTED: Color = Color::srgb(0.68, 0.7, 0.62);
    pub const HEADING: Color = Color::srgb(0.95, 0.8, 0.45);
    pub const WARNING: Color = Color::srgb(1.0, 0.45, 0.35);
    pub const BUTTON: Color = Color::srgb(0.17, 0.24, 0.18);
    pub const BUTTON_HOVER: Color = Color::srgb(0.24, 0.34, 0.25);
    pub const BUTTON_PRESSED: Color = Color::srgb(0.32, 0.46, 0.33);
    pub const BUTTON_CHOSEN: Color = Color::srgb(0.55, 0.42, 0.16);
    pub const LINE: Color = Color::srgb(0.98, 0.96, 0.85);
    pub const BLOCKED: Color = Color::srgb(1.0, 0.3, 0.25);
    pub const DANGER: Color = Color::srgb(1.0, 0.2, 0.15);
}

/// The rivals' colours by seat, matching `rules::game::RIVAL_NAMES`.
const RIVAL_COLOURS: [[f32; 3]; MAX_RIVALS] = [
    [0.96, 0.74, 0.18],
    [0.89, 0.27, 0.22],
    [0.66, 0.4, 0.9],
    [0.94, 0.94, 0.92],
];

pub fn rival_rgb(rival: RivalId) -> [f32; 3] {
    RIVAL_COLOURS[rival.index() % MAX_RIVALS]
}

pub fn rival_colour(rival: RivalId) -> Color {
    let [r, g, b] = rival_rgb(rival);
    Color::srgb(r, g, b)
}

/// Fira Sans Bold, for headings.
#[derive(Resource)]
pub struct Fonts {
    pub bold: Handle<Font>,
}

/// White shapes drawn in code, tinted by sprites.
#[derive(Resource)]
pub struct Shapes {
    pub disc: Handle<Image>,
    pub ring: Handle<Image>,
    pub frame: Handle<Image>,
}

pub struct StylePlugin;

impl Plugin for StylePlugin {
    fn build(&self, app: &mut App) {
        let world = app.world_mut();
        let mut fonts = world.resource_mut::<Assets<Font>>();
        let regular =
            Font::from_bytes(include_bytes!("../assets/fonts/FiraSans-Regular.ttf").to_vec());
        // Replaces Bevy's default font, which has no umlauts.
        let _ = fonts.insert(AssetId::default(), regular);
        let bold = fonts.add(Font::from_bytes(
            include_bytes!("../assets/fonts/FiraSans-Bold.ttf").to_vec(),
        ));
        let mut images = world.resource_mut::<Assets<Image>>();
        let shapes = Shapes {
            disc: images.add(shape(64, |d| (31.5 - d).clamp(0.0, 1.0))),
            ring: images.add(shape(64, |d| ((31.5 - d).min(d - 25.0)).clamp(0.0, 1.0))),
            frame: images.add(frame(32)),
        };
        app.insert_resource(Fonts { bold })
            .insert_resource(shapes)
            .insert_resource(ClearColor(palette::BACKGROUND));
    }
}

/// A square white image whose alpha is `alpha(distance from the centre)`.
fn shape(size: u32, alpha: impl Fn(f32) -> f32) -> Image {
    let mut image = blank(size, size);
    let centre = (size as f32 - 1.0) / 2.0;
    if let Some(data) = image.data.as_mut() {
        for y in 0..size {
            for x in 0..size {
                let d = Vec2::new(x as f32 - centre, y as f32 - centre).length();
                let i = ((y * size + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[255, 255, 255, (alpha(d) * 255.0) as u8]);
            }
        }
    }
    image
}

/// A white outline of a hex standing point up, `size` pixels across its
/// sides and as high as such a hex is.
fn frame(size: u32) -> Image {
    let height = (size as f32 * 2.0 * RADIUS).round() as u32;
    let mut image = blank(size, height);
    let line = 2.5 / size as f32;
    if let Some(data) = image.data.as_mut() {
        for y in 0..height {
            for x in 0..size {
                // In hex widths from the middle.
                let d = Vec2::new(
                    (x as f32 + 0.5) / size as f32 - 0.5,
                    ((y as f32 + 0.5) / height as f32 - 0.5) * 2.0 * RADIUS,
                );
                let inside = to_side(d);
                let alpha = if inside < 0.0 {
                    0.0
                } else {
                    ((line - inside) * size as f32).clamp(0.0, 1.0)
                };
                let i = ((y * size + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[255, 255, 255, (alpha * 255.0) as u8]);
            }
        }
    }
    image
}

/// A transparent image to paint into, sampled without smoothing.
pub fn blank(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

pub fn text(content: impl Into<String>, size: f32, colour: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(colour),
    )
}

pub fn heading(fonts: &Fonts, content: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: fonts.bold.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(palette::HEADING),
    )
}

/// Minutes and seconds, as `m:ss`.
pub fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
