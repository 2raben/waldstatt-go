//! Device input: the pointer and keys become commands for the player's rival
//! (ADR 0002). This and the camera are the only modules that read devices.
//!
//! The pointer is the mouse, or one finger on a touch screen: a tap is a
//! click, and holding the finger still turns the press into the right button.
//! It picks tiles through the active view, so drawing a line works the same in
//! both views. A drawn line is a sequence of steps of one tile, straight or
//! diagonal, that follows the pointer.
//!
//! Pressing on one of the player's groups (a Waldmensch, its figure, its
//! badge, or its clearing) selects it. Dragging on draws a line from that
//! Waldmensch, or the one nearest the clearing pressed, who leads from then
//! on; moving back along the line takes it back. Pressing anywhere else
//! drags from the selected group's lead, and a click there sends the group
//! the fastest way to it (`Order::Goto`). A double click on another group
//! sends the group selected before the fastest way to it, so they merge.
//! The right button on a group splits it; dragging on draws a line for the
//! half nearest the Waldmensch pressed, which goes its own way along it.

use bevy::input::mouse::MouseButtonInput;
use bevy::input::touch::Touches;
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::MainCamera;
use crate::ground::Watcher;
use crate::markers::Drawn;
use crate::rules::game::GameEvent;
use crate::rules::{
    Board, Controls, Game, Group, GroupId, Order, RivalId, Tile, Trail, line_limit,
};
use crate::screens::playing;
use crate::view::BoardView;

/// Seconds within which a second click counts as a double click.
const DOUBLE_CLICK: f64 = 0.4;
/// Seconds a finger is held still before the press turns into the right button.
const LONG_PRESS: f64 = 0.5;
/// How far a finger may move, in screen pixels, and still be held still.
const SLOP: f32 = 12.0;

/// The player's selected group.
#[derive(Resource, Default, Debug)]
pub struct Selection {
    pub group: Option<GroupId>,
    events_seen: usize,
}

/// What releasing a press does when the pointer has not left the tile pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Click {
    /// Nothing more: the press selected the group.
    #[default]
    Select,
    /// Sends the group the fastest way to this tile.
    Goto(Tile),
    /// Splits the group.
    Split,
}

/// The line being drawn for `group`, from its Waldmensch on `from`; or, while
/// the pointer has not left the tile pressed, a click there.
#[derive(Resource, Default, Debug)]
pub struct Draft {
    pub group: Option<GroupId>,
    pub from: Tile,
    pub tiles: Vec<Tile>,
    /// The tile pressed, until the pointer leaves it.
    pub pressed: Option<Tile>,
    pub click: Click,
    button: Button,
    /// The group selected before the press, for a double click.
    before: Option<GroupId>,
}

/// The tile under the pointer.
#[derive(Resource, Default, Debug)]
pub struct Hover(pub Option<Tile>);

/// Where the pointer is, in world coordinates.
#[derive(Resource, Default, Debug)]
struct PointerAt(Option<Vec2>);

/// A button of the pointer: the mouse's, or a finger's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Button {
    #[default]
    Primary,
    Secondary,
}

/// What the pointer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    Down(Button),
    Up(Button),
    /// The press ends without doing anything: a long press turned into the
    /// right button, or a second finger came down.
    Cancel,
}

/// The finger that is the pointer.
#[derive(Clone, Copy, Debug)]
struct Finger {
    id: u64,
    start: Vec2,
    since: f64,
    button: Button,
    still: bool,
}

/// The pointer, read from the mouse and the touch screen each frame.
#[derive(Resource, Default, Debug)]
struct Pointer {
    /// Where it is on the screen.
    screen: Option<Vec2>,
    /// The buttons held: primary, secondary.
    held: [bool; 2],
    /// What it did this frame, in order.
    presses: Vec<Press>,
    finger: Option<Finger>,
    /// After several fingers, none is the pointer until all are lifted.
    several: bool,
}

impl Pointer {
    fn held(&self, button: Button) -> bool {
        self.held[button as usize]
    }
}

/// The last click that selected a group: when, which, and the group
/// selected before it, for a double click.
#[derive(Resource, Default, Debug)]
struct LastClick(Option<(f64, GroupId, Option<GroupId>)>);

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Draft>()
            .init_resource::<Hover>()
            .init_resource::<PointerAt>()
            .init_resource::<Pointer>()
            .init_resource::<LastClick>()
            // Every frame, so clicks in menus are gone before a game starts.
            .add_systems(PreUpdate, read_pointer.after(InputSystems))
            .add_systems(
                PreUpdate,
                (hover, follow_selection, pointer, keys)
                    .chain()
                    .after(read_pointer)
                    .run_if(playing),
            );
    }
}

/// Forgets the selection and the line being drawn, for a new game.
pub fn reset(world: &mut World) {
    world.insert_resource(Selection::default());
    world.insert_resource(Draft::default());
    world.insert_resource(Hover::default());
    world.insert_resource(LastClick::default());
}

/// Reads the mouse and the first finger on the touch screen into the pointer.
fn read_pointer(
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut clicks: MessageReader<MouseButtonInput>,
    touches: Res<Touches>,
    mut pointer: ResMut<Pointer>,
) {
    pointer.presses.clear();
    // In the order they came, so two clicks within one frame stay two.
    for input in clicks.read() {
        let button = match input.button {
            MouseButton::Left => Button::Primary,
            MouseButton::Right => Button::Secondary,
            _ => continue,
        };
        pointer.presses.push(match input.state {
            ButtonState::Pressed => Press::Down(button),
            ButtonState::Released => Press::Up(button),
        });
    }
    pointer.held = [
        buttons.pressed(MouseButton::Left),
        buttons.pressed(MouseButton::Right),
    ];
    pointer.screen = windows.single().ok().and_then(Window::cursor_position);

    let now = time.elapsed_secs_f64();
    let fingers = touches.iter().count();
    if fingers > 1 {
        // Two fingers move the view (`camera.rs`).
        if pointer.finger.take().is_some() {
            pointer.presses.push(Press::Cancel);
        }
        pointer.several = true;
    } else if fingers == 0 && pointer.finger.is_none() {
        pointer.several = false;
    }
    if pointer.finger.is_none()
        && !pointer.several
        && let Some(touch) = touches.iter_just_pressed().next()
    {
        pointer.finger = Some(Finger {
            id: touch.id(),
            start: touch.position(),
            since: now,
            button: Button::Primary,
            still: true,
        });
        pointer.presses.push(Press::Down(Button::Primary));
    }
    let Some(mut finger) = pointer.finger else {
        return;
    };
    if let Some(touch) = touches.get_released(finger.id) {
        pointer.screen = Some(touch.position());
        pointer.presses.push(Press::Up(finger.button));
        pointer.finger = None;
        return;
    }
    let Some(touch) = touches.get_pressed(finger.id) else {
        pointer.presses.push(Press::Cancel);
        pointer.finger = None;
        return;
    };
    pointer.screen = Some(touch.position());
    finger.still &= touch.position().distance(finger.start) <= SLOP;
    if finger.still && finger.button == Button::Primary && now - finger.since >= LONG_PRESS {
        finger.button = Button::Secondary;
        pointer
            .presses
            .extend([Press::Cancel, Press::Down(Button::Secondary)]);
    }
    pointer.held[finger.button as usize] = true;
    pointer.finger = Some(finger);
}

fn hover(
    input: Res<Pointer>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    view: Res<BoardView>,
    board: Option<Res<Board>>,
    mut hover: ResMut<Hover>,
    mut pointer: ResMut<PointerAt>,
) {
    let world = (|| {
        let cursor = input.screen?;
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

/// The player's group under the pointer and the Waldmensch it is pressed at:
/// the one on the tile under it, else the one whose figure or group badge it
/// is on, nearest first, else the one nearest the tile on the group's
/// clearing under it.
fn group_under(
    view: BoardView,
    tile: Option<Tile>,
    world: Option<Vec2>,
    me: RivalId,
    groups: &Query<(&Group, &Trail, &Drawn)>,
) -> Option<(GroupId, Tile)> {
    let own = || groups.iter().filter(|(g, ..)| g.rival == me);
    if let Some(tile) = tile
        && let Some((group, ..)) = own()
            .filter(|(g, ..)| g.tiles().any(|t| t == tile))
            .min_by_key(|(g, ..)| g.id)
    {
        return Some((group.id, tile));
    }
    if let Some(world) = world {
        let scale = view.figure_scale();
        let mut best: Option<(f32, GroupId, Tile)> = None;
        for (group, _, drawn) in own() {
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
        if let Some((_, id, t)) = best {
            return Some((id, t));
        }
    }
    let tile = tile?;
    let (group, ..) = own()
        .filter(|(_, trail, _)| trail.contains(tile))
        .min_by_key(|(g, ..)| g.id)?;
    let nearest = group.tiles().min_by_key(|t| t.chebyshev(tile))?;
    Some((group.id, nearest))
}

#[allow(clippy::too_many_arguments)]
fn pointer(
    time: Res<Time<Real>>,
    input: Res<Pointer>,
    hover: Res<Hover>,
    at: Res<PointerAt>,
    view: Res<BoardView>,
    watcher: Res<Watcher>,
    groups: Query<(&Group, &Trail, &Drawn)>,
    mut selection: ResMut<Selection>,
    mut draft: ResMut<Draft>,
    mut last_click: ResMut<LastClick>,
    mut controls: ResMut<Controls>,
) {
    let Some(me) = watcher.0 else {
        return;
    };
    let group_of = |id: Option<GroupId>| {
        id.and_then(|id| {
            groups
                .iter()
                .map(|(g, ..)| g)
                .find(|g| g.id == id && g.rival == me)
        })
    };
    let now = time.elapsed_secs_f64();
    let under = || group_under(*view, hover.0, at.0, me, &groups);
    // Follows the pointer with the line once it leaves the tile pressed.
    let drag = |draft: &mut Draft| {
        let (Some(group), Some(tile)) = (group_of(draft.group), hover.0) else {
            return;
        };
        if draft.pressed == Some(tile) || draft.tiles.last() == Some(&tile) {
            return;
        }
        draft.pressed = None;
        let size = match draft.click {
            // The half that goes draws the line.
            Click::Split => group.size() / 2,
            _ => group.size(),
        };
        let from = draft.from;
        extend_line(&mut draft.tiles, from, tile, line_limit(size));
    };
    for &press in &input.presses {
        match press {
            Press::Down(Button::Primary) => {
                if let Some((id, from)) = under() {
                    let before = match last_click.0.take() {
                        Some((when, group, before))
                            if group == id && now - when <= DOUBLE_CLICK =>
                        {
                            before
                                .and_then(|b| group_of(Some(b)))
                                .filter(|b| b.id != id)
                        }
                        _ => None,
                    };
                    if let Some(group) = before {
                        // A double click on another group: the one selected
                        // before goes there, so they merge.
                        // A drag draws its line from its lead instead.
                        selection.group = Some(group.id);
                        *draft = Draft {
                            group: Some(group.id),
                            from: group.tile(),
                            tiles: Vec::new(),
                            pressed: hover.0.or(Some(from)),
                            click: Click::Goto(from),
                            button: Button::Primary,
                            before: None,
                        };
                    } else {
                        *draft = Draft {
                            group: Some(id),
                            from,
                            tiles: Vec::new(),
                            pressed: hover.0.or(Some(from)),
                            click: Click::Select,
                            button: Button::Primary,
                            before: selection.group,
                        };
                        selection.group = Some(id);
                    }
                } else if let (Some(group), Some(tile)) = (group_of(selection.group), hover.0) {
                    *draft = Draft {
                        group: Some(group.id),
                        from: group.tile(),
                        tiles: Vec::new(),
                        pressed: Some(tile),
                        click: Click::Goto(tile),
                        button: Button::Primary,
                        before: None,
                    };
                }
            }
            Press::Down(Button::Secondary) => {
                if draft.group.is_some() && input.held(draft.button) {
                    // The right button while drawing takes the line back.
                    *draft = Draft::default();
                } else if let Some((id, from)) = under() {
                    selection.group = Some(id);
                    *draft = Draft {
                        group: Some(id),
                        from,
                        tiles: Vec::new(),
                        pressed: hover.0.or(Some(from)),
                        click: Click::Split,
                        button: Button::Secondary,
                        before: None,
                    };
                } else if let Some(group) = group_of(selection.group) {
                    controls.send(me, stop(group));
                }
            }
            Press::Up(button) if draft.group.is_some() && draft.button == button => {
                drag(&mut draft);
                let done = std::mem::take(&mut *draft);
                let Some(group) = done.group else {
                    continue;
                };
                match (done.pressed, done.click) {
                    (Some(_), Click::Select) => last_click.0 = Some((now, group, done.before)),
                    (Some(_), Click::Goto(to)) => controls.send(
                        me,
                        Order::Goto {
                            group,
                            from: done.from,
                            to,
                        },
                    ),
                    (Some(_), Click::Split) => controls.send(me, Order::split(group)),
                    (None, _) if done.tiles.is_empty() => {}
                    (None, Click::Split) => controls.send(
                        me,
                        Order::Split {
                            group,
                            from: Some(done.from),
                            tiles: done.tiles,
                        },
                    ),
                    (None, _) => controls.send(
                        me,
                        Order::Line {
                            group,
                            from: done.from,
                            tiles: done.tiles,
                        },
                    ),
                }
            }
            Press::Up(_) => {}
            Press::Cancel => *draft = Draft::default(),
        }
    }
    if input.held(draft.button) {
        drag(&mut draft);
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
        controls.send(me, Order::split(group));
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
