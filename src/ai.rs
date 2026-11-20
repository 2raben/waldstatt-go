//! Computer rivals: a few rules, not a search. They spread out early, merge
//! when threatened, close rings round small groups they see, and run or take
//! to the water when a ring is closing round them.
//!
//! They play through the same commands as the player and see only what their
//! groups see: other rivals' groups and open tiles only where their [`Sight`]
//! is clear now, and water only where they have looked before. They read rule
//! state and write only [`Controls`] (ADR 0002). Their memory is their own and
//! ordered, so a game from a seed still plays the same way every time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bevy::prelude::*;

use crate::rules::enclosure::{is_trapped, room};
use crate::rules::game::game_running;
use crate::rules::sight::Sight;
use crate::rules::{
    Board, Controls, Game, Ground, Group, GroupId, Line, Order, Rival, RivalId, RuleSystems, Seat,
    Tile, Trail, Tuning, line_limit,
};

/// Steps between a computer rival's decisions.
const THINK_EVERY: u64 = 16;
/// Enemies this close make a smaller group seek company.
const THREAT_RANGE: i32 = 8;
/// How far a group goes to join another.
const COMPANY_RANGE: i32 = 14;
/// How far a group looks for a group to enclose.
const HUNT_RANGE: i32 = 22;
/// A ring may be at most this wide and high.
const MAX_RING_SIDE: i32 = 18;
/// Searches stop after this many tiles.
const SEARCH_LIMIT: usize = 4000;
/// At most this many groups close one ring.
const HUNTERS: usize = 4;
/// A target is given up after this many steps.
const HUNT_STEPS: u64 = 64 * 60;

pub struct ComputerRivalsPlugin;

impl Plugin for ComputerRivalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Brains>()
            .add_systems(FixedUpdate, think.before(RuleSystems).run_if(game_running));
    }
}

/// What each computer rival remembers between decisions.
#[derive(Resource, Default)]
pub struct Brains(BTreeMap<RivalId, Brain>);

#[derive(Default)]
struct Brain {
    hunt: Option<Hunt>,
}

/// A ring being closed round another rival's group.
struct Hunt {
    target: GroupId,
    ring: Vec<Tile>,
    since: u64,
}

/// The board as one rival knows it.
struct View<'a> {
    board: &'a Board,
    sight: &'a Sight,
    tuning: &'a Tuning,
    rival: RivalId,
    /// The rival's own trails, by group.
    trails: &'a BTreeMap<GroupId, &'a Trail>,
}

impl View<'_> {
    fn ground(&self, tile: Tile) -> Ground {
        if self.sight.has_seen(self.board, tile) {
            self.board.ground(tile)
        } else {
            Ground::Land
        }
    }

    fn other_open(&self, tile: Tile) -> bool {
        self.sight.sees(self.board, tile) && self.board.open(tile).is_some_and(|r| r != self.rival)
    }

    fn own_open(&self, tile: Tile) -> bool {
        self.board.open(tile) == Some(self.rival)
    }

    /// Whether a group could cut or walk onto this land tile, as far as known.
    fn passable(&self, tile: Tile) -> bool {
        if !self.board.contains(tile) || self.ground(tile).is_water() {
            return false;
        }
        self.own_open(tile)
            || (!self.other_open(tile)
                && !tile.neighbours8().into_iter().any(|t| self.other_open(t)))
    }

    /// Whether a group could enter this water tile.
    fn floatable(&self, group: &Group, tile: Tile) -> bool {
        match self.ground(tile) {
            Ground::Land => false,
            // Rivers here are one to three tiles wide.
            Ground::River => group.trees >= self.tuning.river_trees * 3,
            Ground::Lake(_) => {
                group.size <= self.tuning.ship_capacity && group.trees >= self.tuning.ship_trees
            }
        }
    }

    fn trapped(&self, group: &Group) -> bool {
        is_trapped(self.board, group, self.tuning.trapped_room, |t| {
            self.sight.sees(self.board, t)
        })
    }

    /// The shortest way of straight steps from `from` to a tile `goal`
    /// accepts, over passable land and, if allowed, water.
    fn path(&self, group: &Group, goal: impl Fn(Tile) -> bool, water: bool) -> Option<Vec<Tile>> {
        let from = group.tile;
        let mut came: BTreeMap<Tile, Tile> = BTreeMap::new();
        let mut queue = VecDeque::from([from]);
        came.insert(from, from);
        while let Some(tile) = queue.pop_front() {
            if tile != from && goal(tile) {
                let mut path = vec![tile];
                let mut at = tile;
                while let Some(&before) = came.get(&at) {
                    if before == from {
                        break;
                    }
                    path.push(before);
                    at = before;
                }
                path.reverse();
                return Some(path);
            }
            if came.len() > SEARCH_LIMIT {
                return None;
            }
            for next in tile.neighbours4() {
                if came.contains_key(&next) || !self.board.contains(next) {
                    continue;
                }
                let ok = self.passable(next)
                    || goal(next) && self.own_open(next)
                    || (water && self.floatable(group, next));
                if ok {
                    came.insert(next, tile);
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// Straight steps from `from` towards `to`, the longer axis first, as far
    /// as the tiles are passable.
    fn straight(&self, from: Tile, to: Tile, limit: usize) -> Vec<Tile> {
        let mut line = Vec::new();
        let mut at = from;
        while at != to && line.len() < limit {
            let (dx, dy) = (to.x - at.x, to.y - at.y);
            let next = if dx.abs() >= dy.abs() {
                Tile::new(at.x + dx.signum(), at.y)
            } else {
                Tile::new(at.x, at.y + dy.signum())
            };
            if !self.passable(next) {
                break;
            }
            line.push(next);
            at = next;
        }
        line
    }
}

fn think(
    game: Res<Game>,
    board: Res<Board>,
    tuning: Res<Tuning>,
    rivals: Query<(&Rival, &Sight)>,
    groups: Query<(&Group, &Line, &Trail)>,
    mut controls: ResMut<Controls>,
    mut brains: ResMut<Brains>,
) {
    let mut rivals: Vec<_> = rivals.iter().collect();
    rivals.sort_by_key(|(rival, _)| rival.id);
    for (rival, sight) in rivals {
        if rival.seat != Seat::Computer || rival.out {
            continue;
        }
        if !(game.step + u64::from(rival.id.0) * 4).is_multiple_of(THINK_EVERY) {
            continue;
        }
        let mut own: Vec<(&Group, &Line)> = groups
            .iter()
            .filter(|(g, ..)| g.rival == rival.id)
            .map(|(g, l, _)| (g, l))
            .collect();
        own.sort_by_key(|(g, _)| g.id);
        let trails: BTreeMap<GroupId, &Trail> = groups
            .iter()
            .filter(|(g, ..)| g.rival == rival.id)
            .map(|(g, _, t)| (g.id, t))
            .collect();
        let view = View {
            board: &board,
            sight,
            tuning: &tuning,
            rival: rival.id,
            trails: &trails,
        };
        let mut enemies: Vec<&Group> = groups
            .iter()
            .map(|(g, ..)| g)
            .filter(|g| g.rival != rival.id && sight.sees(&board, g.tile))
            .collect();
        enemies.sort_by_key(|g| g.id);
        let brain = brains.0.entry(rival.id).or_default();
        for order in decide(&view, rival, &game, &own, &enemies, brain) {
            controls.send(rival.id, order);
        }
    }
}

fn line(group: &Group, tiles: Vec<Tile>) -> Order {
    Order::Line {
        group: group.id,
        tiles,
    }
}

fn decide(
    view: &View,
    rival: &Rival,
    game: &Game,
    own: &[(&Group, &Line)],
    enemies: &[&Group],
    brain: &mut Brain,
) -> Vec<Order> {
    let mut orders = Vec::new();
    let mut busy: BTreeSet<GroupId> = BTreeSet::new();

    // New Waldleute go to the largest group, which hunts best.
    if let Some((largest, _)) = own
        .iter()
        .max_by_key(|(g, _)| (g.size, std::cmp::Reverse(g.id)))
        && rival.receiver != Some(largest.id)
    {
        orders.push(Order::Receiver { group: largest.id });
    }

    // Run, or take to the water, when a ring is closing.
    for &(group, _) in own {
        if view.board.is_water(group.tile) {
            continue;
        }
        if view.trapped(group) {
            busy.insert(group.id);
            if let Some(way) = escape(view, group) {
                orders.push(line(group, way));
            }
        } else if let Some(away) = flee(view, group) {
            busy.insert(group.id);
            orders.push(line(group, away));
        }
    }

    // Merge when threatened by a bigger group.
    for &(group, _) in own {
        if busy.contains(&group.id) {
            continue;
        }
        let threatened = enemies
            .iter()
            .any(|e| e.size > group.size && e.tile.chebyshev(group.tile) <= THREAT_RANGE);
        if !threatened {
            continue;
        }
        let company = own
            .iter()
            .map(|(g, _)| *g)
            .filter(|g| g.id != group.id && !busy.contains(&g.id) && !view.board.is_water(g.tile))
            .filter(|g| g.tile.manhattan(group.tile) <= COMPANY_RANGE)
            .min_by_key(|g| (g.tile.manhattan(group.tile), g.id));
        let Some(other) = company else {
            continue;
        };
        // The smaller one goes to the bigger one, which waits.
        let (mover, stayer) = if (group.size, other.id) < (other.size, group.id) {
            (group, other)
        } else {
            (other, group)
        };
        if let Some(way) = view.path(mover, |t| t == stayer.tile, false) {
            orders.push(line(mover, way));
            orders.push(line(stayer, Vec::new()));
            busy.insert(mover.id);
            busy.insert(stayer.id);
        }
    }

    hunt(view, game, own, enemies, brain, &mut busy, &mut orders);

    // Spread out: idle groups split when large and go where nobody has been.
    for (i, &(group, group_line)) in own.iter().enumerate() {
        if busy.contains(&group.id) || !group_line.0.is_empty() || view.board.is_water(group.tile) {
            continue;
        }
        let calm = !enemies
            .iter()
            .any(|e| e.tile.chebyshev(group.tile) <= HUNT_RANGE);
        if group.size >= 5 && calm {
            orders.push(Order::Split { group: group.id });
            continue;
        }
        if let Some(way) = explore(view, game, group, own, i) {
            orders.push(line(group, way));
        }
    }
    orders
}

/// How many ring tiles a group takes on: what its trail holds open, less the
/// way there.
fn capacity(group: &Group) -> usize {
    (line_limit(group.size) * 2 / 3).max(4)
}

/// Closes a ring round the smallest group in sight, with up to [`HUNTERS`] of
/// the groups nearest to it, if their trails together can hold it open. Each
/// hunter cuts a piece of the ring its trail can hold, then waits there.
fn hunt(
    view: &View,
    game: &Game,
    own: &[(&Group, &Line)],
    enemies: &[&Group],
    brain: &mut Brain,
    busy: &mut BTreeSet<GroupId>,
    orders: &mut Vec<Order>,
) {
    // Keep a hunt while its target is in sight and inside the ring, and every
    // tile still to cut can be cut.
    let keep = brain.hunt.as_ref().is_some_and(|h| {
        game.step < h.since + HUNT_STEPS
            && enemies.iter().any(|e| {
                e.id == h.target && inside(&h.ring, e.tile) && !view.board.is_water(e.tile)
            })
            && needed_tiles(view, &h.ring)
                .iter()
                .all(|&t| view.passable(t))
    });
    if !keep {
        brain.hunt = None;
        let free: Vec<&Group> = own
            .iter()
            .map(|(g, _)| *g)
            .filter(|g| !busy.contains(&g.id) && !view.board.is_water(g.tile))
            .collect();
        let mut targets: Vec<&&Group> = enemies
            .iter()
            .filter(|e| !view.board.is_water(e.tile))
            .filter(|e| free.iter().any(|g| g.tile.manhattan(e.tile) <= HUNT_RANGE))
            .collect();
        targets.sort_by_key(|e| {
            let near = free
                .iter()
                .map(|g| g.tile.manhattan(e.tile))
                .min()
                .unwrap_or(i32::MAX);
            (e.size, near, e.id)
        });
        for target in targets {
            let Some(ring) = ring_round(view, target) else {
                continue;
            };
            let needed = needed_tiles(view, &ring).len();
            let mut nearest = free.clone();
            nearest.sort_by_key(|g| (g.tile.manhattan(target.tile), g.id));
            let reach: usize = nearest.iter().take(HUNTERS).map(|g| capacity(g)).sum();
            if reach >= needed {
                brain.hunt = Some(Hunt {
                    target: target.id,
                    ring,
                    since: game.step,
                });
                break;
            }
        }
    }
    let Some(hunt) = &brain.hunt else {
        return;
    };
    let Some(target) = enemies.iter().find(|e| e.id == hunt.target) else {
        return;
    };
    let mut left = needed_tiles(view, &hunt.ring);
    if left.is_empty() {
        return;
    }
    let ring: BTreeSet<Tile> = hunt.ring.iter().copied().collect();
    let mut hunters: Vec<(&Group, &Line)> = own
        .iter()
        .filter(|(g, _)| !busy.contains(&g.id) && !view.board.is_water(g.tile))
        .copied()
        .collect();
    hunters.sort_by_key(|(g, _)| (g.tile.manhattan(target.tile), g.id));
    hunters.truncate(HUNTERS);
    // Hunters already on their piece keep it; hunters holding a piece wait.
    let mut free = Vec::new();
    for (group, group_line) in hunters {
        busy.insert(group.id);
        if group_line.0.back().is_some_and(|t| left.contains(t)) {
            for tile in &group_line.0 {
                left.remove(tile);
            }
        } else {
            let holding = view
                .trails
                .get(&group.id)
                .is_some_and(|trail| trail.tiles().any(|t| ring.contains(&t)));
            if !(holding && group_line.0.is_empty()) {
                free.push(group);
            }
        }
    }
    for group in free {
        let runs = runs_of(&hunt.ring, &left);
        let Some(mut run) = runs.into_iter().min_by_key(|run| {
            let ends = [run[0], run[run.len() - 1]];
            ends.iter()
                .map(|t| t.manhattan(group.tile))
                .min()
                .unwrap_or(i32::MAX)
        }) else {
            break;
        };
        if run[run.len() - 1].manhattan(group.tile) < run[0].manhattan(group.tile) {
            run.reverse();
        }
        run.truncate(capacity(group));
        let start = run[0];
        let mut way = if group.tile == start {
            Vec::new()
        } else {
            match view.path(group, |t| t == start, false) {
                Some(way) => way,
                None => continue,
            }
        };
        for tile in &run {
            left.remove(tile);
        }
        way.extend(run.into_iter().skip(1));
        orders.push(line(group, way));
    }
}

/// A rectangle round the target and its trail, as a loop of tiles, at least
/// two tiles off them so it can be cut, and widened until every tile on it can
/// be. Sides that would lie beyond the board's edge are left out: the edge is
/// a wall.
fn ring_round(view: &View, target: &Group) -> Option<Vec<Tile>> {
    let body = trail_seen(view, target);
    let x0 = body.iter().map(|t| t.x).min()?;
    let x1 = body.iter().map(|t| t.x).max()?;
    let y0 = body.iter().map(|t| t.y).min()?;
    let y1 = body.iter().map(|t| t.y).max()?;
    (2..=4).find_map(|pad| {
        let ring = rectangle(view.board, (x0 - pad, y0 - pad, x1 + pad, y1 + pad))?;
        needed_tiles(view, &ring)
            .iter()
            .all(|&t| view.passable(t))
            .then_some(ring)
    })
}

/// The target's tile and the open tiles of its rival joined to it, as far as
/// seen: its trail, and any trail touching it.
fn trail_seen(view: &View, target: &Group) -> Vec<Tile> {
    let mut body = BTreeSet::from([target.tile]);
    let mut queue = VecDeque::from([target.tile]);
    while let Some(tile) = queue.pop_front() {
        for next in tile.neighbours8() {
            if body.len() < 60
                && !body.contains(&next)
                && view.sight.sees(view.board, next)
                && view.board.open(next) == Some(target.rival)
            {
                body.insert(next);
                queue.push_back(next);
            }
        }
    }
    body.into_iter().collect()
}

/// The loop of tiles round a rectangle, clockwise from the north-west, without
/// the sides beyond the board's edge; `None` if it is too large.
fn rectangle(board: &Board, (x0, y0, x1, y1): (i32, i32, i32, i32)) -> Option<Vec<Tile>> {
    if x1 - x0 > MAX_RING_SIDE || y1 - y0 > MAX_RING_SIDE {
        return None;
    }
    let (w, h) = (board.width, board.height);
    let (left, top, right, bottom) = (x0 < 0, y0 < 0, x1 >= w, y1 >= h);
    let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(w - 1), y1.min(h - 1));
    let sides: [(bool, Vec<Tile>); 4] = [
        (top, (x0..=x1).map(|x| Tile::new(x, y0)).collect()),
        (right, (y0 + 1..=y1).map(|y| Tile::new(x1, y)).collect()),
        (bottom, (x0..x1).rev().map(|x| Tile::new(x, y1)).collect()),
        (left, (y0 + 1..y1).rev().map(|y| Tile::new(x0, y)).collect()),
    ];
    let ring: Vec<Tile> = sides
        .into_iter()
        .filter(|(edge, _)| !edge)
        .flat_map(|(_, tiles)| tiles)
        .collect();
    (!ring.is_empty()).then_some(ring)
}

fn inside(ring: &[Tile], tile: Tile) -> bool {
    let (x0, x1) = (
        ring.iter().map(|t| t.x).min(),
        ring.iter().map(|t| t.x).max(),
    );
    let (y0, y1) = (
        ring.iter().map(|t| t.y).min(),
        ring.iter().map(|t| t.y).max(),
    );
    matches!((x0, x1, y0, y1), (Some(x0), Some(x1), Some(y0), Some(y1))
        if tile.x > x0 && tile.x < x1 && tile.y > y0 && tile.y < y1)
}

/// The ring's land tiles not yet open.
fn needed_tiles(view: &View, ring: &[Tile]) -> BTreeSet<Tile> {
    ring.iter()
        .copied()
        .filter(|&t| !view.ground(t).is_water() && !view.own_open(t))
        .collect()
}

/// The needed tiles as runs along the ring, each one straight steps long.
fn runs_of(ring: &[Tile], needed: &BTreeSet<Tile>) -> Vec<Vec<Tile>> {
    let mut runs: Vec<Vec<Tile>> = Vec::new();
    for &tile in ring {
        if !needed.contains(&tile) {
            continue;
        }
        match runs.last_mut() {
            Some(run) if run.last().is_some_and(|last| last.is_next_to(tile)) => run.push(tile),
            _ => runs.push(vec![tile]),
        }
    }
    // The ring is a loop: the last run may continue into the first.
    if runs.len() > 1 {
        let joined = runs[runs.len() - 1]
            .last()
            .zip(runs[0].first())
            .is_some_and(|(a, b)| a.is_next_to(*b));
        if joined {
            let first = runs.remove(0);
            if let Some(last) = runs.last_mut() {
                last.extend(first);
            }
        }
    }
    runs
}

/// A way out of a closing ring: over the water if there is any, else towards
/// the side with the most room.
fn escape(view: &View, group: &Group) -> Option<Vec<Tile>> {
    let mut pocket = BTreeSet::new();
    let mut queue = VecDeque::from([group.tile]);
    pocket.insert(group.tile);
    while let Some(tile) = queue.pop_front() {
        for next in tile.neighbours4() {
            if !pocket.contains(&next)
                && view.passable(next)
                && pocket.len() < view.tuning.trapped_room
            {
                pocket.insert(next);
                queue.push_back(next);
            }
        }
    }
    let over_water = view.path(group, |t| view.passable(t) && !pocket.contains(&t), true);
    over_water.or_else(|| {
        // No way over water: at least move to the tile with the most room.
        let best = pocket.iter().copied().max_by_key(|&t| {
            (
                room(view.board, view.rival, t, 64, |x| {
                    view.sight.sees(view.board, x)
                }),
                std::cmp::Reverse(t),
            )
        })?;
        (best != group.tile)
            .then(|| view.path(group, |t| t == best, false))
            .flatten()
    })
}

/// When another rival's tiles close in from three sides, runs to the fourth.
fn flee(view: &View, group: &Group) -> Option<Vec<Tile>> {
    let mut sides = [0usize; 4];
    for dy in -6..=6 {
        for dx in -6i32..=6 {
            let t = Tile::new(group.tile.x + dx, group.tile.y + dy);
            if !view.other_open(t) {
                continue;
            }
            if dy < 0 && dy.abs() >= dx.abs() {
                sides[0] += 1;
            }
            if dx > 0 && dx.abs() >= dy.abs() {
                sides[1] += 1;
            }
            if dy > 0 && dy.abs() >= dx.abs() {
                sides[2] += 1;
            }
            if dx < 0 && dx.abs() >= dy.abs() {
                sides[3] += 1;
            }
        }
    }
    // A side counts once it holds a wall, not a few lone tiles.
    if sides.iter().filter(|&&n| n >= 3).count() < 3 {
        return None;
    }
    let (side, _) = sides.iter().enumerate().min_by_key(|&(i, &n)| (n, i))?;
    let reach = line_limit(group.size) as i32;
    let [dx, dy] = [[0, -1], [1, 0], [0, 1], [-1, 0]][side];
    let to = Tile::new(group.tile.x + dx * reach, group.tile.y + dy * reach);
    let way = view.straight(group.tile, to, line_limit(group.size));
    (way.len() >= 2).then_some(way)
}

/// Where an idle group goes: a straight line of its full length towards
/// unseen land, away from its own other groups and from other rivals.
fn explore(
    view: &View,
    game: &Game,
    group: &Group,
    own: &[(&Group, &Line)],
    index: usize,
) -> Option<Vec<Tile>> {
    const DIRECTIONS: [(i32, i32); 8] = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    let reach = line_limit(group.size) as i32 / 2;
    // A turn of preference per group and over time, so rivals do not all
    // walk the same way.
    let turn = (game.step / 512 + index as u64 + u64::from(group.rival.0)) as usize;
    let mut best: Option<(i32, Vec<Tile>)> = None;
    for k in 0..DIRECTIONS.len() {
        let (dx, dy) = DIRECTIONS[(k + turn) % DIRECTIONS.len()];
        let to = Tile::new(group.tile.x + dx * reach, group.tile.y + dy * reach);
        let to = Tile::new(
            to.x.clamp(1, view.board.width - 2),
            to.y.clamp(1, view.board.height - 2),
        );
        let way = view.straight(group.tile, to, line_limit(group.size));
        let Some(&end) = way.last() else {
            continue;
        };
        let mut score = way.len() as i32 * 2;
        for y in -3..=3 {
            for x in -3..=3 {
                let t = Tile::new(end.x + x, end.y + y);
                if view.board.contains(t) && !view.sight.has_seen(view.board, t) {
                    score += 3;
                }
                if view.other_open(t) {
                    score -= 6;
                }
            }
        }
        let spread = own
            .iter()
            .filter(|(g, _)| g.id != group.id)
            .map(|(g, _)| g.tile.chebyshev(end))
            .min()
            .unwrap_or(20)
            .min(20);
        score += spread * 2;
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, way));
        }
    }
    best.map(|(_, way)| way).filter(|way| way.len() >= 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_follow_the_ring_and_wrap_round() {
        let ring: Vec<Tile> = [
            (0, 0),
            (1, 0),
            (2, 0),
            (2, 1),
            (2, 2),
            (1, 2),
            (0, 2),
            (0, 1),
        ]
        .into_iter()
        .map(|(x, y)| Tile::new(x, y))
        .collect();
        let needed: BTreeSet<Tile> = [(0, 0), (1, 0), (0, 1), (2, 2)]
            .into_iter()
            .map(|(x, y)| Tile::new(x, y))
            .collect();
        let runs = runs_of(&ring, &needed);
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().any(|run| run == &[Tile::new(2, 2)]));
        assert!(runs.iter().any(|run| run.len() == 3));
    }
}
