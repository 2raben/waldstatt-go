//! Markers on the board: groups, the selection, lines, the line being drawn,
//! the tile under the pointer and the waters' names.
//!
//! In both views every Waldmensch stands on the board (`people.rs`); a badge
//! over the lead tells the group's size and colour. Lines and highlights lie
//! flat on the board's plane, so they turn with the view; in the isometric
//! view they are drawn over the trees. A Waldmensch glides from the tile it
//! left to the tile it entered and leans towards the tile it is working on;
//! in the rules it is always on one tile.

use bevy::prelude::*;

use crate::ground::{BoardScene, Watcher};
use crate::input::{Draft, Hover, Selection};
use crate::rules::board::RADIUS;
use crate::rules::enclosure::is_trapped;
use crate::rules::group::plan_step;
use crate::rules::sight::Sight;
use crate::rules::{Board, Game, Group, GroupId, Line, Rival, Tile, Tuning};
use crate::style::{Shapes, palette, rival_colour};
use crate::view::{
    BoardPlane, BoardView, ISO, Z_BADGE, Z_GLINT, Z_LABEL, Z_OVER, Z_UPRIGHT, on_plane,
};

/// Fixed steps a Waldmensch takes to glide to its new tile.
const GLIDE_STEPS: f32 = 6.0;
/// Size of a group's badge, in tiles of the view.
const BADGE: f32 = 0.75;

/// Places the groups and sets [`Drawn`]; what follows a group runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaceGroups;

/// Added to rule entities that are groups: their drawing.
#[derive(Component)]
struct GroupMarker;

/// Where a group's lead is drawn now, in board units: between the tile
/// it left and the one it entered, leaning towards its next.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Drawn(pub Vec2);

#[derive(Component)]
struct SizeText;

#[derive(Component)]
struct DangerRing;

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
                (
                    place_groups.in_set(PlaceGroups),
                    place_selection.after(PlaceGroups),
                    place_hover,
                    place_names,
                )
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
            Drawn::default(),
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
            custom_size: Some(Vec2::new(1.0, 2.0 * RADIUS)),
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

/// Where Waldmensch `k` of a group is drawn, in board units.
pub fn member_at(
    group: &Group,
    k: usize,
    line: &Line,
    game: &Game,
    board: &Board,
    tuning: &Tuning,
    overstep: f32,
) -> Vec2 {
    let centre = Tile::centre;
    let member = &group.members[k];
    let since = game.step.saturating_sub(member.entered + 1) as f32 + overstep;
    let glide = (since / GLIDE_STEPS).clamp(0.0, 1.0);
    let mut at = centre(member.from).lerp(centre(member.tile), glide);
    let Some(next) = member.toward else {
        return at;
    };
    let work = if k == group.lead {
        plan_step(board, tuning, group, &line.0, false).map(|p| p.work)
    } else if board.open(next).is_none() && !board.is_water(next) {
        Some(tuning.cut_work)
    } else {
        Some(tuning.walk_work)
    };
    if let Some(work) = work {
        let done = (member.progress as f32 / work.max(1) as f32).min(1.0);
        at += (centre(next) - centre(member.tile)) * done * 0.35 * glide;
    }
    at
}

type MarkedGroup = (
    &'static Group,
    &'static Line,
    &'static mut Drawn,
    &'static Children,
    &'static mut Sprite,
    &'static mut Transform,
    &'static mut Visibility,
);

/// The danger ring round a group's badge.
type MarkerPart = (&'static mut Visibility, &'static mut Sprite);

type PartFilter = (With<DangerRing>, Without<GroupMarker>);

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
    let badge = marker_size(*view) * BADGE;
    let overstep = fixed.overstep_fraction();
    for (group, line, mut drawn, children, mut sprite, mut transform, mut visibility) in &mut groups
    {
        let at = member_at(group, group.lead, line, &game, &board, &tuning, overstep);
        drawn.0 = at;
        let shown =
            Some(group.rival) == watcher.0 || sight.is_none_or(|s| s.sees(&board, group.tile()));
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !shown {
            continue;
        }
        transform.translation =
            (view.project(at) + Vec2::Y * view.badge_rise()).extend(Z_BADGE + view.depth(at));
        sprite.color = rival_colour(group.rival);
        sprite.custom_size = Some(Vec2::splat(badge));
        let trapped = is_trapped(&board, group, tuning.trapped_room, |t| {
            Some(group.rival) == watcher.0 || sight.is_none_or(|s| s.sees(&board, t))
        });
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = group.size().to_string();
            }
            if let Ok((mut vis, mut sprite)) = parts.get_mut(child) {
                *vis = if trapped {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                sprite.custom_size = Some(Vec2::splat(badge * 1.6));
            }
        }
    }
}

/// The selected group's ring: on the ground round its lead.
fn place_selection(
    view: Res<BoardView>,
    selection: Res<Selection>,
    groups: Query<(&Group, &Drawn), With<GroupMarker>>,
    mut rings: Query<(&mut Transform, &mut Sprite, &mut Visibility), With<SelectionRing>>,
) {
    let selected = selection
        .group
        .and_then(|id| groups.iter().find(|(g, _)| g.id == id));
    for (mut transform, mut sprite, mut visibility) in &mut rings {
        match selected {
            Some((_, drawn)) => {
                // Under every upright thing.
                transform.translation = view.project(drawn.0).extend(Z_UPRIGHT - 0.05);
                let width = match *view {
                    BoardView::Isometric => ISO * 1.5,
                    BoardView::TopDown => crate::view::TILE * 1.1,
                };
                sprite.custom_size = Some(Vec2::new(width, width * 0.5));
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// How high flat marks lie on the plane: on the ground seen from above, over
/// the trees in the isometric view.
fn flat_z(view: BoardView, z: f32) -> f32 {
    match view {
        BoardView::TopDown => z,
        BoardView::Isometric => Z_OVER + z,
    }
}

/// What the dots show: each of the player's lines, and the line being drawn
/// with whether each tile is known to be blocked.
#[derive(Default, PartialEq)]
struct Dots {
    view: BoardView,
    lines: Vec<(GroupId, bool, Vec<Tile>)>,
    draft: Vec<(Tile, bool)>,
}

/// Redraws the dots of the player's lines and of the line being drawn when
/// they change.
#[allow(clippy::too_many_arguments)]
fn draw_lines(
    mut commands: Commands,
    mut shown: Local<Dots>,
    view: Res<BoardView>,
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
    let wanted = Dots {
        view: *view,
        lines,
        draft,
    };
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
        let at = tile.centre();
        commands.entity(plane).with_child((
            LineDot,
            Sprite {
                image: shapes.disc.clone(),
                color: colour,
                custom_size: Some(Vec2::splat(size)),
                ..default()
            },
            Transform::from_translation(on_plane(at, flat_z(*view, 0.3))),
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
    view: Res<BoardView>,
    hover: Res<Hover>,
    mut frames: Query<(&mut Transform, &mut Visibility), With<HoverFrame>>,
) {
    for (mut transform, mut visibility) in &mut frames {
        match hover.0 {
            Some(tile) => {
                transform.translation = on_plane(tile.centre(), flat_z(*view, 0.5));
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
        let at = name.0.centre();
        // In the isometric view, on the water under boats and Waldleute.
        let z = match *view {
            BoardView::TopDown => Z_LABEL,
            BoardView::Isometric => Z_GLINT + 0.5,
        };
        transform.translation = view.project(at).extend(z);
        let seen = sight.is_none_or(|s| s.has_seen(&board, name.0));
        *visibility = if seen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
