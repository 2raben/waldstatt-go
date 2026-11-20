//! The balance table: every play-test value of the rules in one place.
//!
//! Work is counted in integer units; each fixed step (64 per second) a group
//! adds [`Tuning::speed`] units to the tile it is working towards. Judge changes
//! in the running game, not by the tests alone.

use bevy::prelude::*;

#[derive(Resource, Clone, Debug)]
pub struct Tuning {
    /// Waldleute each rival starts with, each a group of one at a random tile.
    pub start_waldleute: u32,
    /// Work to cut a forest tile.
    pub cut_work: u32,
    /// Work to walk onto a tile the rival has already opened.
    pub walk_work: u32,
    /// Work to wade into a river tile: ten times a cut.
    pub river_work: u32,
    /// Trees a group pays for each river tile it wades into.
    pub river_trees: u32,
    /// Work to build a ship on the shore and launch it: twenty times a cut.
    pub ship_work: u32,
    /// Trees a ship costs.
    pub ship_trees: u32,
    /// Waldleute a ship carries at most.
    pub ship_capacity: u32,
    /// Work to sail one lake tile.
    pub sail_work: u32,
    /// Work a group of one adds per step.
    pub speed_base: u32,
    /// Work every further Waldmensch adds per step.
    pub speed_per_waldmensch: u32,
    /// Steps between new Waldleute (64 steps per second).
    pub waldmensch_interval: u64,
    /// How far behind the one before it each Waldmensch of a moving group may
    /// fall, in tiles along the way.
    pub gap: u32,
    /// Steps a group stands still before its trail starts growing back, so a
    /// ring can be drawn in several lines.
    pub regrow_after: u32,
    /// Then one tile of its trail grows back every this many steps.
    pub regrow_steps: u32,
    /// How far a group sees, in tiles.
    pub view_radius: i32,
    /// A group that can reach fewer tiles than this by cutting is shown as
    /// trapped, and computer rivals try to escape with it.
    pub trapped_room: usize,
    /// A group that can reach fewer tiles than this by cutting, hemmed in by
    /// other rivals, water or the board's edge, cannot split.
    pub split_room: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        let cut_work = 480;
        Self {
            start_waldleute: 5,
            cut_work,
            walk_work: cut_work / 4,
            river_work: cut_work * 10,
            river_trees: 2,
            ship_work: cut_work * 20,
            ship_trees: 8,
            ship_capacity: 10,
            sail_work: cut_work / 2,
            speed_base: 8,
            speed_per_waldmensch: 5,
            waldmensch_interval: 40 * 64,
            gap: 2,
            regrow_after: 8 * 64,
            regrow_steps: 64,
            view_radius: 7,
            trapped_room: 120,
            split_room: 120,
        }
    }
}

impl Tuning {
    /// Work a group adds per step: a bigger group cuts faster.
    pub fn speed(&self, size: u32) -> u32 {
        self.speed_base + self.speed_per_waldmensch * size.saturating_sub(1)
    }
}

/// How long a line a group of `size` may draw, and how many tiles of its trail
/// stay open: `(log2(size) + 1) × 10`, rounded down. Computed exactly in
/// integers as `10 + floor(log2(size^10))`.
pub fn line_limit(size: u32) -> usize {
    if size == 0 {
        return 0;
    }
    // 4096^10 still fits a u128; no game comes near that many Waldleute.
    let power = u128::from(size.min(4096)).pow(10);
    10 + (127 - power.leading_zeros()) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_limit_follows_the_pitch() {
        assert_eq!(line_limit(1), 10);
        assert_eq!(line_limit(2), 20);
        assert_eq!(line_limit(3), 25);
        assert_eq!(line_limit(4), 30);
        assert_eq!(line_limit(5), 33);
        assert_eq!(line_limit(8), 40);
        assert_eq!(line_limit(10), 43);
    }

    #[test]
    fn bigger_groups_cut_faster() {
        let tuning = Tuning::default();
        assert!(tuning.speed(5) > 3 * tuning.speed(1));
        assert!(tuning.speed(5) < 5 * tuning.speed(1));
    }
}
