//! The two views of the board: top-down and isometric (ADR 0001).
//!
//! The rules know only tiles. A view is a projection from tile coordinates to
//! world coordinates; the pointer picks a tile through the same projection,
//! inverted. The isometric view is the top-down board turned 45° and squashed
//! to half its height, so the flat parts of the board (the ground image, lines
//! and highlights) are children of one plane whose transform is the view, and
//! upright markers are placed with [`BoardView::project`]. Switching views
//! changes only drawing, never the game.

use std::f32::consts::FRAC_PI_4;

use bevy::prelude::*;

use crate::rules::{Board, Tile};

/// World units per tile side in the top-down view.
pub const TILE: f32 = 16.0;
/// Half the width of a tile's diamond in the isometric view; it is half as high.
pub const ISO: f32 = 16.0;

// Draw heights, bottom to top: the ground at 0, glints on the water, upright
// things (discs, trees, Waldleute, boats) sorted by [`BoardView::depth`]
// above `Z_UPRIGHT`, marks drawn over the trees, badges and names.
pub const Z_GLINT: f32 = 1.0;
pub const Z_UPRIGHT: f32 = 10.0;
pub const Z_OVER: f32 = 25.0;
pub const Z_BADGE: f32 = 30.0;
pub const Z_LABEL: f32 = 40.0;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardView {
    #[default]
    TopDown,
    Isometric,
}

impl BoardView {
    pub fn toggled(self) -> Self {
        match self {
            BoardView::TopDown => BoardView::Isometric,
            BoardView::Isometric => BoardView::TopDown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BoardView::TopDown => "von oben",
            BoardView::Isometric => "isometrisch",
        }
    }

    /// How much larger than painted the Waldleute and boats are drawn.
    pub fn figure_scale(self) -> f32 {
        match self {
            BoardView::TopDown => 0.75,
            BoardView::Isometric => 1.35,
        }
    }

    /// How high a group's badge floats over its lead's feet, in world units.
    pub fn badge_rise(self) -> f32 {
        21.0 * self.figure_scale()
    }

    /// World position of a point in tile coordinates; a tile's centre is
    /// `(x + 0.5, y + 0.5)`. North is up in the top-down view and to the upper
    /// right in the isometric one, as in the Waldstatt.
    pub fn project(self, p: Vec2) -> Vec2 {
        match self {
            BoardView::TopDown => Vec2::new(p.x * TILE, -p.y * TILE),
            BoardView::Isometric => Vec2::new((p.x - p.y) * ISO, -(p.x + p.y) * ISO / 2.0),
        }
    }

    /// The inverse of [`BoardView::project`].
    pub fn unproject(self, w: Vec2) -> Vec2 {
        match self {
            BoardView::TopDown => Vec2::new(w.x / TILE, -w.y / TILE),
            BoardView::Isometric => {
                let (u, v) = (w.x / ISO, -2.0 * w.y / ISO);
                Vec2::new((u + v) / 2.0, (v - u) / 2.0)
            }
        }
    }

    pub fn centre(self, tile: Tile) -> Vec2 {
        self.project(Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5))
    }

    /// The tile under a world position, if it is on the board.
    pub fn pick(self, board: &Board, world: Vec2) -> Option<Tile> {
        let p = self.unproject(world).floor();
        let tile = Tile::new(p.x as i32, p.y as i32);
        board.contains(tile).then_some(tile)
    }

    /// Draw order of upright markers: further south and east is nearer.
    pub fn depth(self, p: Vec2) -> f32 {
        match self {
            BoardView::TopDown => p.y * 0.001,
            BoardView::Isometric => (p.x + p.y) * 0.001,
        }
    }

    /// The transforms of the board's root (scale) and of its plane (turn).
    /// Plane coordinates are tile coordinates with y pointing north:
    /// `(x, -y)`.
    pub fn transforms(self) -> (Transform, Transform) {
        match self {
            BoardView::TopDown => (
                Transform::from_scale(Vec3::new(TILE, TILE, 1.0)),
                Transform::IDENTITY,
            ),
            BoardView::Isometric => (
                Transform::from_scale(Vec3::new(ISO * 2f32.sqrt(), ISO / 2f32.sqrt(), 1.0)),
                Transform::from_rotation(Quat::from_rotation_z(-FRAC_PI_4)),
            ),
        }
    }
}

/// Where a point in tile coordinates lies on the board's plane.
pub fn on_plane(p: Vec2, z: f32) -> Vec3 {
    Vec3::new(p.x, -p.y, z)
}

/// The board's root: scaled by the view.
#[derive(Component)]
pub struct BoardRoot;

/// The board's plane: turned by the view. Flat things on the board are its
/// children, placed with [`on_plane`].
#[derive(Component)]
pub struct BoardPlane;

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoardView>().add_systems(
            PostUpdate,
            apply_view
                .run_if(resource_changed::<BoardView>)
                .before(TransformSystems::Propagate),
        );
    }
}

fn apply_view(
    view: Res<BoardView>,
    mut roots: Query<&mut Transform, (With<BoardRoot>, Without<BoardPlane>)>,
    mut planes: Query<&mut Transform, (With<BoardPlane>, Without<BoardRoot>)>,
) {
    let (root, plane) = view.transforms();
    for mut transform in &mut roots {
        *transform = root;
    }
    for mut transform in &mut planes {
        *transform = plane;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tile_maps_to_itself_and_back() {
        let row = ".".repeat(100);
        let board = Board::from_rows(&vec![row.as_str(); 100]);
        for view in [BoardView::TopDown, BoardView::Isometric] {
            for tile in board.tiles() {
                assert_eq!(view.pick(&board, view.centre(tile)), Some(tile), "{view:?}");
            }
        }
    }

    #[test]
    fn the_plane_transforms_agree_with_the_projection() {
        for view in [BoardView::TopDown, BoardView::Isometric] {
            let (root, plane) = view.transforms();
            let matrix = root.to_matrix() * plane.to_matrix();
            for p in [
                Vec2::new(0.5, 0.5),
                Vec2::new(12.0, 3.25),
                Vec2::new(99.5, 40.0),
            ] {
                let drawn = matrix.transform_point3(on_plane(p, 0.0)).truncate();
                assert!(drawn.distance(view.project(p)) < 1e-3, "{view:?} {p}");
            }
        }
    }

    #[test]
    fn north_is_up_or_upper_right() {
        let north = Vec2::new(5.0, 4.0);
        let here = Vec2::new(5.0, 5.0);
        let top = BoardView::TopDown.project(north) - BoardView::TopDown.project(here);
        assert!(top.y > 0.0 && top.x == 0.0);
        let iso = BoardView::Isometric.project(north) - BoardView::Isometric.project(here);
        assert!(iso.x > 0.0 && iso.y > 0.0);
    }
}
