//! Finding a way: the fastest line for a group's lead to a tile, as its rival
//! knows the board.
//!
//! A line found this way is like a drawn one: steps to a neighbour, each one
//! tile, no more tiles than the group may go. It costs what the lead's work
//! would: walking the rival's own clearing, cutting forest, wading, launching
//! and sailing. Another rival's clearing and its margin block only where the
//! rival sees them now; land it has never seen counts as forest. So a way
//! may turn out blocked on the spot, and the lead then looks for another.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use super::board::{Board, Ground, Tile};
use super::group::Group;
use super::sight::Sight;
use super::tuning::Tuning;

/// How many crossing tiles of trees a group wants before it plans to wade.
const CROSSING: u32 = 3;
/// The most tiles one search looks at.
const SEARCH_LIMIT: usize = 6000;
/// Work is counted this many times finer in a search, to break ties.
const SCALE: u32 = 16;
/// The most a step pays for straying from the straight way, in those units.
const STRAY: f32 = 8.0;

/// The fastest way from the group's lead to `to`, at most `budget` tiles,
/// never through tiles where `avoid` holds unless it holds at `to`; empty if
/// there is none.
pub fn route(
    board: &Board,
    tuning: &Tuning,
    group: &Group,
    sight: Option<&Sight>,
    to: Tile,
    avoid: impl Fn(Tile) -> bool,
    budget: u32,
) -> VecDeque<Tile> {
    let from = group.tile();
    let rival = group.rival;
    let seen = |t: Tile| sight.is_none_or(|s| s.has_seen(board, t));
    let sees = |t: Tile| sight.is_none_or(|s| s.sees(board, t));
    let ground = |t: Tile| {
        if seen(t) {
            board.ground(t)
        } else {
            Ground::Land
        }
    };
    if from == to || !board.contains(to) || ground(to) == Ground::River || budget == 0 {
        return VecDeque::new();
    }
    let wade = group.trees >= tuning.river_trees * CROSSING;
    let sail = group.size() <= tuning.ship_capacity && group.trees >= tuning.ship_trees;
    let others = |t: Tile| sees(t) && board.open(t).is_some_and(|r| r != rival);
    let avoiding = !avoid(to);
    let cost = |a: Tile, b: Tile| -> Option<u32> {
        if avoiding && avoid(b) {
            return None;
        }
        match (ground(a), ground(b)) {
            (_, Ground::Land) => {
                if board.open(b) == Some(rival) {
                    Some(tuning.walk_work)
                } else if others(b) || b.neighbours().into_iter().any(others) {
                    None
                } else {
                    Some(tuning.cut_work)
                }
            }
            (Ground::River, Ground::River) => Some(tuning.river_work),
            (_, Ground::River) => wade.then_some(tuning.river_work),
            (Ground::Lake(_), Ground::Lake(_)) => Some(tuning.sail_work),
            (_, Ground::Lake(_)) => sail.then_some(tuning.ship_work),
        }
    };
    // A* over the board, never underestimating: no step costs less than a walk.
    // Costs are scaled, and a step pays a little for how far it strays from
    // the straight way, so of ways equally fast the straightest wins; the
    // most it pays is far less than any difference in work.
    let least = tuning.walk_work.min(tuning.sail_work) * SCALE;
    let guess = |t: Tile| t.distance(to) as u32 * least;
    let (a, b) = (from.centre(), to.centre());
    let across = (b - a).normalize_or_zero().perp();
    let stray = |t: Tile| ((t.centre() - a).dot(across).abs() * 2.0).min(STRAY) as u32;
    let mut spent = vec![u32::MAX; board.len()];
    let mut before = vec![usize::MAX; board.len()];
    let mut done = vec![false; board.len()];
    let (Some(start), Some(goal)) = (board.index(from), board.index(to)) else {
        return VecDeque::new();
    };
    spent[start] = 0;
    let mut heap = BinaryHeap::from([Reverse((guess(from), 0u32, from))]);
    let mut looked = 0;
    while let Some(Reverse((_, c, tile))) = heap.pop() {
        let Some(i) = board.index(tile) else {
            continue;
        };
        if done[i] {
            continue;
        }
        done[i] = true;
        looked += 1;
        if i == goal || looked > SEARCH_LIMIT {
            break;
        }
        for next in board.steps(tile) {
            let Some(j) = board.index(next) else {
                continue;
            };
            if done[j] {
                continue;
            }
            if let Some(step) = cost(tile, next).map(|s| s * SCALE + stray(next))
                && c + step < spent[j]
            {
                spent[j] = c + step;
                before[j] = i;
                heap.push(Reverse((c + step + guess(next), c + step, next)));
            }
        }
    }
    if !done[goal] {
        return VecDeque::new();
    }
    let mut way = VecDeque::new();
    let mut at = goal;
    while at != start {
        way.push_front(board.tile_at(at));
        at = before[at];
    }
    way.truncate(budget as usize);
    // A line never ends in a river.
    while way
        .back()
        .is_some_and(|&t| board.ground(t) == Ground::River)
    {
        way.pop_back();
    }
    way
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::board::RivalId;
    use crate::rules::group::GroupId;

    #[test]
    fn the_way_goes_straight_and_round_another_rivals_clearing() {
        let row = ".".repeat(12);
        let mut board = Board::from_rows(&[row.as_str(); 12]);
        let tuning = Tuning::default();
        let group = Group::new(GroupId(1), RivalId(0), Tile::new(0, 0));
        let way = route(
            &board,
            &tuning,
            &group,
            None,
            Tile::new(5, 5),
            |_| false,
            30,
        );
        assert_eq!(way.len(), 8, "one tile per step: {way:?}");
        let line = Tile::new(0, 0).line_to(Tile::new(5, 5));
        let off = way.iter().filter(|t| !line.contains(t)).count();
        assert!(off <= 2, "close to the straight way: {way:?}");
        board.set_open(Tile::new(3, 3), Some(RivalId(1)));
        let way = route(
            &board,
            &tuning,
            &group,
            None,
            Tile::new(5, 5),
            |_| false,
            30,
        );
        assert_eq!(way.back(), Some(&Tile::new(5, 5)));
        assert!(
            way.iter().all(|t| t.distance(Tile::new(3, 3)) > 1),
            "round the margin: {way:?}"
        );
        let short = route(&board, &tuning, &group, None, Tile::new(9, 9), |_| false, 4);
        assert_eq!(short.len(), 4, "no more tiles than allowed");
    }

    #[test]
    fn walking_the_own_clearing_is_faster_than_cutting() {
        let row = ".".repeat(10);
        let mut board = Board::from_rows(&[row.as_str(); 3]);
        let tuning = Tuning::default();
        let rival = RivalId(0);
        // An open way round through the bottom row.
        for x in 0..=9 {
            board.set_open(Tile::new(x, 2), Some(rival));
        }
        let mut group = Group::new(GroupId(1), rival, Tile::new(0, 2));
        group.members[0].tile = Tile::new(0, 2);
        let way = route(
            &board,
            &tuning,
            &group,
            None,
            Tile::new(9, 1),
            |_| false,
            30,
        );
        assert!(way.iter().take(8).all(|t| t.y == 2), "{way:?}");
    }
}
