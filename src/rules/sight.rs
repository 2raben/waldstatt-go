//! What each rival sees. Round each of its groups the board is clear; what a
//! rival has seen before stays known as ground and water. Visibility is part of
//! the rules, not of the drawing: computer rivals see only this, and the
//! player's fog shows the same (ADR 0002).

use bevy::prelude::*;

use super::board::{Board, RivalId, Tile};
use super::game::Rival;
use super::group::Group;
use super::tuning::Tuning;

#[derive(Component, Clone, Debug, Default)]
pub struct Sight {
    visible: Vec<bool>,
    seen: Vec<bool>,
}

impl Sight {
    pub fn new(board: &Board) -> Self {
        Self {
            visible: vec![false; board.len()],
            seen: vec![false; board.len()],
        }
    }

    /// Whether the rival sees the tile now, with other rivals' groups and trails.
    pub fn sees(&self, board: &Board, tile: Tile) -> bool {
        board.index(tile).is_some_and(|i| self.visible[i])
    }

    /// Whether the rival has ever seen the tile.
    pub fn has_seen(&self, board: &Board, tile: Tile) -> bool {
        board.index(tile).is_some_and(|i| self.seen[i])
    }

    /// Clears the board round the given tiles.
    pub fn look(
        &mut self,
        board: &Board,
        rival: RivalId,
        from: impl Iterator<Item = Tile>,
        radius: i32,
    ) {
        self.visible.fill(false);
        for centre in from {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if dx * dx + dy * dy > radius * radius {
                        continue;
                    }
                    if let Some(i) = board.index(Tile::new(centre.x + dx, centre.y + dy)) {
                        self.visible[i] = true;
                        self.seen[i] = true;
                    }
                }
            }
        }
        // A rival knows its own open tiles wherever they are.
        for (i, seen) in self.seen.iter_mut().enumerate() {
            if board.open(board.tile_at(i)) == Some(rival) {
                *seen = true;
            }
        }
    }
}

pub fn update_sight(
    board: Res<Board>,
    tuning: Res<Tuning>,
    mut rivals: Query<(&Rival, &mut Sight)>,
    groups: Query<&Group>,
) {
    for (rival, mut sight) in &mut rivals {
        let tiles = groups
            .iter()
            .filter(|g| g.rival == rival.id)
            .flat_map(|g| g.tiles().collect::<Vec<_>>());
        sight.look(&board, rival.id, tiles, tuning.view_radius);
    }
}
