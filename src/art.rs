//! The sprites of the isometric view, painted in code when the game starts:
//! trees, a sapling, Waldleute in each rival's colour, a boat, and a glint on
//! the water.
//!
//! They are original drawings in the manner of classic isometric strategy
//! games: lit from the upper left, shaded, with a soft contact shadow. Each is
//! painted at [`SCALE`] pixels per world unit with supersampling and packed
//! into one atlas, so the board draws in few batches. A [`Piece`] is one
//! drawing in the atlas: its rectangle, its size in world units, and its
//! anchor, the point that stands on the ground.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::rules::RivalId;
use crate::rules::board::MAX_RIVALS;
use crate::rules::rng::tile_hash;
use crate::style::{blank, rival_rgb};

/// Atlas pixels per world unit.
const SCALE: f32 = 3.0;
/// Samples per pixel side.
const SAMPLES: usize = 3;
const ATLAS_WIDTH: usize = 1024;
/// Transparent pixels round each drawing, so smoothing never reaches the next.
const PAD: usize = 2;

pub type Rgb = [f32; 3];
type Rgba = [f32; 4];

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

pub fn scale(a: Rgb, f: f32) -> Rgb {
    a.map(|c| (c * f).min(1.0))
}

fn opaque(c: Rgb) -> Option<Rgba> {
    Some([c[0], c[1], c[2], 1.0])
}

/// A value in [0, 1] for each whole point, fixed by the seed.
pub fn grain(x: i32, y: i32, seed: u64) -> f32 {
    (tile_hash(seed, x, y) & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise in [0, 1], one feature per unit.
pub fn noise(x: f32, y: f32, seed: u64) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let h = |dx: i32, dy: i32| grain(x0 as i32 + dx, y0 as i32 + dy, seed);
    let top = h(0, 0) + (h(1, 0) - h(0, 0)) * sx;
    let bottom = h(0, 1) + (h(1, 1) - h(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// One drawing in the atlas.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub rect: Rect,
    /// Size in world units.
    pub size: Vec2,
    pub anchor: Vec2,
}

// The frames of a Waldmensch.
pub const STAND: usize = 0;
/// Four frames of walking.
pub const WALK: usize = 1;
/// Four frames of swinging the axe.
pub const CHOP: usize = 5;
/// Two frames of wading, the legs under water.
pub const WADE: usize = 9;
/// Sitting in a boat.
pub const SEATED: usize = 11;
pub const FRAMES: usize = 12;

#[derive(Resource)]
pub struct Art {
    pub atlas: Handle<Image>,
    pub trees: Vec<Piece>,
    /// How many of `trees` are conifers; the rest are broadleaf trees.
    pub conifers: usize,
    pub sapling: Piece,
    figures: Vec<Piece>,
    pub boat: Piece,
    pub glint: Piece,
}

impl Art {
    pub fn figure(&self, rival: RivalId, frame: usize) -> Piece {
        self.figures[(rival.index() % MAX_RIVALS) * FRAMES + frame % FRAMES]
    }

    /// A sprite showing `piece` at `scale` times its size, and its anchor.
    pub fn sprite(&self, piece: Piece, scale: f32) -> (Sprite, Anchor) {
        (
            Sprite {
                image: self.atlas.clone(),
                rect: Some(piece.rect),
                custom_size: Some(piece.size * scale),
                ..default()
            },
            Anchor(piece.anchor),
        )
    }
}

pub struct ArtPlugin;

impl Plugin for ArtPlugin {
    fn build(&self, app: &mut App) {
        let (image, art) = paint();
        let atlas = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(Art { atlas, ..art });
    }
}

/// A painted drawing before packing.
struct Drawing {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 4]>,
    size: Vec2,
    anchor: Vec2,
}

/// Paints `size` world units with `paint`, which gets points relative to the
/// foot (`foot` from the lower left), `y` up, and returns the colour there.
fn draw(size: Vec2, foot: Vec2, outline: bool, paint: impl Fn(Vec2) -> Option<Rgba>) -> Drawing {
    let width = (size.x * SCALE).ceil() as usize;
    let height = (size.y * SCALE).ceil() as usize;
    let size = Vec2::new(width as f32, height as f32) / SCALE;
    let n = SAMPLES as f32;
    let mut pixels = vec![[0u8; 4]; width * height];
    for j in 0..height {
        for i in 0..width {
            let mut sum = [0.0f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (i as f32 + (sx as f32 + 0.5) / n) / SCALE - foot.x;
                    let y = ((height - j) as f32 - (sy as f32 + 0.5) / n) / SCALE - foot.y;
                    if let Some([r, g, b, a]) = paint(Vec2::new(x, y)) {
                        sum[0] += r * a;
                        sum[1] += g * a;
                        sum[2] += b * a;
                        sum[3] += a;
                    }
                }
            }
            if sum[3] > 0.0 {
                let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                pixels[j * width + i] = [
                    byte(sum[0] / sum[3]),
                    byte(sum[1] / sum[3]),
                    byte(sum[2] / sum[3]),
                    byte(sum[3] / (n * n)),
                ];
            }
        }
    }
    if outline {
        darken_rim(&mut pixels, width, height);
    }
    bleed(&mut pixels, width, height);
    Drawing {
        width,
        height,
        pixels,
        size,
        anchor: Vec2::new(foot.x / size.x - 0.5, foot.y / size.y - 0.5),
    }
}

fn neighbours(i: usize, width: usize, height: usize) -> impl Iterator<Item = usize> {
    let (x, y) = ((i % width) as i64, (i / width) as i64);
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .map(move |(dx, dy)| (x + dx, y + dy))
        .filter(move |&(x, y)| x >= 0 && y >= 0 && x < width as i64 && y < height as i64)
        .map(move |(x, y)| y as usize * width + x as usize)
}

/// A dark rim where a drawing meets the transparent, so small figures read
/// against the ground.
fn darken_rim(pixels: &mut [[u8; 4]], width: usize, height: usize) {
    let rim: Vec<usize> = (0..pixels.len())
        .filter(|&i| pixels[i][3] > 128 && neighbours(i, width, height).any(|j| pixels[j][3] < 64))
        .collect();
    for i in rim {
        for c in &mut pixels[i][..3] {
            *c = (*c as f32 * 0.45) as u8;
        }
    }
}

/// Gives transparent pixels the colour of their opaque neighbours, so a
/// smoothed edge does not fade through black.
fn bleed(pixels: &mut [[u8; 4]], width: usize, height: usize) {
    for _ in 0..2 {
        let copy = pixels.to_vec();
        for i in 0..pixels.len() {
            if copy[i][3] != 0 {
                continue;
            }
            let (mut sum, mut count) = ([0u32; 3], 0);
            for j in neighbours(i, width, height) {
                if copy[j][3] != 0 || copy[j][..3] != [0, 0, 0] {
                    for c in 0..3 {
                        sum[c] += u32::from(copy[j][c]);
                    }
                    count += 1;
                }
            }
            if let (Some(r), Some(g), Some(b)) = (
                sum[0].checked_div(count),
                sum[1].checked_div(count),
                sum[2].checked_div(count),
            ) {
                pixels[i] = [r as u8, g as u8, b as u8, 0];
            }
        }
    }
}

/// Packs the drawings into rows of one atlas.
fn pack(drawings: &[Drawing]) -> (Image, Vec<Piece>) {
    let mut places = Vec::with_capacity(drawings.len());
    let (mut x, mut y, mut row) = (PAD, PAD, 0);
    for d in drawings {
        if x + d.width + PAD > ATLAS_WIDTH {
            x = PAD;
            y += row + PAD;
            row = 0;
        }
        places.push((x, y));
        x += d.width + PAD;
        row = row.max(d.height);
    }
    let height = (y + row + PAD).next_power_of_two();
    let mut image = blank(ATLAS_WIDTH as u32, height as u32);
    image.sampler = ImageSampler::linear();
    if let Some(data) = image.data.as_mut() {
        for (d, &(x0, y0)) in drawings.iter().zip(&places) {
            for j in 0..d.height {
                for i in 0..d.width {
                    let at = ((y0 + j) * ATLAS_WIDTH + x0 + i) * 4;
                    data[at..at + 4].copy_from_slice(&d.pixels[j * d.width + i]);
                }
            }
        }
    }
    let pieces = drawings
        .iter()
        .zip(&places)
        .map(|(d, &(x, y))| Piece {
            rect: Rect::new(
                x as f32,
                y as f32,
                (x + d.width) as f32,
                (y + d.height) as f32,
            ),
            size: d.size,
            anchor: d.anchor,
        })
        .collect();
    (image, pieces)
}

/// Light from the upper left and the viewer.
fn light() -> Vec3 {
    Vec3::new(-0.55, 0.55, 0.63).normalize()
}

/// A soft shadow on the ground round the foot.
fn contact_shadow(p: Vec2, centre: Vec2, radii: Vec2, strength: f32) -> Option<Rgba> {
    let d = ((p - centre) / radii).length_squared();
    (d < 1.0).then_some([0.02, 0.04, 0.02, strength * (1.0 - d)])
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

const BARK: Rgb = [0.32, 0.22, 0.13];

fn bark(p: Vec2, half: f32) -> Option<Rgba> {
    let lit = 0.5 - 0.5 * (p.x / half).clamp(-1.0, 1.0);
    let streak = if (p.y * 2.3 + p.x * 0.7).sin() > 0.6 {
        0.85
    } else {
        1.0
    };
    opaque(scale(BARK, (0.6 + 0.6 * lit) * streak))
}

struct Conifer {
    half: f32,
    height: f32,
    tiers: f32,
    seed: u64,
    dark: Rgb,
    light: Rgb,
}

/// A spruce or fir: tiers of branches round a straight trunk.
fn conifer(p: Vec2, c: &Conifer) -> Option<Rgba> {
    let base = c.height * 0.12;
    if p.y >= base && p.y <= c.height {
        let t = (p.y - base) / (c.height - base);
        let f = (t * c.tiers).fract();
        let side = if p.x < 0.0 { 0.0 } else { 9.0 };
        let jag = 1.0 + 0.3 * (noise(p.y * 1.7, side, c.seed) - 0.5);
        let width = c.half * (1.0 - t).powf(0.92) * (0.55 + 0.45 * (1.0 - f)) * jag + 0.2;
        if p.x.abs() <= width {
            let lit = 0.5 - 0.5 * p.x / width;
            let fine = grain((p.x * SCALE) as i32, (p.y * SCALE) as i32, c.seed) - 0.5;
            let clump = noise(p.x * 1.1, p.y * 1.1, c.seed ^ 3) - 0.5;
            let mut k = 0.1 + 0.62 * lit * (1.0 - 0.5 * f) + 0.22 * fine + 0.3 * clump;
            k -= 0.3 * f * f;
            if width - p.x.abs() < 0.4 {
                k -= 0.12;
            }
            return opaque(mix(c.dark, c.light, k.clamp(0.0, 1.0)));
        }
    }
    if p.x.abs() < 0.75 && (0.0..base + 1.0).contains(&p.y) {
        return bark(p, 0.75);
    }
    contact_shadow(
        p,
        Vec2::new(c.half * 0.25, 0.3),
        Vec2::new(c.half * 0.85, c.half * 0.3),
        0.45,
    )
}

struct Broadleaf {
    blobs: Vec<(Vec2, f32)>,
    height: f32,
    seed: u64,
}

impl Broadleaf {
    fn new(height: f32, seed: u64) -> Self {
        let centre = Vec2::new(0.0, height * 0.62);
        let r = height * 0.28;
        let mut blobs = vec![(centre, r * 0.82)];
        for i in 0..7 {
            let h = |k: u64| grain(i, k as i32, seed);
            let angle = i as f32 * TAU / 7.0 + h(1) * 0.6;
            let at = centre + Vec2::new(angle.cos(), angle.sin() * 0.85) * r * (0.5 + 0.2 * h(2));
            blobs.push((at, r * (0.48 + 0.2 * h(3))));
        }
        Self {
            blobs,
            height,
            seed,
        }
    }
}

const LEAF_DARK: Rgb = [0.1, 0.22, 0.07];
const LEAF_LIGHT: Rgb = [0.55, 0.68, 0.28];

/// A beech or maple: a crown of rounded clumps.
fn broadleaf(p: Vec2, b: &Broadleaf) -> Option<Rgba> {
    let mut best: Option<(f32, Vec3)> = None;
    for &(centre, r) in &b.blobs {
        let d = (p - centre) / r;
        let dd = d.length_squared();
        if dd <= 1.0 {
            let z = (1.0 - dd).sqrt();
            let score = z * r - centre.y * 0.2;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, Vec3::new(d.x, d.y, z)));
            }
        }
    }
    if let Some((_, normal)) = best {
        let leaf = noise(p.x * 1.3, p.y * 1.3, b.seed) - 0.5;
        let fine = grain((p.x * SCALE) as i32, (p.y * SCALE) as i32, b.seed) - 0.5;
        let n = (normal + Vec3::new(leaf, leaf, 0.0) * 0.7).normalize();
        let k = (0.08 + 0.85 * n.dot(light()).max(0.0)) * (0.55 + 0.45 * normal.z) + 0.18 * fine;
        return opaque(mix(LEAF_DARK, LEAF_LIGHT, k.clamp(0.0, 1.0)));
    }
    let top = b.height * 0.55;
    if (0.0..top).contains(&p.y) && p.x.abs() < 1.15 - 0.45 * p.y / top {
        return bark(p, 1.0);
    }
    contact_shadow(p, Vec2::new(2.5, 0.3), Vec2::new(9.0, 3.0), 0.45)
}

/// How a Waldmensch stands in one frame. Points are relative to the feet,
/// facing right.
struct Pose {
    front_foot: Vec2,
    back_foot: Vec2,
    bob: f32,
    front_hand: Vec2,
    back_hand: Vec2,
    /// The axe: grip and the end with the head.
    axe: (Vec2, Vec2),
    /// Below this, the figure is under water or in the boat.
    cut: Option<f32>,
    ripple: Option<f32>,
}

const SHOULDER_FRONT: Vec2 = Vec2::new(0.55, 9.6);
const SHOULDER_BACK: Vec2 = Vec2::new(-0.45, 9.7);

impl Pose {
    fn walking(phase: f32) -> Self {
        let s = phase.sin();
        let bob = 0.3 * phase.cos().abs();
        Self {
            front_foot: Vec2::new(1.3 * s, 0.4),
            back_foot: Vec2::new(-1.3 * s, 0.4),
            bob,
            front_hand: Vec2::new(1.5, 8.0 + bob),
            back_hand: SHOULDER_BACK + Vec2::new(0.9 * s, -2.9 + bob),
            // Carried on the shoulder.
            axe: (Vec2::new(2.0, 7.2 + bob), Vec2::new(-1.6, 12.6 + bob)),
            cut: None,
            ripple: None,
        }
    }

    fn standing() -> Self {
        Self {
            front_foot: Vec2::new(0.6, 0.4),
            back_foot: Vec2::new(-0.6, 0.4),
            ..Self::walking(FRAC_PI_2)
        }
    }

    fn chopping(degrees: f32) -> Self {
        let dir = Vec2::from_angle(degrees.to_radians());
        let hands = SHOULDER_FRONT + dir * 2.7;
        Self {
            front_foot: Vec2::new(1.1, 0.4),
            back_foot: Vec2::new(-1.0, 0.4),
            bob: 0.0,
            front_hand: hands,
            back_hand: hands - dir * 0.8,
            axe: (hands - dir * 1.2, hands + dir * 3.8),
            cut: None,
            ripple: None,
        }
    }
}

const SKIN: Rgb = [0.92, 0.73, 0.57];
const CAP: Rgb = [0.34, 0.23, 0.12];
const TROUSERS: Rgb = [0.3, 0.25, 0.18];
const BOOTS: Rgb = [0.16, 0.11, 0.07];
const BELT: Rgb = [0.26, 0.16, 0.08];
const HANDLE: Rgb = [0.6, 0.42, 0.22];
const METAL: Rgb = [0.66, 0.68, 0.72];
const EDGE: Rgb = [0.93, 0.94, 0.96];

/// The axe's head at the end of the handle, its edge leading the swing.
fn axe_head(p: Vec2, grip: Vec2, end: Vec2) -> Option<Rgb> {
    let d = (end - grip).normalize_or_zero();
    let n = Vec2::new(d.y, -d.x);
    let q = p - end;
    let (u, v) = (q.dot(d), q.dot(n));
    if (-1.1..0.35).contains(&u) && v > -0.5 && v < 1.0 + 0.45 * (u + 1.1) {
        return Some(if v > 0.75 + 0.45 * (u + 1.1) {
            EDGE
        } else {
            METAL
        });
    }
    None
}

fn waldmensch(p: Vec2, pose: &Pose, tunic: Rgb) -> Option<Rgba> {
    if let Some(cut) = pose.cut
        && p.y < cut
    {
        if let Some(level) = pose.ripple {
            let e = (p.x / 3.4).powi(2) + ((p.y - level) / 0.9).powi(2);
            if (e - 1.0).abs() < 0.22 {
                return Some([0.85, 0.93, 0.96, 0.8]);
            }
        }
        return None;
    }
    let bob = Vec2::new(0.0, pose.bob);
    let sleeve = scale(tunic, 0.82);
    // The near arm, in front of everything but the axe's head.
    if let Some(c) = axe_head(p, pose.axe.0, pose.axe.1) {
        return opaque(c);
    }
    if p.distance(pose.front_hand) < 0.55 {
        return opaque(SKIN);
    }
    if distance_to_segment(p, SHOULDER_FRONT + bob, pose.front_hand) < 0.6 {
        return opaque(sleeve);
    }
    if distance_to_segment(p, pose.axe.0, pose.axe.1) < 0.3 {
        return opaque(HANDLE);
    }
    // The head, with a felt cap.
    let head = Vec2::new(0.3, 11.95) + bob;
    let h = p - head;
    if h.length() < 1.45 {
        if h.y > 0.15 || (h.x < -0.6 && h.y > -0.7) {
            return opaque(scale(CAP, 1.0 + 0.25 * (-h.x / 1.45)));
        }
        if (h - Vec2::new(0.85, 0.05)).length() < 0.22 {
            return opaque([0.1, 0.07, 0.05]);
        }
        return opaque(scale(SKIN, 1.0 - 0.15 * (h.x / 1.45).max(0.0)));
    }
    if h.y > 0.6 && h.y < 1.0 && h.x.abs() < 1.95 {
        return opaque(scale(CAP, 0.85));
    }
    // The tunic.
    let y = p.y - pose.bob;
    if (5.0..10.4).contains(&y) {
        let half = 1.85 - (y - 5.0) * 0.07;
        if (p.x - 0.1).abs() < half {
            if (6.2..6.8).contains(&y) {
                return opaque(BELT);
            }
            let lit = 0.5 - 0.5 * (p.x - 0.1) / half;
            return opaque(scale(tunic, 0.72 + 0.4 * lit));
        }
    }
    // Legs: the near one first.
    for (foot, shade) in [(pose.front_foot, 1.0), (pose.back_foot, 0.75)] {
        let hip = Vec2::new(foot.x * 0.25, 5.6) + bob;
        if distance_to_segment(p, hip, foot + Vec2::new(0.0, 0.35)) < 0.72 {
            let c = if p.y < 1.3 { BOOTS } else { TROUSERS };
            return opaque(scale(c, shade));
        }
    }
    if p.distance(pose.back_hand) < 0.5 {
        return opaque(scale(SKIN, 0.8));
    }
    if distance_to_segment(p, SHOULDER_BACK + bob, pose.back_hand) < 0.55 {
        return opaque(scale(sleeve, 0.75));
    }
    if pose.cut.is_none() {
        return contact_shadow(p, Vec2::new(0.4, 0.3), Vec2::new(2.8, 0.9), 0.4);
    }
    None
}

fn poses() -> [Pose; FRAMES] {
    let wade = |k: f32| Pose {
        cut: Some(3.6),
        ripple: Some(3.6 + k * 0.1),
        ..Pose::standing()
    };
    [
        Pose::standing(),
        Pose::walking(0.0),
        Pose::walking(FRAC_PI_2),
        Pose::walking(2.0 * FRAC_PI_2),
        Pose::walking(3.0 * FRAC_PI_2),
        Pose::chopping(150.0),
        Pose::chopping(100.0),
        Pose::chopping(25.0),
        Pose::chopping(-35.0),
        wade(0.0),
        wade(1.0),
        Pose {
            cut: Some(4.6),
            ..Pose::standing()
        },
    ]
}

const WOOD: Rgb = [0.5, 0.33, 0.17];

/// A rowing boat seen from the upper left, with its wake.
fn boat(p: Vec2) -> Option<Rgba> {
    let ellipse = |c: Vec2, r: Vec2| ((p - c) / r).length_squared();
    let inside = ellipse(Vec2::new(0.0, 6.4), Vec2::new(12.0, 2.5));
    if inside < 1.0 {
        let plank = if (p.x * 0.55).fract().abs() < 0.08 {
            0.8
        } else {
            1.0
        };
        return opaque(scale(WOOD, (0.6 + 0.25 * (p.y - 4.0) / 5.0) * plank));
    }
    if ellipse(Vec2::new(0.0, 6.4), Vec2::new(13.2, 3.2)) < 1.0 {
        return opaque(scale(WOOD, 1.3 - 0.15 * p.x / 13.0));
    }
    if ellipse(Vec2::new(0.0, 5.4), Vec2::new(13.6, 4.6)) < 1.0 && p.y <= 6.4 {
        let plank = if (p.y * 1.1).fract() < 0.15 {
            0.75
        } else {
            1.0
        };
        let lit = 0.5 - 0.5 * p.x / 13.6;
        return opaque(scale(WOOD, (0.7 + 0.4 * lit) * plank));
    }
    let wake = ellipse(Vec2::new(0.0, 1.8), Vec2::new(15.5, 2.4));
    if (wake - 1.0).abs() < 0.15 {
        return Some([0.85, 0.93, 0.96, 0.7]);
    }
    None
}

fn glint(p: Vec2) -> Option<Rgba> {
    let d = p.x.abs() / 2.8 + (p.y - 1.2).abs() / 0.7;
    (d < 1.0).then(|| [0.95, 0.98, 1.0, ((1.0 - d) * 2.0).min(1.0)])
}

/// Paints every sprite and packs the atlas.
fn paint() -> (Image, Art) {
    let mut drawings = Vec::new();
    let conifers = [
        (8.5, 36.0, 5.0, [0.04, 0.15, 0.08], [0.26, 0.46, 0.22]),
        (9.5, 40.0, 6.0, [0.04, 0.14, 0.08], [0.24, 0.44, 0.2]),
        (7.5, 33.0, 5.0, [0.05, 0.17, 0.1], [0.3, 0.5, 0.3]),
        (10.0, 38.0, 5.0, [0.05, 0.17, 0.12], [0.29, 0.49, 0.33]),
        (8.0, 30.0, 4.0, [0.04, 0.15, 0.08], [0.28, 0.48, 0.23]),
    ];
    for (i, &(half, height, tiers, dark, light)) in conifers.iter().enumerate() {
        let tree = Conifer {
            half,
            height,
            tiers,
            seed: 100 + i as u64,
            dark,
            light,
        };
        drawings.push(draw(
            Vec2::new(half * 2.0 + 4.0, height + 2.0),
            Vec2::new(half + 2.0, 1.5),
            false,
            |p| conifer(p, &tree),
        ));
    }
    let broadleaves = [30.0, 27.0, 33.0];
    for (i, &height) in broadleaves.iter().enumerate() {
        let tree = Broadleaf::new(height, 200 + i as u64);
        let half = height * 0.45;
        drawings.push(draw(
            Vec2::new(half * 2.0, height + 2.0),
            Vec2::new(half, 1.5),
            false,
            |p| broadleaf(p, &tree),
        ));
    }
    let sapling = Conifer {
        half: 3.0,
        height: 9.0,
        tiers: 3.0,
        seed: 300,
        dark: [0.08, 0.24, 0.1],
        light: [0.4, 0.62, 0.3],
    };
    drawings.push(draw(
        Vec2::new(8.0, 10.5),
        Vec2::new(4.0, 1.0),
        false,
        |p| conifer(p, &sapling),
    ));
    let poses = poses();
    for rival in 0..MAX_RIVALS {
        let tunic = scale(rival_rgb(RivalId(rival as u8)), 0.92);
        for pose in &poses {
            drawings.push(draw(
                Vec2::new(13.0, 18.0),
                Vec2::new(5.5, 1.5),
                true,
                |p| waldmensch(p, pose, tunic),
            ));
        }
    }
    drawings.push(draw(
        Vec2::new(33.0, 11.0),
        Vec2::new(16.5, 0.0),
        false,
        boat,
    ));
    drawings.push(draw(
        Vec2::new(6.5, 2.6),
        Vec2::new(3.25, 0.5),
        false,
        glint,
    ));

    let (image, pieces) = pack(&drawings);
    let trees = conifers.len() + broadleaves.len();
    let figures_from = trees + 1;
    let figures_to = figures_from + MAX_RIVALS * FRAMES;
    let art = Art {
        atlas: Handle::default(),
        trees: pieces[..trees].to_vec(),
        conifers: conifers.len(),
        sapling: pieces[trees],
        figures: pieces[figures_from..figures_to].to_vec(),
        boat: pieces[figures_to],
        glint: pieces[figures_to + 1],
    };
    (image, art)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sprite_is_painted_and_packed_apart() {
        let (image, art) = paint();
        let mut pieces = art.trees.clone();
        pieces.extend([art.sapling, art.boat, art.glint]);
        pieces.extend(art.figures.iter().copied());
        assert_eq!(art.figures.len(), MAX_RIVALS * FRAMES);
        let size = image.size().as_vec2();
        let data = image.data.as_ref().expect("the atlas has pixels");
        for (i, a) in pieces.iter().enumerate() {
            assert!(a.rect.max.x <= size.x && a.rect.max.y <= size.y);
            assert!((-0.5..=0.5).contains(&a.anchor.x) && (-0.5..=0.5).contains(&a.anchor.y));
            let solid = (a.rect.min.y as usize..a.rect.max.y as usize)
                .flat_map(|y| (a.rect.min.x as usize..a.rect.max.x as usize).map(move |x| (x, y)))
                .filter(|&(x, y)| data[(y * size.x as usize + x) * 4 + 3] > 200)
                .count();
            assert!(solid > 3, "piece {i} is empty");
            for b in &pieces[i + 1..] {
                assert!(a.rect.intersect(b.rect).is_empty(), "pieces overlap");
            }
        }
    }

    #[test]
    fn noise_stays_in_range_and_is_continuous() {
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.21);
            let n = noise(x, y, 7);
            assert!((0.0..=1.0).contains(&n));
            assert!((noise(x + 0.001, y, 7) - n).abs() < 0.01);
        }
    }
}
