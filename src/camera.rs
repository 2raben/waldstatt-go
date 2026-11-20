//! The camera: an orthographic view of the board that pans and zooms (with
//! two fingers on a touch screen, too), centres
//! on the selected group, and keeps the same tile in the middle when the view
//! switches between top-down and isometric.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::input::touch::Touches;
use bevy::prelude::*;

use crate::ground::Watcher;
use crate::input::Selection;
use crate::rules::{Board, Group, Rival};
use crate::screens::playing;
use crate::view::{BoardRoot, BoardView};

/// Pan speed with the keys, in screen pixels per second.
const PAN_SPEED: f32 = 700.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 4.0;
/// Tiles across the view when a game starts.
const START_TILES: f32 = 48.0;

#[derive(Component)]
pub struct MainCamera;

/// The view the camera was last placed for.
#[derive(Resource, Default)]
struct CameraView(BoardView);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraView>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (frame_new_board, keep_tile_on_view_switch))
            .add_systems(Update, (pan_and_zoom, centre_on_selection).run_if(playing));
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((Name::new("Camera"), Camera2d, MainCamera));
}

fn zoom_of(projection: &Projection) -> f32 {
    match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    }
}

fn set_zoom(projection: &mut Projection, zoom: f32) {
    if let Projection::Orthographic(ortho) = projection {
        ortho.scale = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }
}

/// Centres on the player's first Waldleute when a game starts.
fn frame_new_board(
    view: Res<BoardView>,
    watcher: Res<Watcher>,
    windows: Query<&Window>,
    roots: Query<(), Added<BoardRoot>>,
    rivals: Query<&Rival>,
    board: Option<Res<Board>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let (false, Some(board)) = (roots.is_empty(), board) else {
        return;
    };
    let home = rivals.iter().find(|r| Some(r.id) == watcher.0).map_or(
        Vec2::new(board.width as f32, board.height as f32) / 2.0,
        |r| Vec2::new(r.home.x as f32 + 0.5, r.home.y as f32 + 0.5),
    );
    let width = windows.iter().next().map_or(1280.0, |w| w.width());
    for (mut transform, mut projection) in &mut cameras {
        transform.translation = view.project(home).extend(transform.translation.z);
        set_zoom(&mut projection, START_TILES * crate::view::TILE / width);
    }
}

fn keep_tile_on_view_switch(
    view: Res<BoardView>,
    mut last: ResMut<CameraView>,
    mut cameras: Query<&mut Transform, With<MainCamera>>,
) {
    if last.0 == *view {
        return;
    }
    for mut transform in &mut cameras {
        let tile = last.0.unproject(transform.translation.truncate());
        transform.translation = view.project(tile).extend(transform.translation.z);
    }
    last.0 = *view;
}

#[allow(clippy::too_many_arguments)]
fn pan_and_zoom(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    touches: Res<Touches>,
    view: Res<BoardView>,
    board: Option<Res<Board>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let Some(board) = board else {
        return;
    };
    let any = |codes: &[KeyCode]| keys.any_pressed(codes.iter().copied());
    let mut direction = Vec2::ZERO;
    direction.x += f32::from(u8::from(any(&[KeyCode::KeyD, KeyCode::ArrowRight])));
    direction.x -= f32::from(u8::from(any(&[KeyCode::KeyA, KeyCode::ArrowLeft])));
    direction.y += f32::from(u8::from(any(&[KeyCode::KeyW, KeyCode::ArrowUp])));
    direction.y -= f32::from(u8::from(any(&[KeyCode::KeyS, KeyCode::ArrowDown])));
    for (mut transform, mut projection) in &mut cameras {
        let zoom = zoom_of(&projection);
        let mut at = transform.translation.truncate();
        at += direction * PAN_SPEED * zoom * time.delta_secs();
        if buttons.pressed(MouseButton::Middle) {
            at += Vec2::new(-motion.delta.x, motion.delta.y) * zoom;
        }
        // Two fingers drag the board along and pinch to zoom.
        let mut pinch = 1.0;
        if let [a, b] = touches.iter().collect::<Vec<_>>()[..] {
            let delta = (a.delta() + b.delta()) / 2.0;
            at += Vec2::new(-delta.x, delta.y) * zoom;
            let before = a.previous_position().distance(b.previous_position());
            let now = a.position().distance(b.position());
            if before > 1.0 && now > 1.0 {
                pinch = before / now;
            }
        }
        // Keep the middle of the view on the board.
        let tile = view.unproject(at);
        let clamped = tile.clamp(
            Vec2::ZERO,
            Vec2::new(board.width as f32, board.height as f32),
        );
        at = view.project(clamped);
        transform.translation = at.extend(transform.translation.z);
        let mut wheel = scroll.delta.y;
        if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
            wheel += 1.0;
        }
        if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
            wheel -= 1.0;
        }
        if wheel != 0.0 || pinch != 1.0 {
            set_zoom(
                &mut projection,
                zoom * pinch * 0.88f32.powf(wheel.clamp(-3.0, 3.0)),
            );
        }
    }
}

fn centre_on_selection(
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<BoardView>,
    selection: Res<Selection>,
    groups: Query<&Group>,
    mut cameras: Query<&mut Transform, With<MainCamera>>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }
    let Some(group) = selection
        .group
        .and_then(|id| groups.iter().find(|g| g.id == id))
    else {
        return;
    };
    let at = view.centre(group.tile());
    for mut transform in &mut cameras {
        transform.translation = at.extend(transform.translation.z);
    }
}
