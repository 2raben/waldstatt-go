//! The ground: one image of the whole board, a few pixels per tile, painted
//! from what the player's rival sees and laid on the board's plane, so both
//! views draw the very same tiles.
//!
//! Unexplored land is black. Land seen before is in fog, with its waters
//! visible; other rivals' trails show only round the player's groups. Forest
//! the player's groups cannot cut, because it lies in another rival's margin,
//! is tinged red, and the tile each trail closes next carries a sapling. When a
//! ring closes, the part it enclosed and the ring flash in the colour of the
//! rival that closed it, also under fog, so it can be read afterwards.

use bevy::prelude::*;

use crate::rules::game::GameEvent;
use crate::rules::rng::tile_hash;
use crate::rules::sight::Sight;
use crate::rules::{Board, Game, Ground, Group, Rival, RivalId, Seat, Tile, Trail};
use crate::style::{blank, rival_rgb};
use crate::view::{BoardPlane, BoardRoot, BoardView};

/// Pixels per tile side in the ground image.
const PIXELS: u32 = 6;
/// How long an enclosure flashes, in seconds.
const FLASH_SECONDS: f32 = 6.0;

/// The rival whose eyes the board is drawn through; `None` shows everything.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Watcher(pub Option<RivalId>);

/// The ground image and the look each tile was last painted with.
#[derive(Resource)]
pub struct GroundImage {
    image: Handle<Image>,
    looks: Vec<u32>,
    flashes: Vec<Flash>,
    events_seen: usize,
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

/// Spawns the drawing of the game that was just started: the board's root
/// and plane in the current view, and the ground image on the plane.
pub fn spawn_scene(world: &mut World) {
    let old: Vec<Entity> = world
        .query_filtered::<Entity, With<BoardScene>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    let (width, height, len) = {
        let board = world.resource::<Board>();
        (board.width, board.height, board.len())
    };
    let watcher = world
        .query::<&Rival>()
        .iter(world)
        .find(|r| r.seat == Seat::Player)
        .map(|r| r.id);
    world.insert_resource(Watcher(watcher));
    let image = world
        .resource_mut::<Assets<Image>>()
        .add(blank(width as u32 * PIXELS, height as u32 * PIXELS));
    let (root, plane) = world.resource::<BoardView>().transforms();
    let size = Vec2::new(width as f32, height as f32);
    world.spawn((
        Name::new("Board"),
        BoardScene,
        BoardRoot,
        root,
        Visibility::default(),
        children![(
            Name::new("Plane"),
            BoardPlane,
            plane,
            Visibility::default(),
            children![(
                Name::new("Ground"),
                Sprite {
                    image: image.clone(),
                    custom_size: Some(size),
                    ..default()
                },
                Transform::from_xyz(size.x / 2.0, -size.y / 2.0, 0.0),
            )],
        )],
    ));
    world.insert_resource(GroundImage {
        image,
        looks: vec![u32::MAX; len],
        flashes: Vec::new(),
        events_seen: 0,
    });
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

#[allow(clippy::too_many_arguments)]
pub fn paint_ground(
    time: Res<Time<Real>>,
    board: Option<Res<Board>>,
    game: Option<Res<Game>>,
    watcher: Res<Watcher>,
    ground: Option<ResMut<GroundImage>>,
    mut images: ResMut<Assets<Image>>,
    sights: Query<(&Rival, &Sight)>,
    groups: Query<(&Group, &Trail)>,
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

    let sight = watcher
        .0
        .and_then(|id| sights.iter().find(|(r, _)| r.id == id).map(|(_, s)| s));
    let sees = |t: Tile| sight.is_none_or(|s| s.sees(&board, t));
    let has_seen = |t: Tile| sight.is_none_or(|s| s.has_seen(&board, t));
    let mut looks: Vec<u32> = board
        .tiles()
        .map(|tile| look(&board, tile, watcher.0, sees(tile), has_seen(tile)))
        .collect();
    for (group, trail) in &groups {
        if Some(group.rival) == watcher.0
            && let Some(tile) = trail.closes_next(group.size)
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

    let changed: Vec<usize> = (0..looks.len())
        .filter(|&i| looks[i] != ground.looks[i])
        .collect();
    if changed.is_empty() {
        return;
    }
    let Some(mut image) = images.get_mut(&ground.image) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let stride = (board.width as u32 * PIXELS) as usize;
    for i in changed {
        let tile = board.tile_at(i);
        paint_tile(data, stride, tile, looks[i], game.seed);
        ground.looks[i] = looks[i];
    }
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

type Rgb = [f32; 3];

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn scale(a: Rgb, f: f32) -> Rgb {
    a.map(|c| (c * f).min(1.0))
}

const BLACK: Rgb = [0.04, 0.05, 0.04];
const FOREST_RGB: Rgb = [0.17, 0.34, 0.17];
const TREE_DARK: Rgb = [0.1, 0.23, 0.11];
const TREE_LIGHT: Rgb = [0.27, 0.48, 0.24];
const RIVER_RGB: Rgb = [0.24, 0.5, 0.78];
const LAKE_RGB: Rgb = [0.19, 0.42, 0.7];
const SAND: Rgb = [0.8, 0.73, 0.56];
const RED: Rgb = [0.85, 0.2, 0.15];
const WHITE: Rgb = [1.0, 1.0, 1.0];

/// Paints one tile into the image.
fn paint_tile(data: &mut [u8], stride: usize, tile: Tile, look: u32, seed: u64) {
    let kind = look & 0xFF;
    let hash = tile_hash(seed ^ 0x5eed, tile.x, tile.y);
    let fog = matches!(kind, FOG_LAND | FOG_RIVER | FOG_LAKE);
    let base: Rgb = match kind {
        UNKNOWN => BLACK,
        FOG_LAND | FOREST => FOREST_RGB,
        FOG_RIVER | RIVER => RIVER_RGB,
        FOG_LAKE | LAKE => LAKE_RGB,
        _ => mix(SAND, rival_rgb(RivalId((kind - OPEN) as u8)), 0.45),
    };
    let flash_rival = RivalId(((look >> FLASH_SHIFT) & 0x3) as u8);
    let flash_level = ((look >> (FLASH_SHIFT + 2)) & 0x3) as f32 / 3.0;
    let n = PIXELS as usize;
    for py in 0..n {
        for px in 0..n {
            let mut c = base;
            match kind {
                FOG_LAND | FOREST => {
                    // Two treetops per tile, placed by the tile's hash.
                    for k in 0..2 {
                        let tx = 1 + ((hash >> (k * 8)) % (n as u64 - 2)) as usize;
                        let ty = 1 + ((hash >> (k * 8 + 4)) % (n as u64 - 2)) as usize;
                        let (dx, dy) = (px.abs_diff(tx), py.abs_diff(ty));
                        if dx + dy <= 1 {
                            c = if dx == 0 && dy == 0 {
                                TREE_LIGHT
                            } else {
                                TREE_DARK
                            };
                        }
                    }
                }
                RIVER | LAKE | FOG_RIVER | FOG_LAKE => {
                    if (px + py * 3 + (hash as usize % 5)).is_multiple_of(7) {
                        c = scale(c, 1.15);
                    }
                }
                k if k >= OPEN && (px == 0 || py == 0) => c = scale(c, 0.85),
                _ => {}
            }
            if look & MARGIN != 0 {
                c = mix(c, RED, 0.28);
            }
            if look & CLOSES_NEXT != 0 && (2..n - 2).contains(&px) && (2..n - 2).contains(&py) {
                c = TREE_DARK;
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
            let x = tile.x as usize * n + px;
            let y = tile.y as usize * n + py;
            let i = (y * stride + x) * 4;
            let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Watcher>()
            .add_systems(PostUpdate, paint_ground);
    }
}
