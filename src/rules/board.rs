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

/// Rows of hexes lie this far apart, in steps of 50 m: √3 / 2.
pub const ROW: f32 = 0.866_025_4;
/// A hex's radius to its corners, in steps: 1 / √3. Its sides lie half a
/// step from its centre.
pub const RADIUS: f32 = 0.577_350_3;

/// The six directions from a hex to its neighbours, clockwise from the
/// north-east, as axial steps `(q, r)`: `q` grows east, `r` south-east.
const DIRECTIONS: [(i32, i32); 6] = [(1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1)];

/// A tile in board coordinates: a hex standing point up, in column `x` of
/// row `y`. Every odd row is shifted half a tile east, so the board is a
/// rectangle of rows with ragged sides. All six neighbours of a tile are one
/// step of 50 m away and share a side with it (ADR 0008).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

impl Tile {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Axial coordinates `(q, r)`.
    fn axial(self) -> (i32, i32) {
        (self.x - (self.y - (self.y & 1)) / 2, self.y)
    }

    fn from_axial(q: i32, r: i32) -> Self {
        Self::new(q + (r - (r & 1)) / 2, r)
    }

    /// The six neighbours, clockwise from the north-east.
    pub fn neighbours(self) -> [Tile; 6] {
        let (q, r) = self.axial();
        DIRECTIONS.map(|(dq, dr)| Tile::from_axial(q + dq, r + dr))
    }

    /// The tile `n` steps away in direction `d` (0 north-east, clockwise).
    pub fn go(self, d: usize, n: i32) -> Tile {
        let (q, r) = self.axial();
        let (dq, dr) = DIRECTIONS[d % 6];
        Tile::from_axial(q + dq * n, r + dr * n)
    }

    /// Whether `other` is one step away.
    pub fn is_next_to(self, other: Tile) -> bool {
        self.distance(other) == 1
    }

    /// Steps between the two tiles.
    pub fn distance(self, other: Tile) -> i32 {
        let ((q1, r1), (q2, r2)) = (self.axial(), other.axial());
        let (dq, dr) = (q1 - q2, r1 - r2);
        (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
    }

    /// Every tile at most `radius` steps away, this one included, row by row.
    pub fn within(self, radius: i32) -> impl Iterator<Item = Tile> {
        let (q, r) = self.axial();
        (-radius..=radius).flat_map(move |dr| {
            let from = (-radius).max(-dr - radius);
            let to = radius.min(-dr + radius);
            (from..=to).map(move |dq| Tile::from_axial(q + dq, r + dr))
        })
    }

    /// The centre in board units: one unit is a step of 50 m, `x` grows east
    /// and `y` south, and the board's north-west corner is the origin.
    pub fn centre(self) -> Vec2 {
        Vec2::new(
            self.x as f32 + 0.5 + 0.5 * (self.y & 1) as f32,
            self.y as f32 * ROW + RADIUS,
        )
    }

    /// The tile whose hex holds the point, in board units.
    pub fn at(p: Vec2) -> Tile {
        let r = (p.y - RADIUS) / ROW;
        let q = p.x - 0.5 - r / 2.0;
        let (q, r) = round_cube(q, r);
        Tile::from_axial(q, r)
    }

    /// The tiles of the straightest way from here to `to`, one step each,
    /// without this tile and ending at `to`.
    pub fn line_to(self, to: Tile) -> Vec<Tile> {
        let n = self.distance(to);
        let ((q1, r1), (q2, r2)) = (self.axial(), to.axial());
        // Nudged off the corners between hexes, so ties always break alike.
        let (q1, r1) = (q1 as f32 + 1e-4, r1 as f32 + 2e-4);
        let (q2, r2) = (q2 as f32 + 1e-4, r2 as f32 + 2e-4);
        (1..=n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let (q, r) = round_cube(q1 + (q2 - q1) * t, r1 + (r2 - r1) * t);
                Tile::from_axial(q, r)
            })
            .collect()
    }
}

/// How far a point, relative to a hex's centre in board units, lies inside
/// the hex's nearest side; negative outside it.
pub fn to_side(d: Vec2) -> f32 {
    const NORMALS: [Vec2; 3] = [Vec2::X, Vec2::new(0.5, ROW), Vec2::new(-0.5, ROW)];
    0.5 - NORMALS.iter().map(|n| d.dot(*n).abs()).fold(0.0, f32::max)
}

/// The hex nearest fractional axial coordinates.
fn round_cube(q: f32, r: f32) -> (i32, i32) {
    let s = -q - r;
    let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
    let (dq, dr, ds) = ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
    if dq > dr && dq > ds {
        rq = -rr - rs;
    } else if dr > ds {
        rr = -rq - rs;
    }
    (rq as i32, rr as i32)
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
    /// digit a lake of that number. Odd rows may start with a space, so the
    /// text reads as hexes; see [`hex_row`].
    pub fn from_rows(rows: &[&str]) -> Self {
        let rows: Vec<&str> = rows.iter().map(|row| hex_row(row)).collect();
        let height = rows.len() as i32;
        let width = rows.first().map_or(0, |row| row.chars().count()) as i32;
        let mut ground = Vec::new();
        for row in &rows {
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
                for next in self.tile_at(i).neighbours() {
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

    /// The board's width and height in board units (see [`Tile::centre`]):
    /// the odd rows reach half a tile further east.
    pub fn extent(&self) -> Vec2 {
        Vec2::new(
            self.width as f32 + 0.5,
            (self.height - 1).max(0) as f32 * ROW + 2.0 * RADIUS,
        )
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

    /// The region with the most land.
    #[cfg(test)]
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

    /// Whether a Waldmensch may step from `from` to `to`: to any of its six
    /// neighbours on the board. Neighbouring hexes share a side, so no step
    /// slips past a corner.
    pub fn can_step(&self, from: Tile, to: Tile) -> bool {
        self.contains(to) && from.is_next_to(to)
    }

    /// The tiles a Waldmensch may step to from `tile`.
    pub fn steps(&self, tile: Tile) -> impl Iterator<Item = Tile> + '_ {
        tile.neighbours()
            .into_iter()
            .filter(move |&t| self.contains(t))
    }

    /// Whether another rival than `rival` holds this tile or any of its six
    /// neighbours open: such a tile cannot be cut, so a margin of trees always
    /// stays between rivals.
    pub fn in_margin_of_other(&self, tile: Tile, rival: RivalId) -> bool {
        std::iter::once(tile)
            .chain(tile.neighbours())
            .any(|t| self.open(t).is_some_and(|r| r != rival))
    }

    /// Whether the rival's open tiles changed since the last call.
    pub fn take_changed(&mut self, rival: RivalId) -> bool {
        std::mem::take(&mut self.changed[rival.index()])
    }
}

/// A row of a test board as text: a leading space, which odd rows may have so
/// the text reads as hexes, is dropped.
#[cfg(test)]
pub fn hex_row(row: &str) -> &str {
    row.strip_prefix(' ').unwrap_or(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_and_edge_divide_the_land_into_regions() {
        let board = Board::from_rows(&[
            "..~...",  //
            " ..~...", //
            "~~~...",  //
            " ......", //
        ]);
        let corner = board.region(Tile::new(0, 0));
        assert_eq!(board.region_size(corner), 4);
        assert_eq!(board.region(Tile::new(2, 0)), NO_REGION);
        assert_eq!(board.largest_region(), board.region(Tile::new(5, 3)));
        // A brook down the rows zigzags, and still no step slips through.
        let brook = Board::from_rows(&[
            ".~..",  //
            " .~..", //
            ".~..",  //
        ]);
        assert_eq!(brook.region_size(brook.region(Tile::new(0, 0))), 3);
        assert_ne!(brook.region(Tile::new(0, 1)), brook.region(Tile::new(2, 1)));
    }

    #[test]
    fn every_neighbour_is_one_step_away_and_shares_a_side() {
        for tile in [Tile::new(4, 4), Tile::new(4, 5), Tile::new(-3, -1)] {
            let neighbours = tile.neighbours();
            for (k, n) in neighbours.into_iter().enumerate() {
                assert_eq!(tile.distance(n), 1);
                assert!((tile.centre().distance(n.centre()) - 1.0).abs() < 1e-5);
                assert_eq!(tile.go(k, 1), n);
                // Clockwise: each neighbour is next to the one before.
                assert!(n.is_next_to(neighbours[(k + 1) % 6]));
            }
            assert_eq!(tile.within(2).count(), 19);
            assert!(tile.within(2).all(|t| t.distance(tile) <= 2));
            assert_eq!(Tile::at(tile.centre()), tile);
        }
        // A column zigzags down the rows: each tile is next to the one below.
        assert!(Tile::new(3, 0).is_next_to(Tile::new(3, 1)));
        assert!(Tile::new(3, 1).is_next_to(Tile::new(3, 2)));
    }

    #[test]
    fn a_line_between_tiles_takes_one_tile_per_step() {
        let from = Tile::new(1, 2);
        for to in [
            Tile::new(9, 2),
            Tile::new(4, 9),
            Tile::new(0, 7),
            from.go(5, 6),
        ] {
            let line = from.line_to(to);
            assert_eq!(line.len() as i32, from.distance(to));
            assert_eq!(line.last(), Some(&to));
            let mut at = from;
            for &t in &line {
                assert!(at.is_next_to(t), "{to:?}: {line:?}");
                at = t;
            }
        }
        assert!(from.line_to(from).is_empty());
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
