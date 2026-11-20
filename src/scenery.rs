//! The isometric scenery: a tree standing for every tree a forest tile
//! yields, the sapling on the tile each of the player's trails closes next,
//! and glints on the water in sight.
//!
//! Trees are planted once per game where the ground draws their tops and
//! stumps ([`tree_spots`]), sorted by depth with the Waldleute, and shown only
//! in the isometric view. A tree follows its tile's look: hidden when the
//! tile is unexplored or cut, dimmed in fog, reddened in another rival's
//! margin, and tinted while an enclosure flashes. Trees standing in front of
//! open ground in sight turn see-through, so trails and the Waldleute on them
//! stay visible.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::art::Art;
use crate::ground::{BoardScene, Cover, GroundImage, cover, is_clearing, paint_ground, tree_spots};
use crate::rules::rng::tile_hash;
use crate::rules::{Board, Game, Ground, Tile};
use crate::view::{BoardPlane, BoardView, Z_GLINT, Z_UPRIGHT};

/// Trees are drawn this much smaller than painted.
const TREE_SCALE: f32 = 0.8;
/// How opaque a tree in front of open ground is.
const SEE_THROUGH: f32 = 0.38;
/// The tiles behind a tree that it hides in the isometric view, where north
/// is up: the two north of it and the one two rows up.
fn behind(tile: Tile) -> [Tile; 3] {
    let (north_west, north_east) = (tile.go(5, 1), tile.go(0, 1));
    [north_west, north_east, north_west.go(0, 1)]
}
/// Marks a look whose trees are see-through.
const FADED: u32 = 1 << 30;

#[derive(Component)]
struct Tree {
    piece: usize,
    size: f32,
}

#[derive(Component)]
struct Glint {
    tile: usize,
    phase: f32,
    rate: f32,
}

/// The trees of each tile and the look they were last shown with.
#[derive(Resource, Default)]
struct Grove {
    /// `trees[first[i]..first[i + 1]]` stand on tile `i`.
    first: Vec<usize>,
    trees: Vec<Entity>,
    looks: Vec<u32>,
    view: Option<BoardView>,
}

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Grove>()
            .add_systems(Update, plant)
            .add_systems(PostUpdate, (tend.after(paint_ground), twinkle));
    }
}

/// Plants the trees and the glints of a new board.
fn plant(
    mut commands: Commands,
    art: Res<Art>,
    board: Option<Res<Board>>,
    game: Option<Res<Game>>,
    planes: Query<(), Added<BoardPlane>>,
) {
    let (false, Some(board), Some(game)) = (planes.is_empty(), board, game) else {
        return;
    };
    let iso = BoardView::Isometric;
    let mut grove = Grove {
        first: Vec::with_capacity(board.len() + 1),
        looks: vec![u32::MAX; board.len()],
        ..default()
    };
    for (i, tile) in board.tiles().enumerate() {
        grove.first.push(grove.trees.len());
        match board.ground(tile) {
            Ground::Land => {
                for spot in tree_spots(game.seed, tile, board.trees(tile)) {
                    let piece = if spot.broadleaf {
                        art.conifers + (spot.hash >> 32) as usize % (art.trees.len() - art.conifers)
                    } else {
                        (spot.hash >> 32) as usize % art.conifers
                    };
                    let size = spot.size * TREE_SCALE;
                    let (sprite, anchor) = art.sprite(art.trees[piece], size);
                    let entity = commands
                        .spawn((
                            BoardScene,
                            Tree { piece, size },
                            sprite,
                            anchor,
                            Transform::from_translation(
                                iso.project(spot.at).extend(Z_UPRIGHT + iso.depth(spot.at)),
                            ),
                            Visibility::Hidden,
                        ))
                        .id();
                    grove.trees.push(entity);
                }
            }
            Ground::River | Ground::Lake(_) => {
                let hash = tile_hash(game.seed ^ 0x611d, tile.x, tile.y);
                if !hash.is_multiple_of(3) {
                    continue;
                }
                let unit = |shift: u32| ((hash >> shift) & 0xFF) as f32 / 255.0;
                let at = tile.centre() + Vec2::new(unit(8) - 0.5, unit(16) - 0.5) * 0.6;
                let (sprite, anchor) = art.sprite(art.glint, 0.7 + 0.6 * unit(24));
                commands.spawn((
                    BoardScene,
                    Glint {
                        tile: i,
                        phase: unit(32) * std::f32::consts::TAU,
                        rate: 1.2 + 1.6 * unit(40),
                    },
                    sprite,
                    anchor,
                    Transform::from_translation(iso.project(at).extend(Z_GLINT)),
                    Visibility::Hidden,
                ));
            }
        }
    }
    grove.first.push(grove.trees.len());
    commands.insert_resource(grove);
}

/// Shows each tile's trees as its look says.
fn tend(
    view: Res<BoardView>,
    art: Res<Art>,
    board: Option<Res<Board>>,
    ground: Option<Res<GroundImage>>,
    mut grove: ResMut<Grove>,
    mut trees: Query<(&Tree, &mut Sprite, &mut Anchor, &mut Visibility)>,
) {
    let (Some(ground), Some(board)) = (ground, board) else {
        return;
    };
    let looks = ground.looks();
    if looks.len() != grove.looks.len() || looks.len() != board.len() {
        return;
    }
    let all = grove.view != Some(*view);
    grove.view = Some(*view);
    let iso = *view == BoardView::Isometric;
    for (i, &look) in looks.iter().enumerate() {
        let tile = board.tile_at(i);
        let faded = behind(tile)
            .into_iter()
            .any(|t| board.index(t).is_some_and(|j| is_clearing(looks[j])));
        let key = look | if faded { FADED } else { 0 };
        if !all && grove.looks[i] == key {
            continue;
        }
        grove.looks[i] = key;
        let cover = if iso { cover(look) } else { Cover::Bare };
        for (k, &entity) in grove.trees[grove.first[i]..grove.first[i + 1]]
            .iter()
            .enumerate()
        {
            let Ok((tree, mut sprite, mut anchor, mut visibility)) = trees.get_mut(entity) else {
                continue;
            };
            let (piece, size, colour) = match cover {
                Cover::Trees(colour) if faded => (
                    art.trees[tree.piece],
                    tree.size,
                    colour.with_alpha(SEE_THROUGH),
                ),
                Cover::Trees(colour) => (art.trees[tree.piece], tree.size, colour),
                Cover::Sapling if k == 0 => (art.sapling, 1.0, Color::WHITE),
                _ => {
                    *visibility = Visibility::Hidden;
                    continue;
                }
            };
            sprite.rect = Some(piece.rect);
            sprite.custom_size = Some(piece.size * size);
            sprite.color = colour;
            anchor.0 = piece.anchor;
            *visibility = Visibility::Inherited;
        }
    }
}

/// Glints come and go on the water in sight.
fn twinkle(
    time: Res<Time>,
    view: Res<BoardView>,
    ground: Option<Res<GroundImage>>,
    mut glints: Query<(&Glint, &mut Sprite, &mut Visibility)>,
) {
    let Some(ground) = ground else {
        return;
    };
    let looks = ground.looks();
    let t = time.elapsed_secs();
    let iso = *view == BoardView::Isometric;
    for (glint, mut sprite, mut visibility) in &mut glints {
        let water = looks
            .get(glint.tile)
            .is_some_and(|&look| matches!(cover(look), Cover::Water));
        let alpha = (t * glint.rate + glint.phase).sin().max(0.0).powi(6) * 0.85;
        let shown = iso && water && alpha > 0.02;
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if shown {
            sprite.color = Color::srgba(1.0, 1.0, 1.0, alpha);
        }
    }
}
