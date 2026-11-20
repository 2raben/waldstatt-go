//! The ground: the whole board painted from what the player's rival sees, in
//! square chunks of a few tiles laid on the board's plane, so both views draw
//! the very same tiles and a change repaints only its chunk.
//!
//! Unexplored land is black. Land seen before is in fog, with its waters
//! visible; other rivals' trails show only round the player's groups. Forest
//! the player's groups cannot cut, because it lies in another rival's margin,
//! is tinged red, and the tile each trail closes next carries a sapling. When a
//! ring closes, the part it enclosed and the ring flash in the colour of the
//! rival that closed it, also under fog, so it can be read afterwards.
//!
//! Seen from above, forest shows its treetops. In the isometric view the trees
//! stand upright (`scenery.rs`), so the ground shows the forest floor under
//! them. Water is deeper away from the shore, with foam along it and waves;
//! cut tiles show the stumps of the trees that stood there.

use bevy::image::ImageSampler;
use bevy::prelude::*;

use crate::art::{Rgb, mix, noise, scale};
use crate::rules::game::GameEvent;
use crate::rules::rng::tile_hash;
use crate::rules::sight::Sight;
use crate::rules::{Board, Game, Ground, Group, Rival, RivalId, Seat, Tile};
use crate::style::{blank, rival_rgb};
use crate::view::{BoardPlane, BoardRoot, BoardView};

/// Pixels per tile side in the ground's images.
const PIXELS: usize = 16;
/// Tiles per side of a chunk.
const CHUNK: i32 = 16;
/// How long an enclosure flashes, in seconds.
const FLASH_SECONDS: f32 = 6.0;
/// The shore's distance is kept in hundredths of a tile up to this.
const FAR: u8 = 250;

/// The rival whose eyes the board is drawn through; `None` shows everything.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Watcher(pub Option<RivalId>);

/// The ground's chunks and the look each tile was last painted with.
#[derive(Resource)]
pub struct GroundImage {
    chunks: Vec<Handle<Image>>,
    chunks_across: usize,
    looks: Vec<u32>,
    view: BoardView,
    /// For every pixel of the board: how far the nearest tile of the other
    /// kind (land for water, water for land) is, in hundredths of a tile.
    shore: Vec<u8>,
    /// For every pixel of the board: smooth noise, for texture.
    grain: Vec<u8>,
    flashes: Vec<Flash>,
    events_seen: usize,
}

impl GroundImage {
    /// The look each tile is drawn with now, by tile index.
    pub fn looks(&self) -> &[u32] {
        &self.looks
    }
}

struct Flash {
    by: RivalId,
    part: Vec<Tile>,
    ring: Vec<Tile>,
    started: f32,
}

/// Everything drawn for a game; despawned when the next one starts.
#[derive(Component)]
pub struct BoardScene;

/// Where a forest tile's trees stand, in tile coordinates.
pub struct TreeSpot {
    pub at: Vec2,
    pub hash: u64,
    pub broadleaf: bool,
    pub size: f32,
}

/// The trees of a tile: as many as it yields, placed by the seed. The ground
/// draws their tops or stumps here, and the isometric view stands them here.
pub fn tree_spots(seed: u64, tile: Tile, trees: u32) -> Vec<TreeSpot> {
    let pattern: &[(f32, f32)] = match trees {
        0 => &[],
        1 => &[(0.5, 0.55)],
        2 => &[(0.3, 0.4), (0.7, 0.62)],
        _ => &[(0.27, 0.32), (0.73, 0.4), (0.48, 0.76)],
    };
    let hash = tile_hash(seed ^ 0x7ee5, tile.x, tile.y);
    let mirror = hash & 1 == 1;
    pattern
        .iter()
        .enumerate()
        .map(|(k, &(x, y))| {
            let h = crate::rules::rng::mix(hash.wrapping_add(k as u64));
            let jitter = |shift: u32| ((h >> shift) & 0xFF) as f32 / 255.0 * 0.14 - 0.07;
            let x = if mirror { 1.0 - x } else { x };
            TreeSpot {
                at: Vec2::new(tile.x as f32 + x + jitter(0), tile.y as f32 + y + jitter(8)),
                hash: h,
                broadleaf: (h >> 16) % 10 >= 7,
                size: 0.85 + ((h >> 24) & 0xFF) as f32 / 255.0 * 0.3,
            }
        })
        .collect()
}

/// Spawns the drawing of the game that was just started: the board's root
/// and plane in the current view, and the ground's chunks on the plane.
pub fn spawn_scene(world: &mut World) {
    let old: Vec<Entity> = world
        .query_filtered::<Entity, With<BoardScene>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    let watcher = world
        .query::<&Rival>()
        .iter(world)
        .find(|r| r.seat == Seat::Player)
        .map(|r| r.id);
    world.insert_resource(Watcher(watcher));
    let (shore, grain, width, height, len) = {
        let board = world.resource::<Board>();
        let seed = world.resource::<Game>().seed;
        (
            shore_distances(board),
            grains(board, seed),
            board.width,
            board.height,
            board.len(),
        )
    };
    let across = (width + CHUNK - 1) / CHUNK;
    let down = (height + CHUNK - 1) / CHUNK;
    let mut chunks = Vec::new();
    let mut sprites = Vec::new();
    for cy in 0..down {
        for cx in 0..across {
            let (x0, y0) = (cx * CHUNK, cy * CHUNK);
            let size = Vec2::new(
                (width - x0).min(CHUNK) as f32,
                (height - y0).min(CHUNK) as f32,
            );
            let mut image = blank(size.x as u32 * PIXELS as u32, size.y as u32 * PIXELS as u32);
            image.sampler = ImageSampler::linear();
            let image = world.resource_mut::<Assets<Image>>().add(image);
            chunks.push(image.clone());
            sprites.push((
                Name::new("Ground"),
                Sprite {
                    image,
                    custom_size: Some(size),
                    ..default()
                },
                Transform::from_xyz(x0 as f32 + size.x / 2.0, -(y0 as f32 + size.y / 2.0), 0.0),
            ));
        }
    }
    let view = *world.resource::<BoardView>();
    let (root, plane) = view.transforms();
    world
        .spawn((
            Name::new("Board"),
            BoardScene,
            BoardRoot,
            root,
            Visibility::default(),
        ))
        .with_children(|root| {
            root.spawn((Name::new("Plane"), BoardPlane, plane, Visibility::default()))
                .with_children(|plane| {
                    for sprite in sprites {
                        plane.spawn(sprite);
                    }
                });
        });
    world.insert_resource(GroundImage {
        chunks,
        chunks_across: across as usize,
        looks: vec![u32::MAX; len],
        view,
        shore,
        grain,
        flashes: Vec::new(),
        events_seen: 0,
    });
}

/// How far each pixel lies from the nearest tile of the other kind, looking
/// two tiles round.
fn shore_distances(board: &Board) -> Vec<u8> {
    let stride = board.width as usize * PIXELS;
    let mut out = vec![FAR; stride * board.height as usize * PIXELS];
    for tile in board.tiles() {
        let water = board.is_water(tile);
        let others: Vec<Tile> = (-2..=2)
            .flat_map(|dy| (-2..=2).map(move |dx| Tile::new(tile.x + dx, tile.y + dy)))
            .filter(|&t| board.contains(t) && board.is_water(t) != water)
            .collect();
        if others.is_empty() {
            continue;
        }
        for py in 0..PIXELS {
            for px in 0..PIXELS {
                let p = Vec2::new(
                    tile.x as f32 + (px as f32 + 0.5) / PIXELS as f32,
                    tile.y as f32 + (py as f32 + 0.5) / PIXELS as f32,
                );
                let d = others
                    .iter()
                    .map(|t| {
                        let dx = (t.x as f32 - p.x).max(p.x - (t.x + 1) as f32).max(0.0);
                        let dy = (t.y as f32 - p.y).max(p.y - (t.y + 1) as f32).max(0.0);
                        dx.hypot(dy)
                    })
                    .fold(f32::MAX, f32::min);
                let i = (tile.y as usize * PIXELS + py) * stride + tile.x as usize * PIXELS + px;
                out[i] = (d * 100.0).min(FAR as f32) as u8;
            }
        }
    }
    out
}

fn grains(board: &Board, seed: u64) -> Vec<u8> {
    let stride = board.width as usize * PIXELS;
    let height = board.height as usize * PIXELS;
    let mut out = Vec::with_capacity(stride * height);
    for y in 0..height {
        for x in 0..stride {
            let (x, y) = (x as f32, y as f32);
            let g = 0.6 * noise(x / 7.0, y / 7.0, seed) + 0.4 * noise(x / 2.5, y / 2.5, seed ^ 1);
            out.push((g * 255.0) as u8);
        }
    }
    out
}

// A tile's look: its kind in the low byte, then flags.
const UNKNOWN: u32 = 0;
const FOG_LAND: u32 = 1;
const FOG_RIVER: u32 = 2;
const FOG_LAKE: u32 = 3;
const FOREST: u32 = 4;
const RIVER: u32 = 5;
const LAKE: u32 = 6;
const OPEN: u32 = 8; // + rival
const MARGIN: u32 = 1 << 8;
const CLOSES_NEXT: u32 = 1 << 9;
const FLASH_PART: u32 = 1 << 10;
const FLASH_RING: u32 = 1 << 11;
const FLASH_SHIFT: u32 = 12; // + rival << 12, level << 14

/// What stands on a tile in the isometric view.
pub enum Cover {
    Bare,
    /// Its trees, tinted.
    Trees(Color),
    /// The sapling of a tile that grows back next.
    Sapling,
    /// Open water in sight.
    Water,
}

/// Whether a tile is open ground in sight, which trees in front of it
/// should not hide.
pub fn is_clearing(look: u32) -> bool {
    look & 0xFF >= OPEN
}

pub fn cover(look: u32) -> Cover {
    let kind = look & 0xFF;
    match kind {
        RIVER | LAKE => Cover::Water,
        FOREST | FOG_LAND => {
            let mut c: Rgb = if kind == FOG_LAND {
                [0.36, 0.38, 0.4]
            } else {
                [1.0, 1.0, 1.0]
            };
            if look & MARGIN != 0 {
                c = mix(c, [1.0, 0.55, 0.5], 0.6);
            }
            if look & FLASH_PART != 0 {
                let level = ((look >> (FLASH_SHIFT + 2)) & 0x3) as f32 / 3.0;
                let by = RivalId(((look >> FLASH_SHIFT) & 0x3) as u8);
                c = mix(c, rival_rgb(by), 0.3 + 0.4 * level);
            }
            Cover::Trees(Color::srgb(c[0], c[1], c[2]))
        }
        k if k >= OPEN && look & CLOSES_NEXT != 0 => Cover::Sapling,
        _ => Cover::Bare,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn paint_ground(
    time: Res<Time<Real>>,
    view: Res<BoardView>,
    board: Option<Res<Board>>,
    game: Option<Res<Game>>,
    watcher: Res<Watcher>,
    ground: Option<ResMut<GroundImage>>,
    mut images: ResMut<Assets<Image>>,
    sights: Query<(&Rival, &Sight)>,
    groups: Query<&Group>,
) {
    let (Some(board), Some(game), Some(mut ground)) = (board, game, ground) else {
        return;
    };
    let now = time.elapsed_secs();
    // New enclosures start flashing.
    for event in &game.events[ground.events_seen.min(game.events.len())..] {
        if let GameEvent::Enclosed { by, part, ring, .. } = event {
            ground.flashes.push(Flash {
                by: *by,
                part: part.clone(),
                ring: ring.clone(),
                started: now,
            });
        }
    }
    ground.events_seen = game.events.len();
    ground.flashes.retain(|f| now - f.started < FLASH_SECONDS);
    if ground.view != *view {
        ground.view = *view;
        ground.looks.fill(u32::MAX);
    }

    let sight = watcher
        .0
        .and_then(|id| sights.iter().find(|(r, _)| r.id == id).map(|(_, s)| s));
    let sees = |t: Tile| sight.is_none_or(|s| s.sees(&board, t));
    let has_seen = |t: Tile| sight.is_none_or(|s| s.has_seen(&board, t));
    let mut looks: Vec<u32> = board
        .tiles()
        .map(|tile| look(&board, tile, watcher.0, sees(tile), has_seen(tile)))
        .collect();
    for group in &groups {
        if Some(group.rival) == watcher.0
            && let Some(tile) = group.regrows
            && let Some(i) = board.index(tile)
        {
            looks[i] |= CLOSES_NEXT;
        }
    }
    for flash in &ground.flashes {
        let age = now - flash.started;
        // Pulses, fading towards the end.
        let level = ((1.0 - age / FLASH_SECONDS) * (0.6 + 0.4 * (age * 6.0).cos()) * 3.0).round();
        let level = level.clamp(0.0, 3.0) as u32;
        if level == 0 {
            continue;
        }
        let tint = (u32::from(flash.by.0) << FLASH_SHIFT) | (level << (FLASH_SHIFT + 2));
        for (tiles, flag) in [(&flash.part, FLASH_PART), (&flash.ring, FLASH_RING)] {
            for &tile in tiles {
                if let Some(i) = board.index(tile) {
                    looks[i] = (looks[i] & !(0xF << FLASH_SHIFT)) | flag | tint;
                }
            }
        }
    }

    // The changed tiles, chunk by chunk.
    let mut dirty: Vec<Vec<usize>> = vec![Vec::new(); ground.chunks.len()];
    for (i, &look) in looks.iter().enumerate() {
        if look != ground.looks[i] {
            let tile = board.tile_at(i);
            let chunk =
                (tile.y / CHUNK) as usize * ground.chunks_across + (tile.x / CHUNK) as usize;
            dirty[chunk].push(i);
        }
    }
    let painter = Painter {
        board: &board,
        seed: game.seed,
        iso: *view == BoardView::Isometric,
        shore: &ground.shore,
        grain: &ground.grain,
        stride: board.width as usize * PIXELS,
    };
    for (chunk, tiles) in dirty.iter().enumerate() {
        if tiles.is_empty() {
            continue;
        }
        let Some(mut image) = images.get_mut(&ground.chunks[chunk]) else {
            continue;
        };
        let width = image.width() as usize;
        let Some(data) = image.data.as_mut() else {
            continue;
        };
        for &i in tiles {
            painter.paint_tile(data, width, board.tile_at(i), looks[i]);
        }
    }
    ground.looks = looks;
}

fn look(board: &Board, tile: Tile, watcher: Option<RivalId>, sees: bool, has_seen: bool) -> u32 {
    let own = watcher.is_some() && board.open(tile) == watcher;
    if !sees && !own {
        if !has_seen {
            return UNKNOWN;
        }
        return match board.ground(tile) {
            Ground::Land => FOG_LAND,
            Ground::River => FOG_RIVER,
            Ground::Lake(_) => FOG_LAKE,
        };
    }
    match board.ground(tile) {
        Ground::River => RIVER,
        Ground::Lake(_) => LAKE,
        Ground::Land => match board.open(tile) {
            Some(rival) => OPEN + u32::from(rival.0),
            None => {
                let margin = watcher.is_some_and(|w| {
                    std::iter::once(tile)
                        .chain(tile.neighbours8())
                        .any(|t| board.open(t).is_some_and(|r| r != w))
                });
                FOREST | if margin { MARGIN } else { 0 }
            }
        },
    }
}

const BLACK: Rgb = [0.04, 0.05, 0.04];
const FLOOR_DARK: Rgb = [0.1, 0.15, 0.07];
const FLOOR_LIGHT: Rgb = [0.25, 0.29, 0.12];
const NEEDLES: Rgb = [0.4, 0.28, 0.13];
const CANOPY_DARK: Rgb = [0.05, 0.16, 0.08];
const CANOPY_LIGHT: Rgb = [0.3, 0.5, 0.25];
const LEAVES_LIGHT: Rgb = [0.5, 0.64, 0.27];
const EARTH: Rgb = [0.48, 0.39, 0.24];
const GRASS: Rgb = [0.45, 0.53, 0.26];
const WOOD: Rgb = [0.78, 0.63, 0.4];
const STUMP_BARK: Rgb = [0.3, 0.2, 0.1];
const BANK: Rgb = [0.66, 0.6, 0.44];
const LAKE_SHALLOW: Rgb = [0.27, 0.56, 0.58];
const LAKE_DEEP: Rgb = [0.06, 0.2, 0.38];
const RIVER_SHALLOW: Rgb = [0.32, 0.6, 0.63];
const RIVER_DEEP: Rgb = [0.13, 0.36, 0.55];
const FOAM: Rgb = [0.86, 0.93, 0.93];
const RED: Rgb = [0.85, 0.2, 0.15];
const WHITE: Rgb = [1.0, 1.0, 1.0];

/// Paints tiles into a chunk.
struct Painter<'a> {
    board: &'a Board,
    seed: u64,
    iso: bool,
    shore: &'a [u8],
    grain: &'a [u8],
    /// Pixels across the whole board.
    stride: usize,
}

impl Painter<'_> {
    fn paint_tile(&self, data: &mut [u8], width: usize, tile: Tile, look: u32) {
        let kind = look & 0xFF;
        let fog = matches!(kind, FOG_LAND | FOG_RIVER | FOG_LAKE);
        let flash_rival = RivalId(((look >> FLASH_SHIFT) & 0x3) as u8);
        let flash_level = ((look >> (FLASH_SHIFT + 2)) & 0x3) as f32 / 3.0;
        let land = matches!(kind, FOG_LAND | FOREST) || kind >= OPEN;
        let spots = if land {
            tree_spots(self.seed, tile, self.board.trees(tile))
        } else {
            Vec::new()
        };
        let (cx, cy) = (
            (tile.x % CHUNK) as usize * PIXELS,
            (tile.y % CHUNK) as usize * PIXELS,
        );
        for py in 0..PIXELS {
            for px in 0..PIXELS {
                let gx = tile.x as usize * PIXELS + px;
                let gy = tile.y as usize * PIXELS + py;
                let g = self.grain[gy * self.stride + gx] as f32 / 255.0;
                let shore = self.shore[gy * self.stride + gx] as f32 / 100.0;
                let at = Vec2::new(
                    tile.x as f32 + (px as f32 + 0.5) / PIXELS as f32,
                    tile.y as f32 + (py as f32 + 0.5) / PIXELS as f32,
                );
                let mut c = match kind {
                    UNKNOWN => BLACK,
                    RIVER | FOG_RIVER => water(true, shore, g, gx, gy),
                    LAKE | FOG_LAKE => water(false, shore, g, gx, gy),
                    FOREST | FOG_LAND if self.iso => bank(self.floor(g, gx, gy), shore),
                    FOREST | FOG_LAND => self.canopy(at, &spots, g, shore),
                    _ => {
                        let rival = RivalId((kind - OPEN) as u8);
                        let mut c = bank(mix(EARTH, GRASS, g), shore);
                        c = mix(c, rival_rgb(rival), 0.38);
                        if let Some(stump) = stump(at, &spots) {
                            c = stump;
                        }
                        if !self.iso && (px == 0 || py == 0) {
                            c = scale(c, 0.85);
                        }
                        c
                    }
                };
                if look & MARGIN != 0 {
                    c = mix(c, RED, 0.28);
                }
                if !self.iso && look & CLOSES_NEXT != 0 && at.distance(tile_centre(tile)) < 0.28 {
                    c = CANOPY_DARK;
                }
                if fog {
                    c = mix(scale(c, 0.45), BLACK, 0.2);
                }
                if look & FLASH_PART != 0 {
                    c = mix(c, rival_rgb(flash_rival), 0.25 + 0.4 * flash_level);
                }
                if look & FLASH_RING != 0 {
                    c = mix(rival_rgb(flash_rival), WHITE, 0.6 * flash_level);
                }
                let i = ((cy + py) * width + cx + px) * 4;
                let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
                data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
    }

    /// Moss and needles under the trees.
    fn floor(&self, g: f32, gx: usize, gy: usize) -> Rgb {
        let fleck = tile_hash(self.seed ^ 0xf100, gx as i32, gy as i32) & 0xFF;
        match fleck {
            0..12 => NEEDLES,
            250.. => scale(FLOOR_LIGHT, 1.3),
            _ => mix(FLOOR_DARK, FLOOR_LIGHT, g),
        }
    }

    /// Treetops seen from above, lit from the north-west, with their shadows.
    fn canopy(&self, at: Vec2, spots: &[TreeSpot], g: f32, shore: f32) -> Rgb {
        let mut best: Option<(f32, Rgb)> = None;
        let mut shade = 1.0;
        for spot in spots {
            let r = 0.27 * spot.size;
            let d = (at - spot.at) / r;
            if (at - spot.at - Vec2::splat(0.08)).length() < r {
                shade = 0.55;
            }
            let dd = d.length_squared();
            if dd < 1.0 {
                let z = (1.0 - dd).sqrt();
                let wobble = noise(at.x * 14.0, at.y * 14.0, spot.hash) - 0.5;
                let normal = Vec3::new(d.x + wobble * 0.5, d.y + wobble * 0.5, z).normalize();
                let lit = normal.dot(Vec3::new(-0.5, -0.5, 0.7).normalize()).max(0.0);
                let light = if spot.broadleaf {
                    LEAVES_LIGHT
                } else {
                    CANOPY_LIGHT
                };
                let colour = mix(
                    CANOPY_DARK,
                    light,
                    (0.1 + 0.8 * lit + 0.15 * (g - 0.5)).clamp(0.0, 1.0),
                );
                let height = z * r - spot.at.y * 0.01;
                if best.is_none_or(|(h, _)| height > h) {
                    best = Some((height, colour));
                }
            }
        }
        match best {
            Some((_, colour)) => colour,
            None => scale(bank(mix(FLOOR_DARK, FLOOR_LIGHT, g), shore), shade),
        }
    }
}

fn tile_centre(tile: Tile) -> Vec2 {
    Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5)
}

/// Sand and mud where land meets water.
fn bank(c: Rgb, shore: f32) -> Rgb {
    const WIDTH: f32 = 0.16;
    if shore < WIDTH {
        mix(c, BANK, 0.85 * (1.0 - shore / WIDTH))
    } else {
        c
    }
}

/// The stump of a felled tree, if `at` is on one.
fn stump(at: Vec2, spots: &[TreeSpot]) -> Option<Rgb> {
    spots.iter().find_map(|spot| {
        let d = at.distance(spot.at);
        let colour = if d < 0.055 * spot.size {
            WOOD
        } else {
            STUMP_BARK
        };
        (d < 0.08 * spot.size).then_some(colour)
    })
}

/// Water: shallow and light along the shore, deeper away from it, with foam
/// at the shore and waves.
fn water(river: bool, shore: f32, g: f32, gx: usize, gy: usize) -> Rgb {
    let (x, y) = (gx as f32, gy as f32);
    let mut c = if river {
        mix(RIVER_SHALLOW, RIVER_DEEP, (shore / 0.45).min(1.0))
    } else {
        mix(LAKE_SHALLOW, LAKE_DEEP, (shore / 1.8).min(1.0).powf(0.7))
    };
    let wave = (x * 0.42 + (y * 0.17).sin() * 2.4 + g * 5.0).sin();
    if wave > 0.88 {
        c = mix(c, FOAM, 0.22 * (wave - 0.88) / 0.12);
    }
    c = scale(c, 0.9 + 0.2 * g);
    if shore < 0.07 {
        c = mix(c, FOAM, 0.6 * (1.0 - shore / 0.07));
    }
    c
}

pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Watcher>()
            .add_systems(PostUpdate, paint_ground);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_has_as_many_trees_as_it_yields_and_keeps_them_inside() {
        for trees in 0..=3 {
            for (x, y) in [(0, 0), (7, 3), (42, 99)] {
                let tile = Tile::new(x, y);
                let spots = tree_spots(9, tile, trees);
                assert_eq!(spots.len(), trees as usize);
                for spot in spots {
                    let inside = spot.at - Vec2::new(x as f32, y as f32);
                    assert!((0.0..1.0).contains(&inside.x) && (0.0..1.0).contains(&inside.y));
                }
            }
        }
    }

    #[test]
    fn the_shore_is_measured_from_the_other_kind() {
        let board = Board::from_rows(&["....", ".~~.", "....", "...."]);
        let shore = shore_distances(&board);
        let stride = 4 * PIXELS;
        let at = |x: f32, y: f32| {
            shore[(y * PIXELS as f32) as usize * stride + (x * PIXELS as f32) as usize]
        };
        // Land touching the river, the river's middle, and land far from it.
        assert!(at(0.97, 1.5) < 10);
        assert!(at(1.5, 1.5) >= 45);
        assert!(at(3.5, 3.9) > 150);
    }
}
