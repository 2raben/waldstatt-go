//! The two views of the board: top-down and isometric (ADRs 0001 and 0008).
//!
//! The rules know only tiles. A view is a projection from board units (see
//! [`Tile::centre`]) to world coordinates; the pointer picks the hex under it
//! through the same projection, inverted, rounded to the nearest hex. Both
//! views keep north up: the isometric view is the same hex board squashed to
//! half its height, so the flat parts of the board (the ground image, lines
//! and highlights) are children of one plane under a root whose scale is the
//! view, and upright markers are placed with [`BoardView::project`] and drawn
//! back to front by row. Switching views changes only drawing, never the game.

use bevy::prelude::*;

use crate::rules::{Board, Tile};

/// World units per step between hexes in the top-down view.
pub const TILE: f32 = 16.0;
/// World units per step east in the isometric view; a step south is half as
/// long on screen. A hex then covers about as much of the screen as the
/// diamond of a square tile did.
pub const ISO: f32 = 24.0;

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

    /// World units per board unit, east and north.
    fn scale(self) -> Vec2 {
        match self {
            BoardView::TopDown => Vec2::splat(TILE),
            BoardView::Isometric => Vec2::new(ISO, ISO / 2.0),
        }
    }

    /// World position of a point in board units. North is up in both views.
    pub fn project(self, p: Vec2) -> Vec2 {
        Vec2::new(p.x, -p.y) * self.scale()
    }

    /// The inverse of [`BoardView::project`].
    pub fn unproject(self, w: Vec2) -> Vec2 {
        let p = w / self.scale();
        Vec2::new(p.x, -p.y)
    }

    pub fn centre(self, tile: Tile) -> Vec2 {
        self.project(tile.centre())
    }

    /// The tile under a world position, if it is on the board.
    pub fn pick(self, board: &Board, world: Vec2) -> Option<Tile> {
        let tile = Tile::at(self.unproject(world));
        board.contains(tile).then_some(tile)
    }

    /// Draw order of upright markers: further south is nearer, and within a
    /// row further east.
    pub fn depth(self, p: Vec2) -> f32 {
        p.y * 0.001 + p.x * 0.000_001
    }

    /// The transforms of the board's root (scale) and of its plane. Plane
    /// coordinates are board units with y pointing north: `(x, -y)`.
    pub fn transforms(self) -> (Transform, Transform) {
        (
            Transform::from_scale(self.scale().extend(1.0)),
            Transform::IDENTITY,
        )
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
        let board = Board::from_rows(&vec![row.as_str(); 115]);
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
    fn north_is_up_in_both_views() {
        let north = Vec2::new(5.0, 4.0);
        let here = Vec2::new(5.0, 5.0);
        for view in [BoardView::TopDown, BoardView::Isometric] {
            let up = view.project(north) - view.project(here);
            assert!(up.y > 0.0 && up.x == 0.0, "{view:?}");
        }
    }

    #[test]
    fn the_pointer_picks_the_nearest_hex() {
        let row = ".".repeat(20);
        let board = Board::from_rows(&vec![row.as_str(); 20]);
        for view in [BoardView::TopDown, BoardView::Isometric] {
            for tile in board
                .tiles()
                .filter(|t| (2..18).contains(&t.x) && (2..18).contains(&t.y))
            {
                // Points just inside the hex's sides and corners.
                for k in 0..6 {
                    let angle = std::f32::consts::FRAC_PI_3 * k as f32;
                    let side = Vec2::from_angle(angle) * 0.45;
                    let corner = Vec2::from_angle(angle + std::f32::consts::FRAC_PI_6)
                        * (crate::rules::board::RADIUS * 0.9);
                    for p in [side, corner] {
                        let world = view.project(tile.centre() + p);
                        assert_eq!(view.pick(&board, world), Some(tile), "{view:?} {p}");
                    }
                }
            }
        }
    }
}
