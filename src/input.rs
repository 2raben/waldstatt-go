//! Device input: the pointer and keys become commands for the player's rival
//! (ADR 0002). This and the camera are the only modules that read devices.
//!
//! The pointer picks tiles through the active view, so drawing a line works
//! the same in both views. A line is a sequence of straight steps that follows
//! the pointer: no smoothing and no way round obstacles. Pressing on one of
//! the player's groups selects it and starts a line from it; pressing anywhere
//! else draws from the selected group. Moving back along the line takes it back.

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::MainCamera;
use crate::ground::Watcher;
use crate::rules::game::GameEvent;
use crate::rules::{Board, Controls, Game, Group, GroupId, Order, Tile, line_limit};
use crate::screens::playing;
use crate::view::BoardView;

/// The player's selected group, which also receives the next Waldmensch.
#[derive(Resource, Default, Debug)]
pub struct Selection {
    pub group: Option<GroupId>,
    /// The receiver last sent to the rules.
    sent: Option<GroupId>,
    events_seen: usize,
}

/// The line being drawn.
#[derive(Resource, Default, Debug)]
pub struct Draft {
    pub group: Option<GroupId>,
    pub tiles: Vec<Tile>,
}

/// The tile under the pointer.
#[derive(Resource, Default, Debug)]
pub struct Hover(pub Option<Tile>);

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Draft>()
            .init_resource::<Hover>()
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
) {
    let tile = (|| {
        let cursor = windows.single().ok()?.cursor_position()?;
        let (camera, transform) = cameras.single().ok()?;
        let world = camera.viewport_to_world_2d(transform, cursor).ok()?;
        view.pick(board.as_deref()?, world)
    })();
    if hover.0 != tile {
        hover.0 = tile;
    }
}

/// Keeps the selection on one of the player's groups: the group it merged
/// into, or the largest when it was enclosed. Tells the rules which group
/// receives the next Waldmensch whenever the selection changes.
fn follow_selection(
    watcher: Res<Watcher>,
    game: Option<Res<Game>>,
    groups: Query<&Group>,
    mut selection: ResMut<Selection>,
    mut controls: ResMut<Controls>,
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
            .max_by_key(|g| (g.size, std::cmp::Reverse(g.id)))
            .map(|g| g.id);
    }
    if selection.group != selection.sent {
        selection.sent = selection.group;
        if let Some(group) = selection.group {
            controls.send(me, Order::Receiver { group });
        }
    }
}

/// One straight step from `at` towards `to`, along the longer axis first.
fn step_towards(at: Tile, to: Tile) -> Tile {
    let (dx, dy) = (to.x - at.x, to.y - at.y);
    if dx.abs() >= dy.abs() {
        Tile::new(at.x + dx.signum(), at.y)
    } else {
        Tile::new(at.x, at.y + dy.signum())
    }
}

/// Extends the line from `start` towards `to` in straight steps, up to
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

fn pointer(
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<Hover>,
    watcher: Res<Watcher>,
    groups: Query<&Group>,
    mut selection: ResMut<Selection>,
    mut draft: ResMut<Draft>,
    mut controls: ResMut<Controls>,
) {
    let Some(me) = watcher.0 else {
        return;
    };
    let group_of =
        |id: Option<GroupId>| id.and_then(|id| groups.iter().find(|g| g.id == id && g.rival == me));
    if buttons.just_pressed(MouseButton::Left)
        && let Some(tile) = hover.0
    {
        let mut here: Vec<&Group> = groups
            .iter()
            .filter(|g| g.rival == me && g.tile == tile)
            .collect();
        here.sort_by_key(|g| g.id);
        if !here.is_empty() {
            // Several groups on one tile: each press picks the next.
            let at = here.iter().position(|g| Some(g.id) == selection.group);
            let pick = here[at.map_or(0, |i| (i + 1) % here.len())];
            selection.group = Some(pick.id);
            *draft = Draft {
                group: Some(pick.id),
                tiles: Vec::new(),
            };
        } else if let Some(group) = group_of(selection.group) {
            let mut tiles = Vec::new();
            extend_line(&mut tiles, group.tile, tile, line_limit(group.size));
            *draft = Draft {
                group: Some(group.id),
                tiles,
            };
        }
    }
    if buttons.pressed(MouseButton::Left)
        && let (Some(group), Some(tile)) = (group_of(draft.group), hover.0)
        && draft.tiles.last() != Some(&tile)
    {
        let (start, limit) = (group.tile, line_limit(group.size));
        extend_line(&mut draft.tiles, start, tile, limit);
    }
    if buttons.just_released(MouseButton::Left) {
        if let Some(group) = draft.group
            && !draft.tiles.is_empty()
        {
            let tiles = std::mem::take(&mut draft.tiles);
            controls.send(me, Order::Line { group, tiles });
        }
        *draft = Draft::default();
    }
    if buttons.just_pressed(MouseButton::Right) {
        if draft.group.is_some() {
            *draft = Draft::default();
        } else if let Some(group) = selection.group {
            controls.send(
                me,
                Order::Line {
                    group,
                    tiles: Vec::new(),
                },
            );
        }
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
    if keys.any_just_pressed([KeyCode::KeyX, KeyCode::Backspace]) {
        controls.send(
            me,
            Order::Line {
                group,
                tiles: Vec::new(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawn_line_is_straight_steps_and_can_be_taken_back() {
        let start = Tile::new(0, 0);
        let mut tiles = Vec::new();
        extend_line(&mut tiles, start, Tile::new(3, 2), 10);
        assert_eq!(tiles.len(), 5);
        let mut at = start;
        for &tile in &tiles {
            assert!(at.is_next_to(tile));
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
