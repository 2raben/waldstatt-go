//! Device input: the pointer and keys become commands for the player's rival
//! (ADR 0002). This and the camera are the only modules that read devices.
//!
//! The pointer picks tiles through the active view, so drawing a line works
//! the same in both views. A drawn line is a sequence of steps of one tile,
//! straight or diagonal, that follows the pointer; a click beside the selected
//! group sends it the fastest way there instead (`Order::Goto`). Pressing on
//! any of the player's Waldleute (its tile, its figure, or its group's badge)
//! selects the group and starts a line from that Waldmensch, who leads from
//! then on; dragging from anywhere else draws from the selected group's lead.
//! Moving back along the line takes it back.

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::MainCamera;
use crate::ground::Watcher;
use crate::markers::Drawn;
use crate::rules::game::GameEvent;
use crate::rules::{Board, Controls, Game, Group, GroupId, Order, RivalId, Tile, line_limit};
use crate::screens::playing;
use crate::view::BoardView;

/// The player's selected group.
#[derive(Resource, Default, Debug)]
pub struct Selection {
    pub group: Option<GroupId>,
    events_seen: usize,
}

/// The line being drawn, from the Waldmensch on `from`; or, while the
/// pointer has not left the tile pressed beside the group, a click there.
#[derive(Resource, Default, Debug)]
pub struct Draft {
    pub group: Option<GroupId>,
    pub from: Tile,
    pub tiles: Vec<Tile>,
    pub click: Option<Tile>,
}

/// The tile under the pointer.
#[derive(Resource, Default, Debug)]
pub struct Hover(pub Option<Tile>);

/// Where the pointer is, in world coordinates.
#[derive(Resource, Default, Debug)]
struct PointerAt(Option<Vec2>);

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Draft>()
            .init_resource::<Hover>()
            .init_resource::<PointerAt>()
            .add_systems(
                PreUpdate,
                (hover, follow_selection, pointer, keys)
                    .chain()
                    .after(InputSystems)
                    .run_if(playing),
            );
    }
}

/// Forgets the selection and the line being drawn, for a new game.
pub fn reset(world: &mut World) {
    world.insert_resource(Selection::default());
    world.insert_resource(Draft::default());
    world.insert_resource(Hover::default());
}

fn hover(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    view: Res<BoardView>,
    board: Option<Res<Board>>,
    mut hover: ResMut<Hover>,
    mut pointer: ResMut<PointerAt>,
) {
    let world = (|| {
        let cursor = windows.single().ok()?.cursor_position()?;
        let (camera, transform) = cameras.single().ok()?;
        camera.viewport_to_world_2d(transform, cursor).ok()
    })();
    pointer.0 = world;
    let tile = world.and_then(|world| view.pick(board.as_deref()?, world));
    if hover.0 != tile {
        hover.0 = tile;
    }
}

/// Keeps the selection on one of the player's groups: the group it merged
/// into, or the largest when it was enclosed.
fn follow_selection(
    watcher: Res<Watcher>,
    game: Option<Res<Game>>,
    groups: Query<&Group>,
    mut selection: ResMut<Selection>,
) {
    let (Some(me), Some(game)) = (watcher.0, game) else {
        return;
    };
    let seen = selection.events_seen.min(game.events.len());
    for event in &game.events[seen..] {
        if let GameEvent::Merged { from, into, .. } = event
            && selection.group == Some(*from)
        {
            selection.group = Some(*into);
        }
    }
    selection.events_seen = game.events.len();
    let own = |id: GroupId| groups.iter().any(|g| g.id == id && g.rival == me);
    if !selection.group.is_some_and(own) {
        selection.group = groups
            .iter()
            .filter(|g| g.rival == me)
            .max_by_key(|g| (g.size(), std::cmp::Reverse(g.id)))
            .map(|g| g.id);
    }
}

/// One step from `at` towards `to`: diagonal while both axes differ.
fn step_towards(at: Tile, to: Tile) -> Tile {
    Tile::new(at.x + (to.x - at.x).signum(), at.y + (to.y - at.y).signum())
}

/// Extends the line from `start` towards `to` in steps of one tile, up to
/// `limit` tiles, or takes it back to `to` if the line already passes there.
pub fn extend_line(tiles: &mut Vec<Tile>, start: Tile, to: Tile, limit: usize) {
    if to == start {
        tiles.clear();
        return;
    }
    if let Some(i) = tiles.iter().position(|&t| t == to) {
        tiles.truncate(i + 1);
        return;
    }
    let mut at = tiles.last().copied().unwrap_or(start);
    while at != to && tiles.len() < limit {
        let next = step_towards(at, to);
        if next == start {
            tiles.clear();
        } else if let Some(i) = tiles.iter().position(|&t| t == next) {
            tiles.truncate(i + 1);
        } else {
            tiles.push(next);
        }
        at = next;
    }
}

/// The player's Waldmensch under the pointer: on the tile under it, else
/// the one whose figure or group badge it is on, nearest first.
fn waldmensch_under(
    view: BoardView,
    tile: Option<Tile>,
    world: Option<Vec2>,
    me: RivalId,
    groups: &Query<(&Group, &Drawn)>,
) -> Option<(GroupId, Tile)> {
    let own = || groups.iter().filter(|(g, _)| g.rival == me);
    if let Some(tile) = tile {
        let mut here: Vec<GroupId> = own()
            .filter(|(g, _)| g.tiles().any(|t| t == tile))
            .map(|(g, _)| g.id)
            .collect();
        here.sort();
        if let Some(&id) = here.first() {
            return Some((id, tile));
        }
    }
    let world = world?;
    let scale = view.figure_scale();
    let mut best: Option<(f32, GroupId, Tile)> = None;
    for (group, drawn) in own() {
        let badge = view.project(drawn.0) + Vec2::Y * view.badge_rise();
        let mut hits: Vec<(f32, Tile)> = group
            .tiles()
            .filter_map(|t| {
                let feet = view.project(Vec2::new(t.x as f32 + 0.5, t.y as f32 + 0.5));
                let d = world - feet;
                (d.x.abs() < 6.0 * scale && (-2.0..17.0 * scale).contains(&d.y))
                    .then_some((d.length(), t))
            })
            .collect();
        if world.distance(badge) < 8.0 * scale {
            hits.push((0.0, group.tile()));
        }
        for (d, t) in hits {
            if best.is_none_or(|(b, ..)| d < b) {
                best = Some((d, group.id, t));
            }
        }
    }
    best.map(|(_, id, t)| (id, t))
}

#[allow(clippy::too_many_arguments)]
fn pointer(
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<Hover>,
    at: Res<PointerAt>,
    view: Res<BoardView>,
    watcher: Res<Watcher>,
    groups: Query<(&Group, &Drawn)>,
    mut selection: ResMut<Selection>,
    mut draft: ResMut<Draft>,
    mut controls: ResMut<Controls>,
) {
    let Some(me) = watcher.0 else {
        return;
    };
    let group_of = |id: Option<GroupId>| {
        id.and_then(|id| {
            groups
                .iter()
                .map(|(g, _)| g)
                .find(|g| g.id == id && g.rival == me)
        })
    };
    if buttons.just_pressed(MouseButton::Left) {
        if let Some((id, from)) = waldmensch_under(*view, hover.0, at.0, me, &groups) {
            // Pressing on a Waldmensch: it leads the line drawn from it.
            selection.group = Some(id);
            *draft = Draft {
                group: Some(id),
                from,
                tiles: Vec::new(),
                click: None,
            };
        } else if let (Some(group), Some(tile)) = (group_of(selection.group), hover.0) {
            // Pressing beside the group: a click sends it the fastest way
            // there; dragging on draws a line from its lead.
            *draft = Draft {
                group: Some(group.id),
                from: group.tile(),
                tiles: Vec::new(),
                click: Some(tile),
            };
        }
    }
    if buttons.pressed(MouseButton::Left)
        && let (Some(group), Some(tile)) = (group_of(draft.group), hover.0)
        && draft.click != Some(tile)
        && draft.tiles.last() != Some(&tile)
    {
        draft.click = None;
        let from = draft.from;
        extend_line(&mut draft.tiles, from, tile, line_limit(group.size()));
    }
    if buttons.just_released(MouseButton::Left) {
        if let (Some(group), Some(to)) = (draft.group, draft.click) {
            controls.send(
                me,
                Order::Goto {
                    group,
                    from: draft.from,
                    to,
                },
            );
        } else if let Some(group) = draft.group
            && !draft.tiles.is_empty()
        {
            let tiles = std::mem::take(&mut draft.tiles);
            controls.send(
                me,
                Order::Line {
                    group,
                    from: draft.from,
                    tiles,
                },
            );
        }
        *draft = Draft::default();
    }
    if buttons.just_pressed(MouseButton::Right) {
        if draft.group.is_some() {
            *draft = Draft::default();
        } else if let Some(group) = group_of(selection.group) {
            controls.send(me, stop(group));
        }
    }
}

/// Stops a group where its lead stands.
fn stop(group: &Group) -> Order {
    Order::Line {
        group: group.id,
        from: group.tile(),
        tiles: Vec::new(),
    }
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    watcher: Res<Watcher>,
    groups: Query<&Group>,
    mut view: ResMut<BoardView>,
    mut selection: ResMut<Selection>,
    mut controls: ResMut<Controls>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        *view = view.toggled();
    }
    let Some(me) = watcher.0 else {
        return;
    };
    if keys.just_pressed(KeyCode::Tab) {
        let mut own: Vec<GroupId> = groups
            .iter()
            .filter(|g| g.rival == me)
            .map(|g| g.id)
            .collect();
        own.sort();
        if !own.is_empty() {
            let back = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            let at = own
                .iter()
                .position(|&id| Some(id) == selection.group)
                .unwrap_or(0);
            let next = if back { at + own.len() - 1 } else { at + 1 } % own.len();
            selection.group = Some(own[next]);
        }
    }
    let Some(group) = selection.group else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyT) {
        controls.send(me, Order::Split { group });
    }
    if keys.any_just_pressed([KeyCode::KeyX, KeyCode::Backspace])
        && let Some(group) = groups.iter().find(|g| g.id == group)
    {
        controls.send(me, stop(group));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawn_line_steps_one_tile_at_a_time_and_can_be_taken_back() {
        let start = Tile::new(0, 0);
        let mut tiles = Vec::new();
        extend_line(&mut tiles, start, Tile::new(3, 2), 10);
        assert_eq!(tiles.len(), 3, "diagonals count one: {tiles:?}");
        let mut at = start;
        for &tile in &tiles {
            assert_eq!(at.chebyshev(tile), 1);
            at = tile;
        }
        assert_eq!(at, Tile::new(3, 2));
        extend_line(&mut tiles, start, Tile::new(2, 0), 10);
        assert_eq!(tiles.last(), Some(&Tile::new(2, 0)));
        extend_line(&mut tiles, start, start, 10);
        assert!(tiles.is_empty());
        extend_line(&mut tiles, start, Tile::new(20, 0), 10);
        assert_eq!(tiles.len(), 10, "no longer than the limit");
    }
}
