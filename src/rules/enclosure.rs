//! Enclosing: a rival's open tiles, the water and the board's edge split the
//! board into parts, and every part but the largest is enclosed by that rival.
//!
//! Rings connect diagonally, while parts are found with straight steps only,
//! so no ring leaks at a corner. A part counts as enclosed only where the
//! rival's tiles split land that water and the edge alone leave whole: a bend
//! of a river or a corner of the board encloses nobody by itself.

use std::collections::{BTreeSet, VecDeque};

use bevy::prelude::*;

use super::board::{Board, NO_REGION, RivalId, Tile};
use super::game::{Game, GameEvent, Rival};
use super::group::{Group, Line, Trail, is_safe_on_water};

/// Tiles that are walls for this rival belong to no part.
const WALL: u32 = u32::MAX;

/// How one rival's open tiles divide the board.
#[derive(Clone, Debug)]
pub struct Partition {
    part: Vec<u32>,
    enclosed: Vec<bool>,
}

impl Partition {
    /// Splits the board by the rival's open tiles, the water and the edge.
    pub fn of(board: &Board, rival: RivalId) -> Self {
        let mut part = vec![WALL; board.len()];
        let mut sizes: Vec<usize> = Vec::new();
        let mut regions: Vec<u32> = Vec::new();
        let is_wall = |tile: Tile| board.is_water(tile) || board.open(tile) == Some(rival);
        let mut stack = Vec::new();
        for start in board.tiles() {
            let Some(i) = board.index(start) else {
                continue;
            };
            if part[i] != WALL || is_wall(start) {
                continue;
            }
            let id = sizes.len() as u32;
            part[i] = id;
            stack.push(start);
            let mut size = 0;
            while let Some(tile) = stack.pop() {
                size += 1;
                for next in tile.neighbours4() {
                    if let Some(j) = board.index(next)
                        && part[j] == WALL
                        && !is_wall(next)
                    {
                        part[j] = id;
                        stack.push(next);
                    }
                }
            }
            sizes.push(size);
            regions.push(board.region(start));
        }
        // The largest parts in each land region are free; ties stay free too.
        let mut largest: Vec<(u32, usize)> = Vec::new();
        for (&region, &size) in regions.iter().zip(&sizes) {
            match largest.iter_mut().find(|(r, _)| *r == region) {
                Some(entry) => entry.1 = entry.1.max(size),
                None => largest.push((region, size)),
            }
        }
        let enclosed = regions
            .iter()
            .zip(&sizes)
            .map(|(region, &size)| {
                *region != NO_REGION && largest.iter().any(|&(r, most)| r == *region && size < most)
            })
            .collect();
        Self { part, enclosed }
    }

    /// The part a tile belongs to; `None` for the rival's walls.
    pub fn part(&self, board: &Board, tile: Tile) -> Option<u32> {
        board
            .index(tile)
            .map(|i| self.part[i])
            .filter(|&p| p != WALL)
    }

    /// Whether the tile lies in a part this rival encloses.
    pub fn encloses(&self, board: &Board, tile: Tile) -> bool {
        self.part(board, tile)
            .is_some_and(|p| self.enclosed[p as usize])
    }

    /// The tiles of a part.
    pub fn tiles_of(&self, board: &Board, part: u32) -> Vec<Tile> {
        board
            .tiles()
            .filter(|&t| self.part(board, t) == Some(part))
            .collect()
    }
}

/// The last partition of each rival, tested again only when its open tiles
/// changed.
#[derive(Component, Default, Debug)]
pub struct Enclosures(pub Option<Partition>);

/// Finds rings that closed this step and turns every other rival's group
/// inside to the rival that closed it, with its Waldleute, trees and trail.
/// Rivals are tested in the game's order; a group changes sides at most once
/// per step, so rings closing round each other at once resolve in that order.
pub fn resolve_enclosures(
    mut board: ResMut<Board>,
    mut game: ResMut<Game>,
    mut rivals: Query<(&mut Rival, &mut Enclosures)>,
    mut groups: Query<(&mut Group, &mut Trail, &mut Line)>,
) {
    let order = game.order.clone();
    let mut changed_sides = BTreeSet::new();
    loop {
        let mut any = false;
        for &rival in &order {
            let Some((_, mut cache)) = rivals.iter_mut().find(|(r, _)| r.id == rival && !r.out)
            else {
                continue;
            };
            if board.take_changed(rival) || cache.0.is_none() {
                cache.0 = Some(Partition::of(&board, rival));
            }
            let Some(partition) = cache.0.clone() else {
                continue;
            };
            let mut caught: Vec<_> = groups
                .iter_mut()
                .filter(|(group, ..)| {
                    group.rival != rival
                        && !changed_sides.contains(&group.id)
                        && !is_safe_on_water(&board, group)
                        && partition.encloses(&board, group.tile)
                })
                .collect();
            caught.sort_by_key(|(group, ..)| (game.rank(group.rival), group.id));
            for (mut group, trail, mut line) in caught {
                let Some(part) = partition.part(&board, group.tile) else {
                    continue;
                };
                let tiles = partition.tiles_of(&board, part);
                let ring = ring_round(&board, rival, &tiles);
                let from = group.rival;
                group.rival = rival;
                group.progress = 0;
                line.0.clear();
                for tile in trail.tiles() {
                    board.set_open(tile, Some(rival));
                }
                changed_sides.insert(group.id);
                let step = game.step;
                game.events.push(GameEvent::Enclosed {
                    step,
                    by: rival,
                    from,
                    group: group.id,
                    size: group.size,
                    part: tiles,
                    ring,
                });
                any = true;
            }
        }
        if !any {
            break;
        }
    }
    if changed_sides.is_empty() {
        return;
    }
    // Receivers that changed sides fall back to the rival's largest group.
    for (mut rival, _) in &mut rivals {
        let id = rival.id;
        if let Some(receiver) = rival.receiver
            && !groups
                .iter()
                .any(|(g, ..)| g.id == receiver && g.rival == id)
        {
            rival.receiver = None;
        }
    }
}

/// The rival's open tiles that touch the part, straight or diagonally: the
/// ring that closed round it.
pub fn ring_round(board: &Board, rival: RivalId, part: &[Tile]) -> Vec<Tile> {
    let mut ring = BTreeSet::new();
    for tile in part {
        for next in tile.neighbours8() {
            if board.open(next) == Some(rival) {
                ring.insert(next);
            }
        }
    }
    ring.into_iter().collect()
}

/// How many tiles a group of `rival` at `start` can reach by cutting and
/// walking, counted up to `cap`. Tiles in another rival's margin cannot be cut,
/// so a ring that is about to close traps a group before it closes. `sees`
/// limits what is known of other rivals' tiles to what the observer sees.
pub fn room(
    board: &Board,
    rival: RivalId,
    start: Tile,
    cap: usize,
    sees: impl Fn(Tile) -> bool,
) -> usize {
    let open_to = |tile: Tile| -> bool {
        if !board.contains(tile) || board.is_water(tile) {
            return false;
        }
        let other = |t: Tile| sees(t) && board.open(t).is_some_and(|r| r != rival);
        match board.open(tile) {
            Some(r) if r == rival => true,
            _ => !other(tile) && !tile.neighbours8().into_iter().any(other),
        }
    };
    let mut seen = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(tile) = queue.pop_front() {
        if seen.len() >= cap {
            break;
        }
        for next in tile.neighbours4() {
            if !seen.contains(&next) && open_to(next) {
                seen.insert(next);
                queue.push_back(next);
            }
        }
    }
    seen.len().min(cap)
}

/// Whether a group is trapped: it can reach less than `cap` tiles and less than
/// its whole land region, so a ring is closing round it.
pub fn is_trapped(board: &Board, group: &Group, cap: usize, sees: impl Fn(Tile) -> bool) -> bool {
    if board.is_water(group.tile) {
        return false;
    }
    let region = board.region_size(board.region(group.tile));
    let reached = room(board, group.rival, group.tile, cap, sees);
    reached < cap && reached < region
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: RivalId = RivalId(0);

    fn board_with(rows: &[&str]) -> Board {
        // `#` marks A's open tiles on land.
        let ground: Vec<String> = rows.iter().map(|r| r.replace('#', ".")).collect();
        let refs: Vec<&str> = ground.iter().map(String::as_str).collect();
        let mut board = Board::from_rows(&refs);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if c == '#' {
                    board.set_open(Tile::new(x as i32, y as i32), Some(A));
                }
            }
        }
        board
    }

    #[test]
    fn a_ring_encloses_its_inside() {
        let board = board_with(&[
            "........", //
            "..###...", //
            "..#.#...", //
            "..###...", //
            "........", //
        ]);
        let p = Partition::of(&board, A);
        assert!(p.encloses(&board, Tile::new(3, 2)));
        assert!(!p.encloses(&board, Tile::new(0, 0)));
        assert!(p.part(&board, Tile::new(2, 1)).is_none());
    }

    #[test]
    fn a_diagonal_ring_holds() {
        let board = board_with(&[
            ".......", //
            "...#...", //
            "..#.#..", //
            "...#...", //
            ".......", //
        ]);
        let p = Partition::of(&board, A);
        assert!(p.encloses(&board, Tile::new(3, 2)));
    }

    #[test]
    fn a_ring_with_a_gap_encloses_nothing() {
        let board = board_with(&[
            "........", //
            "..###...", //
            "..#.....", //
            "..###...", //
            "........", //
        ]);
        let p = Partition::of(&board, A);
        assert!(!p.encloses(&board, Tile::new(3, 2)));
    }

    #[test]
    fn water_and_edge_close_rings_with_the_rival() {
        let board = board_with(&[
            "..#.~...", //
            "..#.~...", //
            "..###...", //
            "........", //
            "........", //
        ]);
        let p = Partition::of(&board, A);
        // Between the trail, the river and the board's top edge.
        assert!(p.encloses(&board, Tile::new(3, 0)));
        assert!(!p.encloses(&board, Tile::new(0, 4)));
    }

    #[test]
    fn water_alone_encloses_nobody() {
        let board = board_with(&[
            "..~.....", //
            "~~~.....", //
            "......#.", //
        ]);
        let p = Partition::of(&board, A);
        assert!(!p.encloses(&board, Tile::new(0, 0)));
    }

    #[test]
    fn a_closing_ring_traps_before_it_closes() {
        let row = ".".repeat(12);
        let mut board = Board::from_rows(&[row.as_str(); 9]);
        let b = RivalId(1);
        // B's ring round (5, 4) with a gap of one tile at (8, 4).
        for x in 2..=8 {
            board.set_open(Tile::new(x, 1), Some(b));
            board.set_open(Tile::new(x, 7), Some(b));
        }
        for y in 1..=7 {
            board.set_open(Tile::new(2, y), Some(b));
            if y != 4 {
                board.set_open(Tile::new(8, y), Some(b));
            }
        }
        let room = room(&board, A, Tile::new(5, 4), 100, |_| true);
        assert!(room < 30, "only the inside is reachable: {room}");
    }
}
