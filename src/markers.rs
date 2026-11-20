//! Markers on the board: groups, the selection, lines, the line being drawn,
//! the tile under the pointer and the waters' names.
//!
//! Groups are upright discs placed with the view's projection; lines and
//! highlights lie flat on the board's plane, so they turn with the view. A
//! group's disc glides from the tile it left to the tile it entered and leans
//! towards the tile it is working on; in the rules it is always on one tile.

use bevy::prelude::*;

use crate::ground::{BoardScene, Watcher};
use crate::input::{Draft, Hover, Selection};
use crate::rules::enclosure::is_trapped;
use crate::rules::group::{Activity, activity, plan_step};
use crate::rules::sight::Sight;
use crate::rules::{Board, Game, Ground, Group, GroupId, Line, Rival, Tile, Tuning};
use crate::style::{Shapes, palette, rival_colour};
use crate::view::{BoardPlane, BoardView, on_plane};

/// Fixed steps a disc takes to glide to its new tile.
const GLIDE_STEPS: f32 = 6.0;
/// Size of a group's disc, in tiles.
const DISC: f32 = 0.9;

const Z_MARKERS: f32 = 10.0;
const Z_LABELS: f32 = 40.0;

/// Added to rule entities that are groups: their drawing.
#[derive(Component)]
struct GroupMarker;

#[derive(Component)]
struct SizeText;

#[derive(Component)]
struct DangerRing;

#[derive(Component)]
struct ShipMark;

#[derive(Component)]
struct SelectionRing;

#[derive(Component)]
struct LineDot;

#[derive(Component)]
struct HoverFrame;

#[derive(Component)]
struct WaterName(Tile);

pub struct MarkersPlugin;

impl Plugin for MarkersPlugin {
    fn build(&self, app: &mut App) {
        // Dots are spawned in `Update`, so they are placed before this frame's
        // transforms propagate.
        app.add_systems(Update, (decorate_groups, spawn_scene_markers, draw_lines))
            .add_systems(
                PostUpdate,
                (place_groups, place_selection, place_hover, place_names)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// The size a marker should have in world units, in either view.
fn marker_size(view: BoardView) -> f32 {
    match view {
        BoardView::TopDown => crate::view::TILE,
        BoardView::Isometric => crate::view::ISO * 1.2,
    }
}

fn decorate_groups(
    mut commands: Commands,
    shapes: Res<Shapes>,
    groups: Query<Entity, (With<Group>, Without<GroupMarker>)>,
) {
    for entity in &groups {
        commands.entity(entity).insert((
            GroupMarker,
            Sprite {
                image: shapes.disc.clone(),
                ..default()
            },
            Transform::default(),
            Visibility::Hidden,
            children![
                (
                    SizeText,
                    Text2d::new(""),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(Color::BLACK),
                    Transform::from_xyz(0.0, 0.0, 0.002),
                ),
                (
                    DangerRing,
                    Sprite {
                        image: shapes.ring.clone(),
                        color: palette::DANGER,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 0.001),
                    Visibility::Hidden,
                ),
                (
                    ShipMark,
                    Sprite::from_color(Color::srgb(0.45, 0.3, 0.16), Vec2::ONE),
                    Transform::from_xyz(0.0, -3.0, -0.001),
                    Visibility::Hidden,
                ),
            ],
        ));
    }
}

/// Spawns the selection ring, the hover frame and the waters' names once the
/// board's plane exists.
fn spawn_scene_markers(
    mut commands: Commands,
    shapes: Res<Shapes>,
    board: Option<Res<Board>>,
    planes: Query<Entity, Added<BoardPlane>>,
) {
    let (Ok(plane), Some(board)) = (planes.single(), board) else {
        return;
    };
    commands.spawn((
        BoardScene,
        SelectionRing,
        Sprite {
            image: shapes.ring.clone(),
            color: Color::WHITE,
            ..default()
        },
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.entity(plane).with_child((
        HoverFrame,
        Sprite {
            image: shapes.frame.clone(),
            color: Color::srgba(1.0, 1.0, 1.0, 0.7),
            custom_size: Some(Vec2::ONE),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.5),
        Visibility::Hidden,
    ));
    for label in &board.labels {
        commands.spawn((
            BoardScene,
            WaterName(label.tile),
            Text2d::new(label.name.clone()),
            TextFont {
                font_size: FontSize::Px(if label.lake { 13.0 } else { 11.0 }),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.88, 1.0)),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// Where a group's disc is drawn, in tile coordinates.
fn drawn_at(
    group: &Group,
    line: &Line,
    game: &Game,
    board: &Board,
    tuning: &Tuning,
    overstep: f32,
) -> Vec2 {
    let centre = |t: Tile| Vec2::new(t.x as f32 + 0.5, t.y as f32 + 0.5);
    let since = game.step.saturating_sub(group.entered + 1) as f32 + overstep;
    let glide = (since / GLIDE_STEPS).clamp(0.0, 1.0);
    let mut at = centre(group.from).lerp(centre(group.tile), glide);
    if let Some(&next) = line.0.front()
        && let Some(plan) = plan_step(board, tuning, group, &line.0, false)
    {
        let work = group.progress as f32 / plan.work.max(1) as f32;
        at += (centre(next) - centre(group.tile)) * work * 0.35 * glide;
    }
    at
}

type MarkedGroup = (
    &'static Group,
    &'static Line,
    &'static Children,
    &'static mut Sprite,
    &'static mut Transform,
    &'static mut Visibility,
);

/// The danger ring or the ship under a group's disc.
type MarkerPart = (
    &'static mut Visibility,
    &'static mut Sprite,
    &'static mut Transform,
    Has<DangerRing>,
);

type PartFilter = (Or<(With<DangerRing>, With<ShipMark>)>, Without<GroupMarker>);

#[allow(clippy::too_many_arguments)]
fn place_groups(
    view: Res<BoardView>,
    watcher: Res<Watcher>,
    fixed: Res<Time<Fixed>>,
    board: Option<Res<Board>>,
    game: Option<Res<Game>>,
    tuning: Res<Tuning>,
    sights: Query<(&Rival, &Sight)>,
    mut groups: Query<MarkedGroup, With<GroupMarker>>,
    mut texts: Query<&mut Text2d, With<SizeText>>,
    mut parts: Query<MarkerPart, PartFilter>,
) {
    let (Some(board), Some(game)) = (board, game) else {
        return;
    };
    let sight = watcher
        .0
        .and_then(|id| sights.iter().find(|(r, _)| r.id == id).map(|(_, s)| s));
    let size = marker_size(*view);
    let overstep = fixed.overstep_fraction();
    for (group, line, children, mut sprite, mut transform, mut visibility) in &mut groups {
        let shown =
            Some(group.rival) == watcher.0 || sight.is_none_or(|s| s.sees(&board, group.tile));
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !shown {
            continue;
        }
        let at = drawn_at(group, line, &game, &board, &tuning, overstep);
        transform.translation = view.project(at).extend(Z_MARKERS + view.depth(at));
        sprite.color = rival_colour(group.rival);
        sprite.custom_size = Some(Vec2::splat(size * DISC));
        let trapped = is_trapped(&board, group, tuning.trapped_room, |t| {
            Some(group.rival) == watcher.0 || sight.is_none_or(|s| s.sees(&board, t))
        });
        let doing = activity(&board, group, line);
        let afloat = matches!(doing, Activity::Afloat | Activity::BuildingShip)
            || matches!(board.ground(group.tile), Ground::Lake(_));
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = group.size.to_string();
            }
            if let Ok((mut vis, mut sprite, mut transform, danger)) = parts.get_mut(child) {
                if danger {
                    *vis = if trapped {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    sprite.custom_size = Some(Vec2::splat(size * 1.5));
                } else {
                    *vis = if afloat {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    sprite.custom_size = Some(Vec2::new(size * 1.3, size * 0.45));
                    transform.translation.y = -size * 0.35;
                }
            }
        }
    }
}

type PlacedGroup = (&'static Group, &'static Transform);

fn place_selection(
    view: Res<BoardView>,
    selection: Res<Selection>,
    groups: Query<PlacedGroup, (With<GroupMarker>, Without<SelectionRing>)>,
    mut rings: Query<(&mut Transform, &mut Sprite, &mut Visibility), With<SelectionRing>>,
) {
    let selected = selection
        .group
        .and_then(|id| groups.iter().find(|(g, _)| g.id == id));
    for (mut transform, mut sprite, mut visibility) in &mut rings {
        match selected {
            Some((_, at)) => {
                transform.translation = at.translation - Vec3::Z * 0.0005;
                sprite.custom_size = Some(Vec2::splat(marker_size(*view) * 1.25));
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// What the dots show: each of the player's lines, and the line being drawn
/// with whether each tile is known to be blocked.
#[derive(Default, PartialEq)]
struct Dots {
    lines: Vec<(GroupId, bool, Vec<Tile>)>,
    draft: Vec<(Tile, bool)>,
}

/// Redraws the dots of the player's lines and of the line being drawn when
/// they change.
#[allow(clippy::too_many_arguments)]
fn draw_lines(
    mut commands: Commands,
    mut shown: Local<Dots>,
    shapes: Res<Shapes>,
    watcher: Res<Watcher>,
    selection: Res<Selection>,
    draft: Res<Draft>,
    board: Option<Res<Board>>,
    planes: Query<Entity, With<BoardPlane>>,
    groups: Query<(&Group, &Line)>,
    dots: Query<Entity, With<LineDot>>,
) {
    let (Ok(plane), Some(board), Some(me)) = (planes.single(), board, watcher.0) else {
        return;
    };
    let mut lines: Vec<(GroupId, bool, Vec<Tile>)> = groups
        .iter()
        .filter(|(g, l)| g.rival == me && !l.0.is_empty())
        .map(|(g, l)| {
            (
                g.id,
                selection.group == Some(g.id),
                l.0.iter().copied().collect(),
            )
        })
        .collect();
    lines.sort_by_key(|(id, ..)| *id);
    let mut blocked = false;
    let draft = draft
        .tiles
        .iter()
        .map(|&tile| {
            // Known obstacles: another rival's open tile or its margin.
            blocked |= board.open(tile).is_some_and(|r| r != me)
                || (board.open(tile).is_none() && board.in_margin_of_other(tile, me));
            (tile, blocked)
        })
        .collect();
    let wanted = Dots { lines, draft };
    // A new board has a new plane: the old dots went with the old one.
    if wanted == *shown
        && !dots.is_empty() == (!wanted.lines.is_empty() || !wanted.draft.is_empty())
    {
        return;
    }
    for dot in &dots {
        commands.entity(dot).despawn();
    }
    let mut spawn = |tile: Tile, colour: Color, size: f32| {
        let at = Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5);
        commands.entity(plane).with_child((
            LineDot,
            Sprite {
                image: shapes.disc.clone(),
                color: colour,
                custom_size: Some(Vec2::splat(size)),
                ..default()
            },
            Transform::from_translation(on_plane(at, 0.3)),
        ));
    };
    for (_, selected, tiles) in &wanted.lines {
        let colour = rival_colour(me).with_alpha(if *selected { 0.95 } else { 0.55 });
        let last = tiles.len().saturating_sub(1);
        for (i, &tile) in tiles.iter().enumerate() {
            spawn(tile, colour, if i == last { 0.5 } else { 0.28 });
        }
    }
    let last = wanted.draft.len().saturating_sub(1);
    for (i, &(tile, blocked)) in wanted.draft.iter().enumerate() {
        let colour = if blocked {
            palette::BLOCKED
        } else {
            palette::LINE
        };
        spawn(tile, colour, if i == last { 0.55 } else { 0.34 });
    }
    *shown = wanted;
}

fn place_hover(
    hover: Res<Hover>,
    mut frames: Query<(&mut Transform, &mut Visibility), With<HoverFrame>>,
) {
    for (mut transform, mut visibility) in &mut frames {
        match hover.0 {
            Some(tile) => {
                let at = Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5);
                transform.translation = on_plane(at, 0.5);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn place_names(
    view: Res<BoardView>,
    watcher: Res<Watcher>,
    board: Option<Res<Board>>,
    sights: Query<(&Rival, &Sight)>,
    mut names: Query<(&WaterName, &mut Transform, &mut Visibility)>,
) {
    let Some(board) = board else {
        return;
    };
    let sight = watcher
        .0
        .and_then(|id| sights.iter().find(|(r, _)| r.id == id).map(|(_, s)| s));
    for (name, mut transform, mut visibility) in &mut names {
        let at = Vec2::new(name.0.x as f32 + 0.5, name.0.y as f32 + 0.5);
        transform.translation = view.project(at).extend(Z_LABELS);
        let seen = sight.is_none_or(|s| s.has_seen(&board, name.0));
        *visibility = if seen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
