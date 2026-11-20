//! Enclosing: a rival's open tiles, the water and the board's edge split the
//! board into parts, and every part but the largest is enclosed by that rival.
//!
//! Rings connect diagonally, while parts are found with straight steps only,
//! so no ring leaks at a corner. A part counts as enclosed only where the
//! rival's tiles split land that water and the edge alone leave whole: a bend
//! of a river or a corner of the board encloses nobody by itself.
//!
//! A ring round a group's clearing takes its Waldleute: they join the group of
//! the rival that closed it. Waldleute in a river, or already across it, stay
//! free. A ring round a lake, with the rivers and the edge, takes those in a
//! boat on it (ADR 0007).

use std::collections::BTreeSet;
use std::collections::VecDeque;

use bevy::prelude::*;

use super::board::{Board, Ground, NO_REGION, RivalId, Tile};
use super::game::{Game, GameEvent, Rival};
use super::group::{Group, Line, Trail, merge_trails};

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
        Self::with(board, rival, Ground::is_water)
    }

    /// Splits the board by the rival's open tiles, the rivers and the edge,
    /// so lakes lie in parts: for taking boats.
    pub fn round_lakes(board: &Board, rival: RivalId) -> Self {
        Self::with(board, rival, |g| g == Ground::River)
    }

    fn with(board: &Board, rival: RivalId, water_wall: impl Fn(Ground) -> bool) -> Self {
        let regions = fill(board, |t| water_wall(board.ground(t)));
        let (part, sizes) = parts(board, |t| {
            water_wall(board.ground(t)) || board.open(t) == Some(rival)
        });
        let mut part_region = vec![NO_REGION; sizes.len()];
        for (i, &p) in part.iter().enumerate() {
            if p != WALL {
                part_region[p as usize] = regions[i];
            }
        }
        // The largest parts in each region are free; ties stay free too.
        let mut largest: Vec<(u32, usize)> = Vec::new();
        for (&region, &size) in part_region.iter().zip(&sizes) {
            match largest.iter_mut().find(|(r, _)| *r == region) {
                Some(entry) => entry.1 = entry.1.max(size),
                None => largest.push((region, size)),
            }
        }
        let enclosed = part_region
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

/// Numbers the pieces the walls leave, by straight steps; walls get [`WALL`].
fn parts(board: &Board, is_wall: impl Fn(Tile) -> bool) -> (Vec<u32>, Vec<usize>) {
    let mut part = vec![WALL; board.len()];
    let mut sizes: Vec<usize> = Vec::new();
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
    }
    (part, sizes)
}

/// The regions the given water and the edge alone divide the board into.
fn fill(board: &Board, is_wall: impl Fn(Tile) -> bool) -> Vec<u32> {
    let (part, _) = parts(board, is_wall);
    part.into_iter()
        .map(|p| if p == WALL { NO_REGION } else { p })
        .collect()
}

/// The last partition of each rival, tested again only when its open tiles
/// changed.
#[derive(Component, Default, Debug)]
pub struct Enclosures(pub Option<Partition>);

/// Which of a group's Waldleute a rival's rings take: those on land in an
/// enclosed part, and those on a lake ringed with the rivers and the edge.
fn caught(board: &Board, group: &Group, land: &Partition, lakes: Option<&Partition>) -> Vec<usize> {
    (0..group.members.len())
        .filter(|&k| {
            let tile = group.members[k].tile;
            match board.ground(tile) {
                Ground::Land => land.encloses(board, tile),
                Ground::Lake(_) => lakes.is_some_and(|p| p.encloses(board, tile)),
                Ground::River => false,
            }
        })
        .collect()
}

/// Finds rings that closed this step. The Waldleute they take join the
/// group of the rival that closed it with the most tiles in the ring, with
/// their share of the trees and the trail inside. Rivals are tested in the
/// game's order; a group loses Waldleute at most once per step, so rings
/// closing round each other at once resolve in that order.
pub fn resolve_enclosures(
    mut commands: Commands,
    mut board: ResMut<Board>,
    mut game: ResMut<Game>,
    mut rivals: Query<(&mut Rival, &mut Enclosures)>,
    mut groups: Query<(Entity, &mut Group, &mut Trail, &mut Line)>,
) {
    let order = game.order.clone();
    let mut taken = BTreeSet::new();
    let mut gone = BTreeSet::new();
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
            let Some(land) = cache.0.clone() else {
                continue;
            };
            let afloat = groups.iter().any(|(_, g, ..)| {
                g.rival != rival
                    && g.tiles()
                        .any(|t| matches!(board.ground(t), Ground::Lake(_)))
            });
            let lakes = afloat.then(|| Partition::round_lakes(&board, rival));
            let mut victims: Vec<(usize, Entity, Vec<usize>)> = groups
                .iter()
                .filter(|(e, g, ..)| {
                    g.rival != rival && !taken.contains(&g.id) && !gone.contains(e)
                })
                .map(|(e, g, ..)| {
                    (
                        game.rank(g.rival),
                        e,
                        caught(&board, g, &land, lakes.as_ref()),
                    )
                })
                .filter(|(.., k)| !k.is_empty())
                .collect();
            victims.sort_by_key(|(rank, e, _)| (*rank, groups.get(*e).map(|(_, g, ..)| g.id).ok()));
            for (_, victim, keys) in victims {
                let Ok((_, group, ..)) = groups.get(victim) else {
                    continue;
                };
                // The parts the taken Waldleute stand in, and the ring round them.
                let mut part_ids = BTreeSet::new();
                for &k in &keys {
                    let tile = group.members[k].tile;
                    let p = match board.ground(tile) {
                        Ground::Land => land.part(&board, tile).map(|p| (false, p)),
                        _ => lakes
                            .as_ref()
                            .and_then(|l| l.part(&board, tile))
                            .map(|p| (true, p)),
                    };
                    part_ids.extend(p);
                }
                let mut tiles = BTreeSet::new();
                for &(lake, p) in &part_ids {
                    let partition = if lake { lakes.as_ref() } else { Some(&land) };
                    if let Some(partition) = partition {
                        tiles.extend(partition.tiles_of(&board, p));
                    }
                }
                let tiles: Vec<Tile> = tiles.into_iter().collect();
                let ring = ring_round(&board, rival, &tiles);
                // The encloser's group with the most of the ring.
                let capturer = groups
                    .iter()
                    .filter(|(e, g, ..)| g.rival == rival && !gone.contains(e))
                    .max_by_key(|(_, g, trail, _)| {
                        let held = ring
                            .iter()
                            .filter(|&&t| trail.contains(t) || g.tiles().any(|m| m == t))
                            .count();
                        (held, std::cmp::Reverse(g.id))
                    })
                    .map(|(e, ..)| e);
                let Some(capturer) = capturer else {
                    continue;
                };
                let Ok(
                    [
                        (_, mut from, mut from_trail, mut from_line),
                        (_, mut into, mut into_trail, _),
                    ],
                ) = groups.get_many_mut([victim, capturer])
                else {
                    continue;
                };
                let size = from.size();
                let trees = from.trees * keys.len() as u32 / size;
                let inside: BTreeSet<Tile> = tiles.iter().copied().collect();
                let mut moved = Trail::default();
                from_trail.0.retain(|&(t, s)| {
                    if inside.contains(&t) {
                        moved.0.push_back((t, s));
                        false
                    } else {
                        true
                    }
                });
                for t in moved.tiles() {
                    board.set_open(t, Some(rival));
                }
                *into_trail = merge_trails(&into_trail, &moved);
                let lead_taken = keys.contains(&from.lead);
                let lead_tile = from.tile();
                let mut kept = Vec::new();
                for (k, member) in from.members.iter().enumerate() {
                    if keys.contains(&k) {
                        into.members.push(*member);
                    } else {
                        kept.push(*member);
                    }
                }
                from.trees -= trees;
                into.trees += trees;
                from.members = kept;
                let event = GameEvent::Enclosed {
                    step: game.step,
                    by: rival,
                    from: from.rival,
                    group: from.id,
                    into: into.id,
                    size: keys.len() as u32,
                    part: tiles,
                    ring,
                };
                taken.insert(from.id);
                if from.members.is_empty() {
                    gone.insert(victim);
                    commands.entity(victim).despawn();
                } else if lead_taken {
                    from.lead = 0;
                    from_line.0.clear();
                    from.target = from.tile();
                } else {
                    from.lead = from.member_on(lead_tile).unwrap_or(0);
                }
                game.events.push(event);
                any = true;
            }
        }
        if !any {
            break;
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
    let tile = group.tile();
    if board.is_water(tile) {
        return false;
    }
    let region = board.region_size(board.region(tile));
    let reached = room(board, group.rival, tile, cap, sees);
    reached < cap && reached < region
}

/// Whether a group may split: it has two Waldleute or more and can reach at
/// least `cap` tiles by cutting from its lead, so no other rival, water or the
/// board's edge hems it in. A group on the water cannot split.
pub fn can_split(board: &Board, group: &Group, cap: usize, sees: impl Fn(Tile) -> bool) -> bool {
    let tile = group.tile();
    group.size() >= 2 && !board.is_water(tile) && room(board, group.rival, tile, cap, sees) >= cap
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
