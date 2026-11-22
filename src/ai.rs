//! Computer rivals gather a hunting group of three. It closes rings round
//! visible rivals while the other groups and new recruits move to join it.
//! A threatened group can still flee or take to the water.
//!
//! They play through the same commands as the player and see only what their
//! groups see: other rivals' groups and open tiles only where their [`Sight`]
//! is clear now, and water only where they have looked before. They read rule
//! state and write only [`Controls`] (ADR 0002). Their memory is their own and
//! ordered, so a game from a seed still plays the same way every time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bevy::prelude::*;

use crate::rules::enclosure::{can_split, is_trapped, room};
use crate::rules::game::{STEPS_PER_SECOND, game_running};
use crate::rules::sight::Sight;
use crate::rules::{
    Board, Controls, Game, Ground, Group, GroupId, Line, Order, Rival, RivalId, RuleSystems, Seat,
    Tile, Trail, Tuning, line_limit,
};

/// Steps between a computer rival's decisions.
const THINK_EVERY: u64 = 16;
/// Searches stop after this many tiles.
const SEARCH_LIMIT: usize = 4000;

/// How a computer rival weighs its choices. Like [`Tuning`], these are
/// play-test values; the arena (`mise run arena`) measures how a change plays
/// against the default over many seeded games.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Temperament {
    /// Size at which the largest group starts hunting while others join it.
    /// Zero selects the distributed opening, which splits and hunts in teams.
    pub rally_size: u32,
    /// Bigger enemies this close, in steps, make a group seek company.
    pub threat_range: i32,
    /// How far a group goes to join another, in steps.
    pub company_range: i32,
    /// How far a group looks for a group to enclose, in steps.
    pub hunt_range: i32,
    /// Maximum span of a ring in steps along each of the three hex axes.
    pub max_ring_side: i32,
    /// At most this many groups close one ring.
    pub hunters: usize,
    /// A target is given up after this many seconds.
    pub hunt_seconds: u64,
    /// The share of what its trail holds open, in percent, that a hunter
    /// takes on of a ring; the rest is the way there.
    pub ring_share: usize,
    /// Calm groups of at least this many Waldleute split.
    pub split_size: u32,
    /// Other rivals' open tiles on one side that count as a wall to flee from.
    pub flee_wall: usize,
    /// Exploring: the score of each tile of the way.
    pub explore_length: i32,
    /// Exploring: the score of each unseen tile round the end.
    pub explore_unseen: i32,
    /// Exploring: what each other rival's open tile round the end costs.
    pub explore_crowd: i32,
    /// Exploring: the score of each step, up to 20, away from the rival's
    /// nearest other group.
    pub explore_spread: i32,
}

impl Default for Temperament {
    fn default() -> Self {
        Self {
            rally_size: 3,
            threat_range: 8,
            company_range: 12,
            hunt_range: 18,
            max_ring_side: 18,
            hunters: 4,
            hunt_seconds: 60,
            ring_share: 67,
            split_size: 5,
            flee_wall: 3,
            explore_length: 2,
            explore_unseen: 3,
            explore_crowd: 6,
            explore_spread: 2,
        }
    }
}

// Only the arena, which the browser build leaves out, names the values.
#[cfg(not(target_arch = "wasm32"))]
impl Temperament {
    /// The names [`Temperament::set`] takes, with their values.
    pub fn values(&self) -> Vec<(&'static str, i64)> {
        vec![
            ("rally_size", self.rally_size.into()),
            ("threat_range", self.threat_range.into()),
            ("company_range", self.company_range.into()),
            ("hunt_range", self.hunt_range.into()),
            ("max_ring_side", self.max_ring_side.into()),
            ("hunters", self.hunters as i64),
            ("hunt_seconds", self.hunt_seconds as i64),
            ("ring_share", self.ring_share as i64),
            ("split_size", self.split_size.into()),
            ("flee_wall", self.flee_wall as i64),
            ("explore_length", self.explore_length.into()),
            ("explore_unseen", self.explore_unseen.into()),
            ("explore_crowd", self.explore_crowd.into()),
            ("explore_spread", self.explore_spread.into()),
        ]
    }

    /// Sets the value of that name; values must not be negative.
    pub fn set(&mut self, name: &str, value: i64) -> Result<(), String> {
        fn fit<T: TryFrom<i64>>(name: &str, value: i64) -> Result<T, String> {
            T::try_from(value).map_err(|_| format!("{name}: {value} is out of range"))
        }
        if value < 0 {
            return Err(format!("{name}: {value} is negative"));
        }
        match name {
            "rally_size" => self.rally_size = fit(name, value)?,
            "threat_range" => self.threat_range = fit(name, value)?,
            "company_range" => self.company_range = fit(name, value)?,
            "hunt_range" => self.hunt_range = fit(name, value)?,
            "max_ring_side" => self.max_ring_side = fit(name, value)?,
            "hunters" => self.hunters = fit(name, value)?,
            "hunt_seconds" => self.hunt_seconds = fit(name, value)?,
            "ring_share" => self.ring_share = fit(name, value)?,
            "split_size" => self.split_size = fit(name, value)?,
            "flee_wall" => self.flee_wall = fit(name, value)?,
            "explore_length" => self.explore_length = fit(name, value)?,
            "explore_unseen" => self.explore_unseen = fit(name, value)?,
            "explore_crowd" => self.explore_crowd = fit(name, value)?,
            "explore_spread" => self.explore_spread = fit(name, value)?,
            _ => return Err(format!("no value called {name}")),
        }
        Ok(())
    }
}

/// Each computer rival's [`Temperament`]: the default unless given here.
#[derive(Resource, Clone, Debug, Default)]
pub struct Temperaments {
    pub by_rival: BTreeMap<RivalId, Temperament>,
    pub default: Temperament,
}

impl Temperaments {
    pub fn of(&self, rival: RivalId) -> &Temperament {
        self.by_rival.get(&rival).unwrap_or(&self.default)
    }
}

pub struct ComputerRivalsPlugin;

impl Plugin for ComputerRivalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Brains>()
            .init_resource::<Temperaments>()
            .add_systems(FixedUpdate, think.before(RuleSystems).run_if(game_running));
    }
}

/// What each computer rival remembers between decisions.
#[derive(Resource, Default)]
pub struct Brains(BTreeMap<RivalId, Brain>);

#[derive(Default)]
struct Brain {
    rally_group: Option<GroupId>,
    ready_to_hunt: bool,
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
    temperament: &'a Temperament,
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
                && !tile.neighbours().into_iter().any(|t| self.other_open(t)))
    }

    /// Whether a group could enter this water tile.
    fn floatable(&self, group: &Group, tile: Tile) -> bool {
        match self.ground(tile) {
            Ground::Land => false,
            // Rivers here are one to three tiles wide.
            Ground::River => group.trees >= self.tuning.river_trees * 3,
            Ground::Lake(_) => {
                group.size() <= self.tuning.ship_capacity && group.trees >= self.tuning.ship_trees
            }
        }
    }

    fn trapped(&self, group: &Group) -> bool {
        is_trapped(self.board, group, self.tuning.trapped_room, |t| {
            self.sight.sees(self.board, t)
        })
    }

    /// Whether the group has room to split, as far as the rival sees.
    fn can_split(&self, group: &Group) -> bool {
        can_split(self.board, group, self.tuning.split_room, |t| {
            self.sight.sees(self.board, t)
        })
    }

    /// The shortest way from `from` to a tile `goal` accepts, over passable
    /// land and, if allowed, water.
    fn path(&self, group: &Group, goal: impl Fn(Tile) -> bool, water: bool) -> Option<Vec<Tile>> {
        let from = group.tile();
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
            for next in tile.neighbours() {
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

    /// The straightest way from `from` towards `to`, as far as the tiles are
    /// passable.
    fn straight(&self, from: Tile, to: Tile, limit: usize) -> Vec<Tile> {
        from.line_to(to)
            .into_iter()
            .take(limit)
            .take_while(|&t| self.passable(t))
            .collect()
    }
}

fn think(
    game: Res<Game>,
    board: Res<Board>,
    (tuning, temperaments): (Res<Tuning>, Res<Temperaments>),
    rivals: Query<(&Rival, &Sight)>,
    groups: Query<(&Group, &Line, &Trail)>,
    mut controls: ResMut<Controls>,
    mut brains: ResMut<Brains>,
) {
    if game.step == 0 {
        brains.0.clear();
    }
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
            temperament: temperaments.of(rival.id),
            rival: rival.id,
            trails: &trails,
        };
        let mut enemies: Vec<&Group> = groups
            .iter()
            .map(|(g, ..)| g)
            .filter(|g| g.rival != rival.id && sight.sees(&board, g.tile()))
            .collect();
        enemies.sort_by_key(|g| g.id);
        let brain = brains.0.entry(rival.id).or_default();
        for order in decide(&view, &game, &own, &enemies, brain) {
            controls.send(rival.id, order);
        }
    }
}

fn line(group: &Group, tiles: Vec<Tile>) -> Order {
    Order::Line {
        group: group.id,
        from: group.tile(),
        tiles,
    }
}

fn decide(
    view: &View,
    game: &Game,
    own: &[(&Group, &Line)],
    enemies: &[&Group],
    brain: &mut Brain,
) -> Vec<Order> {
    let temper = view.temperament;
    let mut orders = Vec::new();
    let mut busy: BTreeSet<GroupId> = BTreeSet::new();

    // Run, or take to the water, when a ring is closing.
    for &(group, _) in own {
        if view.board.is_water(group.tile()) {
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

    if temper.rally_size > 0 {
        rally(view, game, own, enemies, brain, &mut busy, &mut orders);
        return orders;
    }

    // Merge when threatened by a bigger group.
    for &(group, _) in own {
        if busy.contains(&group.id) {
            continue;
        }
        let threatened = enemies.iter().any(|e| {
            e.size() > group.size() && e.tile().distance(group.tile()) <= temper.threat_range
        });
        if !threatened {
            continue;
        }
        let company = own
            .iter()
            .map(|(g, _)| *g)
            .filter(|g| g.id != group.id && !busy.contains(&g.id) && !view.board.is_water(g.tile()))
            .filter(|g| g.tile().distance(group.tile()) <= temper.company_range)
            .min_by_key(|g| (g.tile().distance(group.tile()), g.id));
        let Some(other) = company else {
            continue;
        };
        // The smaller one goes to the bigger one, which waits.
        let (mover, stayer) = if (group.size(), other.id) < (other.size(), group.id) {
            (group, other)
        } else {
            (other, group)
        };
        if let Some(way) = view.path(mover, |t| t == stayer.tile(), false) {
            orders.push(line(mover, way));
            orders.push(line(stayer, Vec::new()));
            busy.insert(mover.id);
            busy.insert(stayer.id);
        }
    }

    hunt(view, game, own, enemies, brain, &mut busy, &mut orders);

    // Spread out: idle groups split when large and go where nobody has been.
    for (i, &(group, group_line)) in own.iter().enumerate() {
        if busy.contains(&group.id) || !group_line.0.is_empty() || view.board.is_water(group.tile())
        {
            continue;
        }
        let calm = !enemies
            .iter()
            .any(|e| e.tile().distance(group.tile()) <= temper.hunt_range);
        if group.size() >= temper.split_size && calm && view.can_split(group) {
            orders.push(Order::split(group.id));
            continue;
        }
        if let Some(way) = explore(view, game, group, own, i) {
            orders.push(line(group, way));
        }
    }
    orders
}

/// Gather around the largest group. On ties keep the current group, then
/// prefer the shortest total distance from its recruits and the lowest ID.
fn rally(
    view: &View,
    game: &Game,
    own: &[(&Group, &Line)],
    enemies: &[&Group],
    brain: &mut Brain,
    busy: &mut BTreeSet<GroupId>,
    orders: &mut Vec<Order>,
) {
    let Some(&(hunter, hunter_line)) = own.iter().min_by_key(|(g, _)| {
        let distance: i32 = own
            .iter()
            .map(|(other, _)| g.tile().distance(other.tile()))
            .sum();
        (
            std::cmp::Reverse(g.size()),
            Some(g.id) != brain.rally_group,
            distance,
            g.id,
        )
    }) else {
        return;
    };
    brain.rally_group = Some(hunter.id);
    brain.ready_to_hunt |= hunter.size() >= view.temperament.rally_size || own.len() == 1;

    for (i, &(group, active)) in own.iter().enumerate() {
        if busy.contains(&group.id) || !active.0.is_empty() {
            continue;
        }
        let destination = if group.id == hunter.id {
            if brain.ready_to_hunt {
                continue;
            }
            // Meet the nearest recruit while the hunting group is forming.
            let Some((nearest, _)) = own
                .iter()
                .filter(|(other, _)| other.id != hunter.id)
                .min_by_key(|(other, _)| (hunter.tile().distance(other.tile()), other.id))
            else {
                continue;
            };
            nearest.tile()
        } else {
            hunter.tile()
        };
        // If the route fails, alternate retries with short local cuts that
        // can earn trees for a crossing.
        let idle_decisions = group.still / THINK_EVERY as u32;
        if group.still <= THINK_EVERY as u32 || idle_decisions.is_multiple_of(2) {
            orders.push(Order::Goto {
                group: group.id,
                from: group.tile(),
                to: destination,
            });
        } else if let Some(mut way) = explore(view, game, group, own, i) {
            way.truncate(3);
            orders.push(line(group, way));
        }
    }

    if brain.ready_to_hunt && !busy.contains(&hunter.id) {
        hunt(
            view,
            game,
            &[(hunter, hunter_line)],
            enemies,
            brain,
            busy,
            orders,
        );
        if !busy.contains(&hunter.id)
            && hunter_line.0.is_empty()
            && let Some(way) = explore(view, game, hunter, own, 0)
        {
            orders.push(line(hunter, way));
        }
    }
}

/// How many ring tiles a group takes on: what its trail holds open, less the
/// way there.
fn capacity(temper: &Temperament, group: &Group) -> usize {
    (line_limit(group.size()) * temper.ring_share / 100).max(4)
}

/// Closes a ring round the smallest group in sight, with up to
/// [`Temperament::hunters`] of
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
    let temper = view.temperament;
    // Keep a hunt while its target is in sight and inside the ring, and every
    // tile still to cut can be cut.
    let keep = brain.hunt.as_ref().is_some_and(|h| {
        game.step < h.since + temper.hunt_seconds * STEPS_PER_SECOND
            && enemies.iter().any(|e| {
                e.id == h.target && inside(&h.ring, e.tile()) && !view.board.is_water(e.tile())
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
            .filter(|g| !busy.contains(&g.id) && !view.board.is_water(g.tile()))
            .collect();
        let mut targets: Vec<&&Group> = enemies
            .iter()
            .filter(|e| !view.board.is_water(e.tile()))
            .filter(|e| {
                free.iter()
                    .any(|g| g.tile().distance(e.tile()) <= temper.hunt_range)
            })
            .collect();
        targets.sort_by_key(|e| {
            let near = free
                .iter()
                .map(|g| g.tile().distance(e.tile()))
                .min()
                .unwrap_or(i32::MAX);
            (e.size(), near, e.id)
        });
        for target in targets {
            let Some(ring) = ring_round(view, target) else {
                continue;
            };
            let needed = needed_tiles(view, &ring).len();
            let mut nearest = free.clone();
            nearest.sort_by_key(|g| (g.tile().distance(target.tile()), g.id));
            let reach: usize = nearest
                .iter()
                .take(temper.hunters)
                .map(|g| capacity(temper, g))
                .sum();
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
        .filter(|(g, _)| !busy.contains(&g.id) && !view.board.is_water(g.tile()))
        .copied()
        .collect();
    hunters.sort_by_key(|(g, _)| (g.tile().distance(target.tile()), g.id));
    hunters.truncate(temper.hunters);
    busy.extend(hunters.iter().map(|(g, _)| g.id));
    // When one group can finish the gap straight back to its own trail,
    // follow that field through regrowth, just as a player's click does.
    // Let an existing close spend its original movement allowance.
    if hunters.iter().any(|(g, _)| g.tail_offset.is_some()) {
        return;
    }
    for &(group, _) in &hunters {
        if let Some(offset) = closing_offset(view, group, &ring, &left) {
            orders.push(Order::Close {
                group: group.id,
                tail_offset: offset,
            });
            return;
        }
    }
    // Keep assigned cuts. A hunter holding its piece can extend it into a neighbouring gap.
    let mut free = Vec::new();
    for (group, group_line) in hunters {
        if group_line.0.back().is_some_and(|t| left.contains(t)) {
            for tile in &group_line.0 {
                left.remove(tile);
            }
        } else {
            let holding = view
                .trails
                .get(&group.id)
                .is_some_and(|trail| trail.tiles().any(|t| ring.contains(&t)));
            let beside_gap = group.tile().neighbours().iter().any(|t| left.contains(t));
            if !holding || !group_line.0.is_empty() || beside_gap {
                free.push(group);
            }
        }
    }
    for group in free {
        let runs = runs_of(&hunt.ring, &left);
        let Some(mut run) = runs.into_iter().min_by_key(|run| {
            let ends = [run[0], run[run.len() - 1]];
            ends.iter()
                .map(|t| t.distance(group.tile()))
                .min()
                .unwrap_or(i32::MAX)
        }) else {
            break;
        };
        if run[run.len() - 1].distance(group.tile()) < run[0].distance(group.tile()) {
            run.reverse();
        }
        run.truncate(capacity(temper, group));
        let start = run[0];
        let mut way = if group.tile() == start {
            Vec::new()
        } else {
            match view.path(group, |t| t == start, false) {
                Some(way) => way,
                None => continue,
            }
        };
        way.extend(run.into_iter().skip(1));
        way.truncate(line_limit(group.size()));
        for tile in &way {
            left.remove(tile);
        }
        orders.push(line(group, way));
    }
}

/// The live trail field beyond the final uncut stretch of one straight side.
fn closing_offset(
    view: &View,
    group: &Group,
    ring: &BTreeSet<Tile>,
    gap: &BTreeSet<Tile>,
) -> Option<usize> {
    let lead = group.tile();
    if !ring.contains(&lead) || gap.len() >= line_limit(group.size()) {
        return None;
    }
    let trail = view.trails.get(&group.id)?;
    let steps = gap.len() as i32;
    for direction in 0..6 {
        let tail = lead.go(direction, steps + 1);
        if ring.contains(&tail)
            && (1..=steps).all(|n| gap.contains(&lead.go(direction, n)))
            && let Some(offset) = trail.tiles().position(|tile| tile == tail)
        {
            return Some(offset);
        }
    }
    None
}

/// Bound the visible clearing on all three hex axes, leaving at least one
/// hex of forest between it and the hunters. Keep the full perimeter for
/// testing whether the target is inside, even where the board edge is a wall.
fn ring_round(view: &View, target: &Group) -> Option<Vec<Tile>> {
    let (q, r) = target.tile().axial();
    let mut lo = [q, r, q + r];
    let mut hi = lo;
    for tile in trail_seen(view, target) {
        let (q, r) = tile.axial();
        for (i, value) in [q, r, q + r].into_iter().enumerate() {
            lo[i] = lo[i].min(value);
            hi[i] = hi[i].max(value);
        }
    }
    for pad in 2..=4 {
        let [q0, r0, sum0] = lo.map(|x| x - pad);
        let [q1, r1, sum1] = hi.map(|x| x + pad);
        if (q1 - q0).max(r1 - r0).max(sum1 - sum0) > view.temperament.max_ring_side {
            break;
        }
        // Each corner is the intersection of two of the six bounding lines.
        let corners = [
            (sum0 - r0, r0),
            (q1, r0),
            (q1, sum1 - q1),
            (sum1 - r1, r1),
            (q0, r1),
            (q0, sum0 - q0),
        ]
        .map(|(q, r)| Tile::from_axial(q, r));
        let ring: Vec<_> = corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .flat_map(|(&a, &b)| a.line_to(b))
            .collect();
        let needed = needed_tiles(view, &ring);
        if !needed.is_empty() && needed.iter().all(|&t| view.passable(t)) {
            return Some(ring);
        }
    }
    None
}

/// The target's tile and the open tiles of its rival joined to it, as far as
/// seen: its trail, and any trail touching it.
fn trail_seen(view: &View, target: &Group) -> Vec<Tile> {
    let mut body = BTreeSet::from([target.tile()]);
    let mut queue = VecDeque::from([target.tile()]);
    while let Some(tile) = queue.pop_front() {
        for next in tile.neighbours() {
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

fn inside(ring: &[Tile], tile: Tile) -> bool {
    // A convex hex ring crosses each interior row at its left and right ends.
    !ring.contains(&tile)
        && ring.iter().any(|t| t.y == tile.y && t.x < tile.x)
        && ring.iter().any(|t| t.y == tile.y && t.x > tile.x)
}

/// The ring's land tiles not yet open.
fn needed_tiles(view: &View, ring: &[Tile]) -> BTreeSet<Tile> {
    ring.iter()
        .copied()
        .filter(|&t| view.board.contains(t) && !view.ground(t).is_water() && !view.own_open(t))
        .collect()
}

/// The needed tiles as runs along the ring, each tile next to the one before.
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
    let mut queue = VecDeque::from([group.tile()]);
    pocket.insert(group.tile());
    while let Some(tile) = queue.pop_front() {
        for next in tile.neighbours() {
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
        (best != group.tile())
            .then(|| view.path(group, |t| t == best, false))
            .flatten()
    })
}

/// When another rival's tiles close in from three sides, runs to the fourth.
fn flee(view: &View, group: &Group) -> Option<Vec<Tile>> {
    let here = group.tile().centre();
    // North, east, south and west.
    const SIDES: [Vec2; 4] = [Vec2::NEG_Y, Vec2::X, Vec2::Y, Vec2::NEG_X];
    let mut sides = [0usize; 4];
    for t in group.tile().within(6) {
        if !view.other_open(t) {
            continue;
        }
        let d = t.centre() - here;
        for (side, towards) in SIDES.iter().enumerate() {
            if d.dot(*towards) > 0.0 && d.dot(*towards) >= d.perp_dot(*towards).abs() {
                sides[side] += 1;
            }
        }
    }
    // A side counts once it holds a wall, not a few lone tiles.
    let wall = view.temperament.flee_wall;
    if sides.iter().filter(|&&n| n >= wall).count() < 3 {
        return None;
    }
    let (side, _) = sides.iter().enumerate().min_by_key(|&(i, &n)| (n, i))?;
    let reach = line_limit(group.size());
    let to = Tile::at(here + SIDES[side] * reach as f32);
    let way = view.straight(group.tile(), to, reach);
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
    let temper = view.temperament;
    let reach = line_limit(group.size()) as i32 / 2;
    // A turn of preference per group and over time, so rivals do not all
    // walk the same way.
    let turn = (game.step / 512 + index as u64 + u64::from(group.rival.0)) as usize;
    let mut best: Option<(i32, Vec<Tile>)> = None;
    for k in 0..6 {
        let to = group.tile().go(k + turn, reach);
        let to = Tile::new(
            to.x.clamp(1, view.board.width - 2),
            to.y.clamp(1, view.board.height - 2),
        );
        let way = view.straight(group.tile(), to, line_limit(group.size()));
        let Some(&end) = way.last() else {
            continue;
        };
        let mut score = way.len() as i32 * temper.explore_length;
        for t in end.within(3) {
            if view.board.contains(t) && !view.sight.has_seen(view.board, t) {
                score += temper.explore_unseen;
            }
            if view.other_open(t) {
                score -= temper.explore_crowd;
            }
        }
        let spread = own
            .iter()
            .filter(|(g, _)| g.id != group.id)
            .map(|(g, _)| g.tile().distance(end))
            .min()
            .unwrap_or(20)
            .min(20);
        score += spread * temper.explore_spread;
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
    fn every_temperament_value_can_be_set_by_its_name() {
        let mut temperament = Temperament::default();
        for (name, value) in Temperament::default().values() {
            temperament.set(name, value + 1).expect("the name is known");
        }
        for ((name, value), (_, default)) in temperament
            .values()
            .into_iter()
            .zip(Temperament::default().values())
        {
            assert_eq!(value, default + 1, "{name}");
        }
        assert!(temperament.set("hunters", -1).is_err());
    }

    #[test]
    fn planned_hex_rings_enclose_the_seen_clearing_even_at_the_board_edge() {
        use crate::rules::enclosure::Partition;

        for centre in [
            Tile::new(8, 8),
            Tile::new(8, 9),
            Tile::new(0, 0),
            Tile::new(15, 15),
        ] {
            let mut board = Board::from_rows(&["................"; 16]);
            let target = Group::new(GroupId(1), RivalId(1), centre);
            let hunter = RivalId(0);
            let end = centre.go(if centre.x == 15 { 4 } else { 1 }, 2);
            let mut body = centre.line_to(end);
            body.push(centre);
            for &t in &body {
                board.set_open(t, Some(target.rival));
            }
            let mut sight = Sight::new(&board);
            sight.look(&board, hunter, [centre].into_iter(), 8);
            let view = View {
                board: &board,
                sight: &sight,
                tuning: &Tuning::default(),
                temperament: &Temperament::default(),
                rival: hunter,
                trails: &BTreeMap::new(),
            };
            let ring = ring_round(&view, &target).expect("a hunt fits");
            assert_eq!(
                ring.len(),
                16,
                "radius two, stretched two steps east or west"
            );
            for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                assert!(a.is_next_to(b));
                assert!(!inside(&ring, a));
            }
            for t in needed_tiles(&view, &ring) {
                board.set_open(t, Some(hunter));
            }
            let partition = Partition::of(&board, hunter);
            for t in body {
                assert!(inside(&ring, t) && partition.encloses(&board, t), "{t:?}");
            }
        }
    }

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
