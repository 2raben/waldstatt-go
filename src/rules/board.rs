//! The board: tiles of 50 m cut from the real map, their ground, and which
//! rival has opened each land tile.
//!
//! Board coordinates start at the north-west corner; `x` grows east and `y`
//! south, as in the Waldstatt. Every land tile starts as forest.

use bevy::prelude::*;

use super::rng::tile_hash;

/// How many rivals a game can have.
pub const MAX_RIVALS: usize = 4;

/// A rival's seat at the board, `0..MAX_RIVALS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RivalId(pub u8);

impl RivalId {
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// A tile in board coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// North, east, south, west: the straight steps groups take.
    pub fn neighbours4(self) -> [Tile; 4] {
        let Tile { x, y } = self;
        [
            Tile::new(x, y - 1),
            Tile::new(x + 1, y),
            Tile::new(x, y + 1),
            Tile::new(x - 1, y),
        ]
    }

    /// All eight neighbours, clockwise from the north.
    pub fn neighbours8(self) -> [Tile; 8] {
        let Tile { x, y } = self;
        [
            Tile::new(x, y - 1),
            Tile::new(x + 1, y - 1),
            Tile::new(x + 1, y),
            Tile::new(x + 1, y + 1),
            Tile::new(x, y + 1),
            Tile::new(x - 1, y + 1),
            Tile::new(x - 1, y),
            Tile::new(x - 1, y - 1),
        ]
    }

    /// Whether `other` is one straight step away.
    pub fn is_next_to(self, other: Tile) -> bool {
        (self.x - other.x).abs() + (self.y - other.y).abs() == 1
    }

    /// Steps along the larger axis, counting diagonals as one.
    pub fn chebyshev(self, other: Tile) -> i32 {
        (self.x - other.x).abs().max((self.y - other.y).abs())
    }

    /// Straight steps between the two tiles.
    pub fn manhattan(self, other: Tile) -> i32 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }
}

/// What a tile is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    Land,
    /// A river or brook, crossed by wading.
    River,
    /// A lake, by the Waldstatt's lake number; crossed by ship.
    Lake(u16),
}

impl Ground {
    pub fn is_water(self) -> bool {
        !matches!(self, Ground::Land)
    }
}

/// A water's name, placed where the Waldstatt labels it.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterLabel {
    pub name: String,
    pub tile: Tile,
    pub lake: bool,
}

/// Water tiles belong to no land region.
pub const NO_REGION: u32 = u32::MAX;

#[derive(Resource, Clone, Debug)]
pub struct Board {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub labels: Vec<WaterLabel>,
    ground: Vec<Ground>,
    /// The land regions that water and the board's edge alone divide the board
    /// into, numbered from 0, and their sizes.
    region: Vec<u32>,
    region_sizes: Vec<usize>,
    trees: Vec<u8>,
    open: Vec<Option<RivalId>>,
    /// Rivals whose open tiles changed since their enclosures were last tested.
    changed: [bool; MAX_RIVALS],
}

impl Board {
    /// A board of the given ground, all forest, with one tree per land tile
    /// until [`Board::grow_forest`] sows it from a seed.
    pub fn new(
        name: impl Into<String>,
        width: i32,
        height: i32,
        ground: Vec<Ground>,
        labels: Vec<WaterLabel>,
    ) -> Self {
        assert_eq!(
            ground.len(),
            (width * height) as usize,
            "ground fits the board"
        );
        let len = ground.len();
        let mut board = Self {
            name: name.into(),
            width,
            height,
            labels,
            ground,
            region: vec![NO_REGION; len],
            region_sizes: Vec::new(),
            trees: vec![1; len],
            open: vec![None; len],
            changed: [false; MAX_RIVALS],
        };
        board.find_regions();
        board
    }

    #[cfg(test)]
    /// A board from rows of text, for tests: `.` is land, `~` a river and a
    /// digit a lake of that number.
    pub fn from_rows(rows: &[&str]) -> Self {
        let height = rows.len() as i32;
        let width = rows.first().map_or(0, |row| row.chars().count()) as i32;
        let mut ground = Vec::new();
        for row in rows {
            assert_eq!(row.chars().count() as i32, width, "rows have one width");
            for c in row.chars() {
                ground.push(match c {
                    '~' => Ground::River,
                    d if d.is_ascii_digit() => Ground::Lake(d as u16 - '0' as u16),
                    _ => Ground::Land,
                });
            }
        }
        Self::new("Test", width, height, ground, Vec::new())
    }

    /// Sows the forest: every land tile yields one to three trees, fixed by the seed.
    pub fn grow_forest(&mut self, seed: u64) {
        for i in 0..self.ground.len() {
            let tile = self.tile_at(i);
            self.trees[i] = 1 + (tile_hash(seed, tile.x, tile.y) % 3) as u8;
        }
    }

    fn find_regions(&mut self) {
        self.region_sizes.clear();
        let mut stack = Vec::new();
        for start in 0..self.ground.len() {
            if self.ground[start].is_water() || self.region[start] != NO_REGION {
                continue;
            }
            let id = self.region_sizes.len() as u32;
            let mut size = 0;
            self.region[start] = id;
            stack.push(start);
            while let Some(i) = stack.pop() {
                size += 1;
                for next in self.tile_at(i).neighbours4() {
                    if let Some(j) = self.index(next)
                        && !self.ground[j].is_water()
                        && self.region[j] == NO_REGION
                    {
                        self.region[j] = id;
                        stack.push(j);
                    }
                }
            }
            self.region_sizes.push(size);
        }
    }

    pub fn len(&self) -> usize {
        self.ground.len()
    }

    pub fn contains(&self, tile: Tile) -> bool {
        (0..self.width).contains(&tile.x) && (0..self.height).contains(&tile.y)
    }

    pub fn index(&self, tile: Tile) -> Option<usize> {
        self.contains(tile)
            .then(|| (tile.y * self.width + tile.x) as usize)
    }

    pub fn tile_at(&self, index: usize) -> Tile {
        let index = index as i32;
        Tile::new(index % self.width, index / self.width)
    }

    /// Every tile, row by row from the north-west.
    pub fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        (0..self.len()).map(|i| self.tile_at(i))
    }

    /// The ground; outside the board, land (callers check `contains`).
    pub fn ground(&self, tile: Tile) -> Ground {
        self.index(tile).map_or(Ground::Land, |i| self.ground[i])
    }

    pub fn is_water(&self, tile: Tile) -> bool {
        self.ground(tile).is_water()
    }

    /// The land region of a tile, or [`NO_REGION`] for water and outside.
    pub fn region(&self, tile: Tile) -> u32 {
        self.index(tile).map_or(NO_REGION, |i| self.region[i])
    }

    pub fn region_size(&self, region: u32) -> usize {
        self.region_sizes.get(region as usize).copied().unwrap_or(0)
    }

    /// The region with the most land: where rivals start.
    pub fn largest_region(&self) -> u32 {
        (0..self.region_sizes.len() as u32)
            .max_by_key(|&r| (self.region_sizes[r as usize], std::cmp::Reverse(r)))
            .unwrap_or(NO_REGION)
    }

    /// Trees a forest tile yields when cut.
    pub fn trees(&self, tile: Tile) -> u32 {
        self.index(tile).map_or(0, |i| u32::from(self.trees[i]))
    }

    /// The rival that holds this tile open, if any; `None` is forest or water.
    pub fn open(&self, tile: Tile) -> Option<RivalId> {
        self.index(tile).and_then(|i| self.open[i])
    }

    /// Opens a tile for a rival, or lets it grow back with `None`.
    pub fn set_open(&mut self, tile: Tile, rival: Option<RivalId>) {
        let Some(i) = self.index(tile) else {
            return;
        };
        let before = self.open[i];
        if before == rival {
            return;
        }
        for changed in [before, rival].into_iter().flatten() {
            self.changed[changed.index()] = true;
        }
        self.open[i] = rival;
    }

    /// Whether another rival than `rival` holds this tile or any of its eight
    /// neighbours open: such a tile cannot be cut, so a margin of trees always
    /// stays between rivals.
    pub fn in_margin_of_other(&self, tile: Tile, rival: RivalId) -> bool {
        std::iter::once(tile)
            .chain(tile.neighbours8())
            .any(|t| self.open(t).is_some_and(|r| r != rival))
    }

    /// Whether the rival's open tiles changed since the last call.
    pub fn take_changed(&mut self, rival: RivalId) -> bool {
        std::mem::take(&mut self.changed[rival.index()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_and_edge_divide_the_land_into_regions() {
        let board = Board::from_rows(&[
            "..~...", //
            "..~...", //
            "~~~...", //
            "......", //
        ]);
        let corner = board.region(Tile::new(0, 0));
        assert_eq!(board.region_size(corner), 4);
        assert_eq!(board.region(Tile::new(2, 0)), NO_REGION);
        assert_eq!(board.largest_region(), board.region(Tile::new(5, 3)));
        // A diagonal of water holds against straight steps.
        let diagonal = Board::from_rows(&[
            ".~..", //
            "~...", //
            "....", //
        ]);
        assert_ne!(
            diagonal.region(Tile::new(0, 0)),
            diagonal.region(Tile::new(1, 1))
        );
    }

    #[test]
    fn opening_tiles_marks_the_rivals_changed() {
        let mut board = Board::from_rows(&["...."]);
        let (a, b) = (RivalId(0), RivalId(1));
        board.set_open(Tile::new(0, 0), Some(a));
        assert!(board.take_changed(a));
        assert!(!board.take_changed(a));
        assert!(!board.take_changed(b));
        assert!(board.in_margin_of_other(Tile::new(1, 0), b));
        assert!(!board.in_margin_of_other(Tile::new(1, 0), a));
        assert!(!board.in_margin_of_other(Tile::new(2, 0), b));
    }
}
