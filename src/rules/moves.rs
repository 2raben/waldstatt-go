//! Moving every group each fixed step (ADRs 0001 and 0007).
//!
//! Groups move in the game's rival order and then by group number, so two
//! rivals reaching the same tile always resolve the same way. For each group:
//!
//! 1. The lead works along the line and enters its next tile when the work
//!    reaches the tile's cost; a bigger group cuts faster.
//! 2. A lead that cuts a tile touching another clearing of its rival, or walks
//!    into one, merges that group into its own.
//! 3. While the group has a line, the others follow the lead over the clearing
//!    and the water it crossed, each at most [`Tuning::gap`] tiles per place in
//!    the file behind it, cutting a shortcut only where it is much shorter.
//!    Without a line they gather as near to its end as they can, cutting where
//!    they must; water, the board's edge, other rivals' margins and other
//!    groups' clearings, the rival's own too, bound them.
//! 4. The trail keeps `line_limit(size)` tiles while the group cuts, and grows
//!    back one tile every [`Tuning::regrow_steps`] once it has stood still for
//!    [`Tuning::regrow_after`]: the
//!    tile none of its Waldleute has stood on for longest; never a tile a
//!    Waldmensch stands on, one the group gathers on, or one that joins its
//!    Waldleute. A piece of clearing nobody can reach grows back at once.
//!
//! Only one Waldmensch stands on a tile of land; on water they share.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;

use super::board::{Board, Ground, RivalId, Tile};
use super::game::{Game, GameEvent, Rival};
use super::group::{Group, Line, Move, Paths, Trail, merge_trails, plan_step};
use super::route::route;
use super::sight::Sight;
use super::tuning::{Tuning, line_limit};

/// What entering an open tile or a crossed water tile costs in a search,
/// against cutting forest: about what walking costs against cutting.
const WALK_COST: u32 = 1;
const CUT_COST: u32 = 4;
/// The most tiles a search for one group looks at.
const SEARCH_LIMIT: usize = 4000;

/// A group taken out of the world for one step.
pub struct Body {
    pub entity: Entity,
    pub group: Group,
    pub trail: Trail,
    pub line: Line,
    pub gone: bool,
}

/// Who stands on each tile of land: the body's index and its rival.
type Crowd = BTreeMap<Tile, (usize, RivalId)>;

pub fn advance_groups(
    mut commands: Commands,
    mut board: ResMut<Board>,
    tuning: Res<Tuning>,
    mut game: ResMut<Game>,
    rivals: Query<(&Rival, &Sight)>,
    mut groups: Query<(Entity, &mut Group, &mut Trail, &mut Line)>,
) {
    let mut bodies: Vec<Body> = groups
        .iter()
        .map(|(entity, group, trail, line)| Body {
            entity,
            group: group.clone(),
            trail: trail.clone(),
            line: line.clone(),
            gone: false,
        })
        .collect();
    bodies.sort_by_key(|b| (game.rank(b.group.rival), b.group.id));
    let sights: BTreeMap<RivalId, &Sight> = rivals.iter().map(|(r, s)| (r.id, s)).collect();
    let events = advance(&mut board, &tuning, game.step, &mut bodies, &sights);
    for body in bodies {
        if body.gone {
            commands.entity(body.entity).despawn();
            continue;
        }
        if let Ok((_, mut group, mut trail, mut line)) = groups.get_mut(body.entity) {
            group.set_if_neq(body.group);
            trail.set_if_neq(body.trail);
            line.set_if_neq(body.line);
        }
    }
    game.events.extend(events);
}

/// One step for every group, in the order of `bodies`.
pub fn advance(
    board: &mut Board,
    tuning: &Tuning,
    step: u64,
    bodies: &mut [Body],
    sights: &BTreeMap<RivalId, &Sight>,
) -> Vec<GameEvent> {
    let mut events = Vec::new();
    let mut crowd = Crowd::new();
    for (i, body) in bodies.iter().enumerate() {
        place(&mut crowd, board, i, &body.group);
    }
    let mut owners = Owners::new(board, bodies);
    for i in 0..bodies.len() {
        if bodies[i].gone {
            continue;
        }
        let sight = sights.get(&bodies[i].group.rival).copied();
        if let Some((kind, tile)) = lead_step(
            board,
            tuning,
            step,
            &mut bodies[i],
            i,
            &mut crowd,
            sight,
            &mut owners,
        ) {
            merge_touching(
                board,
                step,
                bodies,
                i,
                kind,
                tile,
                &mut crowd,
                &mut owners,
                &mut events,
            );
        }
        let keep = others_step(
            board,
            tuning,
            step,
            &mut bodies[i],
            i,
            &mut crowd,
            &mut owners,
        );
        upkeep(board, tuning, step, &mut bodies[i], i, &keep, &mut owners);
    }
    events
}

/// Which group holds each tile, by its place in the step: its trail and its
/// Waldleute on land. Kept up to date as tiles are cut, grow back and merge.
struct Owners {
    of: Vec<usize>,
    rivals: Vec<RivalId>,
}

impl Owners {
    fn new(board: &Board, bodies: &[Body]) -> Self {
        let mut of = vec![usize::MAX; board.len()];
        for (i, body) in bodies.iter().enumerate() {
            if body.gone {
                continue;
            }
            let land = body.group.tiles().filter(|&t| is_land(board, t));
            for tile in body.trail.tiles().chain(land) {
                if let Some(k) = board.index(tile) {
                    of[k] = i;
                }
            }
        }
        let rivals = bodies.iter().map(|b| b.group.rival).collect();
        Self { of, rivals }
    }

    fn take(&mut self, board: &Board, tile: Tile, i: usize) {
        if let Some(k) = board.index(tile) {
            self.of[k] = i;
        }
    }

    fn give_up(&mut self, board: &Board, tile: Tile, i: usize) {
        if let Some(k) = board.index(tile)
            && self.of[k] == i
        {
            self.of[k] = usize::MAX;
        }
    }

    fn merge(&mut self, from: usize, into: usize) {
        for owner in &mut self.of {
            if *owner == from {
                *owner = into;
            }
        }
    }

    /// The other group of `i`'s rival that holds the tile, if any.
    fn other(&self, board: &Board, tile: Tile, i: usize) -> Option<usize> {
        let j = *self.of.get(board.index(tile)?)?;
        (j != usize::MAX && j != i && self.rivals[j] == self.rivals[i]).then_some(j)
    }

    /// Whether the tile or one round it is held by another group of `i`'s
    /// rival: group `i` must not cut there.
    fn near_other(&self, board: &Board, tile: Tile, i: usize) -> bool {
        std::iter::once(tile)
            .chain(tile.neighbours8())
            .any(|t| self.other(board, t, i).is_some())
    }
}

fn is_land(board: &Board, tile: Tile) -> bool {
    board.contains(tile) && board.ground(tile) == Ground::Land
}

/// Records where a group's Waldleute stand on land.
fn place(crowd: &mut Crowd, board: &Board, i: usize, group: &Group) {
    crowd.retain(|_, (j, _)| *j != i);
    for tile in group.tiles() {
        if is_land(board, tile) {
            crowd.entry(tile).or_insert((i, group.rival));
        }
    }
}

/// The lead's work along the line; returns how it entered a tile, if it did.
#[allow(clippy::too_many_arguments)]
fn lead_step(
    board: &mut Board,
    tuning: &Tuning,
    step: u64,
    body: &mut Body,
    i: usize,
    crowd: &mut Crowd,
    sight: Option<&Sight>,
    owners: &mut Owners,
) -> Option<(Move, Tile)> {
    let group = &mut body.group;
    group.lead = group.lead.min(group.members.len() - 1);
    let lead = group.lead;
    let Some(&next) = body.line.0.front() else {
        group.members[lead].progress = 0;
        group.members[lead].toward = None;
        return None;
    };
    let rival = group.rival;
    let other_there = crowd.get(&next).is_some_and(|&(_, r)| r != rival);
    let Some(plan) = plan_step(board, tuning, group, &body.line.0, other_there) else {
        // The line runs into something it may not cross: another way to where
        // it leads, or it stops short.
        if let Some(goal) = group.goal.filter(|_| group.budget > 0) {
            let avoid = |t: Tile| t == next || owners.near_other(board, t, i);
            let way = route(board, tuning, group, sight, goal, avoid, group.budget);
            if !way.is_empty() {
                body.line.0 = way;
                let member = &mut group.members[lead];
                member.progress = 0;
                member.toward = None;
                return None;
            }
        }
        group.goal = None;
        body.line.0.clear();
        let member = &mut group.members[lead];
        member.progress = 0;
        member.toward = None;
        group.target = member.tile;
        return None;
    };
    let speed = tuning.speed(group.size());
    let member = &mut group.members[lead];
    if member.toward != Some(next) {
        member.progress = 0;
        member.toward = Some(next);
    }
    member.progress += speed;
    if member.progress < plan.work {
        return None;
    }
    body.line.0.pop_front();
    let old = member.tile;
    group.trees -= plan.trees;
    group.budget = group.budget.saturating_sub(1);
    match plan.kind {
        Move::Launch => {
            // Everyone boards.
            for member in &mut group.members {
                member.enter(next, step);
            }
            group.way.push(next);
        }
        Move::Sail => {
            for member in group.members.iter_mut().filter(|m| m.tile == old) {
                member.enter(next, step);
            }
            group.way.push(next);
        }
        _ => {
            // A Waldmensch of the group standing ahead swaps places with the lead.
            if is_land(board, next)
                && let Some(k) = group
                    .members
                    .iter()
                    .position(|m| m.tile == next)
                    .filter(|&k| k != lead)
            {
                group.members[k].enter(old, step);
            }
            group.members[lead].enter(next, step);
            if plan.kind == Move::Wade {
                group.way.push(next);
            }
            if plan.kind == Move::Cut {
                board.set_open(next, Some(rival));
                body.trail.push(next, step);
                group.trees += board.trees(next);
            }
            if is_land(board, next) {
                owners.take(board, next, i);
            }
        }
    }
    if body.line.0.is_empty() {
        group.target = next;
        group.goal = None;
    }
    place(crowd, board, i, group);
    Some((plan.kind, next))
}

/// Merges into group `i` every other group of its rival whose clearing the
/// lead just joined: by cutting a tile next to it, or by walking into it.
#[allow(clippy::too_many_arguments)]
fn merge_touching(
    board: &Board,
    step: u64,
    bodies: &mut [Body],
    i: usize,
    kind: Move,
    tile: Tile,
    crowd: &mut Crowd,
    owners: &mut Owners,
    events: &mut Vec<GameEvent>,
) {
    let touched: Vec<Tile> = match kind {
        Move::Cut => board.steps(bodies[i].group.rival, tile).collect(),
        Move::Walk => vec![tile],
        _ => return,
    };
    let rival = bodies[i].group.rival;
    let joined: BTreeSet<usize> = touched
        .iter()
        .filter_map(|&t| owners.other(board, t, i))
        .filter(|&j| !bodies[j].gone)
        .collect();
    for j in joined {
        owners.merge(j, i);
        let (into, from) = pair(bodies, i, j);
        into.group.members.append(&mut from.group.members.clone());
        into.group.trees += from.group.trees;
        into.trail = merge_trails(&into.trail, &from.trail);
        for &t in &from.group.way {
            if !into.group.way.contains(&t) {
                into.group.way.push(t);
            }
        }
        from.gone = true;
        events.push(GameEvent::Merged {
            step,
            rival,
            from: from.group.id,
            into: into.group.id,
        });
        crowd.retain(|_, (k, _)| *k != j);
    }
    place(crowd, board, i, &bodies[i].group);
}

fn pair(bodies: &mut [Body], i: usize, j: usize) -> (&mut Body, &mut Body) {
    if i < j {
        let (a, b) = bodies.split_at_mut(j);
        (&mut a[i], &mut b[0])
    } else {
        let (a, b) = bodies.split_at_mut(i);
        (&mut b[0], &mut a[j])
    }
}

/// Where a group's Waldleute may go and what it costs.
struct Reach<'a> {
    board: &'a Board,
    rival: RivalId,
    open: BTreeSet<Tile>,
    owners: &'a Owners,
    crowd: &'a Crowd,
    i: usize,
}

impl Reach<'_> {
    /// A forest tile its Waldleute may cut.
    fn cuttable(&self, tile: Tile) -> bool {
        is_land(self.board, tile)
            && self.board.open(tile).is_none()
            && !self.board.in_margin_of_other(tile, self.rival)
            && !self.owners.near_other(self.board, tile, self.i)
            && self.crowd.get(&tile).is_none_or(|&(j, _)| j == self.i)
    }

    fn cost(&self, tile: Tile) -> Option<u32> {
        if self.open.contains(&tile) {
            Some(WALK_COST)
        } else if self.cuttable(tile) {
            Some(CUT_COST)
        } else {
            None
        }
    }
}

/// Moves the Waldleute other than the lead: following it while the group has
/// a line, gathering round its end when not. Returns the tiles they gather on.
fn others_step(
    board: &mut Board,
    tuning: &Tuning,
    step: u64,
    body: &mut Body,
    i: usize,
    crowd: &mut Crowd,
    owners: &mut Owners,
) -> BTreeSet<Tile> {
    let group = &body.group;
    let lead_tile = group.tile();
    let ground = Reach {
        board,
        rival: group.rival,
        open: body
            .trail
            .tiles()
            .chain(group.way.iter().copied())
            .collect(),
        owners,
        crowd,
        i,
    };
    let mut home = BTreeSet::new();
    // Each move: which Waldmensch, and the tile it goes to.
    let mut moves: Vec<(usize, Tile)> = Vec::new();
    if !body.line.0.is_empty() || board.is_water(lead_tile) {
        let wanted: BTreeSet<Tile> = group.tiles().collect();
        let paths = Paths::search(
            &[lead_tile],
            |t| board.steps(group.rival, t),
            |t| ground.cost(t),
            |_| true,
            &wanted,
            SEARCH_LIMIT,
        );
        let mut file: Vec<usize> = (0..group.members.len())
            .filter(|&k| k != group.lead)
            .filter(|&k| paths.cost(group.members[k].tile).is_some())
            .collect();
        file.sort_by_key(|&k| {
            let tile = group.members[k].tile;
            (paths.cost(tile), paths.hops(tile), k)
        });
        for (place, &k) in file.iter().enumerate() {
            let tile = group.members[k].tile;
            let hops = paths.hops(tile).unwrap_or(0);
            if hops <= tuning.gap * (place as u32 + 1) {
                continue;
            }
            if let Some(next) = paths.next(tile) {
                moves.push((k, next));
            }
        }
    } else {
        let target = if ground.cost(group.target).is_some() {
            group.target
        } else {
            lead_tile
        };
        let standing: BTreeSet<Tile> = group.tiles().collect();
        let near = home_tiles(&ground, target, group.members.len(), &standing);
        home = near.keys().copied().collect();
        let mut claimed = BTreeSet::new();
        let mut settled = Vec::new();
        let mut loose = Vec::new();
        for (k, member) in group.members.iter().enumerate() {
            if home.contains(&member.tile) && claimed.insert(member.tile) {
                settled.push(k);
            } else {
                loose.push(k);
            }
        }
        // Those settled step into free tiles nearer the end, which leaves room
        // at the edge for those still coming.
        let mut taken = claimed.clone();
        for &k in &settled {
            let tile = group.members[k].tile;
            let inward = board
                .steps(group.rival, tile)
                .filter(|n| {
                    near.contains_key(n) && !taken.contains(n) && near.get(n) < near.get(&tile)
                })
                .min_by_key(|n| (near.get(n).copied(), *n));
            if let Some(next) = inward {
                taken.insert(next);
                moves.push((k, next));
            }
        }
        let free: Vec<Tile> = home.difference(&taken).copied().collect();
        if !loose.is_empty() && !free.is_empty() {
            let wanted: BTreeSet<Tile> = loose.iter().map(|&k| group.members[k].tile).collect();
            let paths = Paths::search(
                &free,
                |t| board.steps(group.rival, t),
                |t| {
                    if claimed.contains(&t) && !wanted.contains(&t) {
                        None
                    } else {
                        ground.cost(t)
                    }
                },
                |_| true,
                &wanted,
                SEARCH_LIMIT,
            );
            loose.sort_by_key(|&k| (paths.cost(group.members[k].tile), k));
            for k in loose {
                if let Some(next) = paths.next(group.members[k].tile) {
                    moves.push((k, next));
                }
            }
        }
    }
    let speed = tuning.speed(group.size());
    for (k, next) in moves {
        let rival = body.group.rival;
        if is_land(board, next) && crowd.contains_key(&next) {
            continue;
        }
        let forest = is_land(board, next) && board.open(next).is_none();
        let work = if forest {
            tuning.cut_work
        } else {
            tuning.walk_work
        };
        let member = &mut body.group.members[k];
        if member.toward != Some(next) {
            member.progress = 0;
            member.toward = Some(next);
        }
        member.progress += speed;
        if member.progress < work {
            continue;
        }
        crowd.remove(&member.tile);
        member.enter(next, step);
        if is_land(board, next) {
            crowd.insert(next, (i, rival));
            owners.take(board, next, i);
        }
        if forest {
            board.set_open(next, Some(rival));
            body.trail.push(next, step);
            body.group.trees += board.trees(next);
        }
    }
    home
}

/// The tiles nearest `target` that a group of `count` gathers on, with how
/// many steps each is from it: by those steps over land it may stand
/// on or cut, then by distance, then where its Waldleute stand already, then
/// open before forest, then row by row.
fn home_tiles(
    ground: &Reach,
    target: Tile,
    count: usize,
    standing: &BTreeSet<Tile>,
) -> BTreeMap<Tile, u32> {
    let mut reached: BTreeMap<Tile, u32> = BTreeMap::from([(target, 0)]);
    let mut layer = vec![target];
    let mut hops = 0;
    while reached.len() < count && !layer.is_empty() {
        hops += 1;
        let mut next_layer = Vec::new();
        for tile in layer {
            for next in ground.board.steps(ground.rival, tile) {
                if !reached.contains_key(&next)
                    && is_land(ground.board, next)
                    && ground.cost(next).is_some()
                {
                    reached.insert(next, hops);
                    next_layer.push(next);
                }
            }
        }
        layer = next_layer;
    }
    let mut tiles: Vec<(u32, i32, Tile)> = reached
        .into_iter()
        .map(|(t, h)| {
            let (dx, dy) = (t.x - target.x, t.y - target.y);
            (h, dx * dx + dy * dy, t)
        })
        .collect();
    tiles.sort_by_key(|&(h, d, t)| {
        let open = ground.open.contains(&t);
        (h, d, !standing.contains(&t), !open, t.y, t.x)
    });
    tiles
        .into_iter()
        .take(count)
        .map(|(h, _, t)| (t, h))
        .collect()
}

/// How many separate pieces of `open` the Waldleute stand on.
fn pieces(board: &Board, rival: RivalId, open: &BTreeSet<Tile>, standing: &[Tile]) -> usize {
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for &start in standing {
        if seen.contains(&start) {
            continue;
        }
        count += 1;
        if !open.contains(&start) {
            continue;
        }
        let mut stack = vec![start];
        seen.insert(start);
        while let Some(tile) = stack.pop() {
            for next in board.steps(rival, tile) {
                if open.contains(&next) && seen.insert(next) {
                    stack.push(next);
                }
            }
        }
    }
    count
}

/// The tile of the trail that grows back next: the one no Waldmensch of the
/// group has stood on for longest; never one a Waldmensch stands on, one in
/// `keep`, or one without which the Waldleute would stand on more pieces than
/// now. So a trail shrinks from where they have not been back to them.
pub fn regrowable(
    board: &Board,
    group: &Group,
    trail: &Trail,
    keep: &BTreeSet<Tile>,
) -> Option<Tile> {
    let open: BTreeSet<Tile> = trail.tiles().chain(group.way.iter().copied()).collect();
    let standing: Vec<Tile> = group.tiles().collect();
    let mut candidates: Vec<(u64, usize, Tile)> = trail
        .0
        .iter()
        .enumerate()
        .filter(|(_, (t, _))| !standing.contains(t) && !keep.contains(t))
        .map(|(i, &(t, touched))| (touched, i, t))
        .collect();
    candidates.sort();
    let now = pieces(board, group.rival, &open, &standing);
    candidates.into_iter().map(|(.., t)| t).find(|tile| {
        let mut without = open.clone();
        without.remove(tile);
        pieces(board, group.rival, &without, &standing) <= now
    })
}

/// The trail tiles no Waldmensch of the group can reach over its clearing and
/// the water it crossed: a clearing is never left empty.
fn deserted(board: &Board, group: &Group, trail: &Trail) -> Vec<Tile> {
    let open: BTreeSet<Tile> = trail.tiles().chain(group.way.iter().copied()).collect();
    let mut reached: BTreeSet<Tile> = group.tiles().filter(|t| open.contains(t)).collect();
    let mut stack: Vec<Tile> = reached.iter().copied().collect();
    while let Some(tile) = stack.pop() {
        for next in board.steps(group.rival, tile) {
            if open.contains(&next) && reached.insert(next) {
                stack.push(next);
            }
        }
    }
    trail.tiles().filter(|t| !reached.contains(t)).collect()
}

fn regrow(board: &mut Board, body: &mut Body, i: usize, owners: &mut Owners, tile: Tile) {
    owners.give_up(board, tile, i);
    body.trail.remove(tile);
    if board.open(tile) == Some(body.group.rival) {
        board.set_open(tile, None);
    }
}

/// Lets the trail grow back, forgets water no longer needed, and notes the
/// tile that grows back next.
fn upkeep(
    board: &mut Board,
    tuning: &Tuning,
    step: u64,
    body: &mut Body,
    i: usize,
    keep: &BTreeSet<Tile>,
    owners: &mut Owners,
) {
    let limit = line_limit(body.group.size());
    // Where the Waldleute stand now is touched now.
    for (tile, touched) in body.trail.0.iter_mut() {
        if body.group.tiles().any(|t| t == *tile) {
            *touched = step;
        }
    }
    for tile in deserted(board, &body.group, &body.trail) {
        regrow(board, body, i, owners, tile);
    }
    let still = body.line.0.is_empty() && !board.is_water(body.group.tile());
    body.group.still = if still { body.group.still + 1 } else { 0 };
    while body.trail.0.len() > limit {
        let Some(tile) = regrowable(board, &body.group, &body.trail, keep) else {
            break;
        };
        regrow(board, body, i, owners, tile);
    }
    if still
        && body.group.still >= tuning.regrow_after
        && (body.group.still - tuning.regrow_after).is_multiple_of(tuning.regrow_steps)
        && let Some(tile) = regrowable(board, &body.group, &body.trail, keep)
    {
        regrow(board, body, i, owners, tile);
    }
    let on_water = body.group.tiles().any(|t| board.is_water(t));
    if !on_water && !body.group.way.is_empty() {
        let open: BTreeSet<Tile> = body.trail.tiles().collect();
        let standing: Vec<Tile> = body.group.tiles().collect();
        if pieces(board, body.group.rival, &open, &standing) <= 1 {
            body.group.way.clear();
        }
    }
    // Only for showing: worked out four times a second.
    if body.group.regrows.is_some_and(|t| !body.trail.contains(t)) {
        body.group.regrows = None;
    }
    if step.is_multiple_of(16) {
        body.group.regrows = if still || body.trail.0.len() >= limit {
            regrowable(board, &body.group, &body.trail, keep)
        } else {
            None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::group::{GroupId, Member};

    fn body(group: Group, trail: Trail, line: Line) -> Body {
        Body {
            entity: Entity::PLACEHOLDER,
            group,
            trail,
            line,
            gone: false,
        }
    }

    #[test]
    fn a_tile_joining_two_waldleute_does_not_grow_back() {
        let board = Board::from_rows(&[&".".repeat(6)]);
        let mut group = Group::new(GroupId(1), RivalId(0), Tile::new(4, 0));
        group.members.push(Member::at(Tile::new(2, 0), 0));
        let trail = Trail((0..=4).map(|x| (Tile::new(x, 0), x as u64)).collect());
        // (0, 0) and (1, 0) are behind both; (3, 0) joins them.
        assert_eq!(
            regrowable(&board, &group, &trail, &BTreeSet::new()),
            Some(Tile::new(0, 0))
        );
        let short = Trail(
            [
                (Tile::new(3, 0), 0),
                (Tile::new(2, 0), 1),
                (Tile::new(4, 0), 2),
            ]
            .into(),
        );
        assert_eq!(regrowable(&board, &group, &short, &BTreeSet::new()), None);
    }

    #[test]
    fn the_tile_untouched_longest_grows_back_first() {
        // Cut north from (0, 5) to (0, 0), walked back south over the middle
        // and cut on to (0, 9), where the group stands: the middle was cut
        // first but touched last, so the north end goes first.
        let board = Board::from_rows(&["."; 10]);
        let group = Group::new(GroupId(1), RivalId(0), Tile::new(0, 9));
        let touched = [5, 10, 9, 8, 7, 6, 11, 12, 13, 14];
        let trail = Trail(
            [5, 4, 3, 2, 1, 0, 6, 7, 8, 9]
                .into_iter()
                .map(|y| (Tile::new(0, y), touched[y as usize]))
                .collect(),
        );
        assert_eq!(
            regrowable(&board, &group, &trail, &BTreeSet::new()),
            Some(Tile::new(0, 0))
        );
    }

    #[test]
    fn a_clearing_nobody_stands_on_grows_back_at_once() {
        let mut board = Board::from_rows(&[".", ".", ".", ".", ".", "."]);
        let tuning = Tuning::default();
        let rival = RivalId(0);
        let group = Group::new(GroupId(1), rival, Tile::new(0, 5));
        let mut trail = Trail::default();
        for y in [0, 1, 4, 5] {
            board.set_open(Tile::new(0, y), Some(rival));
            trail.push(Tile::new(0, y), 0);
        }
        let mut bodies = vec![body(group, trail, Line::default())];
        advance(&mut board, &tuning, 1, &mut bodies, &BTreeMap::new());
        let left: Vec<Tile> = bodies[0].trail.tiles().collect();
        assert!(!left.contains(&Tile::new(0, 0)) && !left.contains(&Tile::new(0, 1)));
        assert_eq!(board.open(Tile::new(0, 0)), None);
        assert!(left.contains(&Tile::new(0, 4)));
    }

    #[test]
    fn gathering_takes_the_tiles_nearest_the_target() {
        let board = Board::from_rows(&[".......", ".......", ".......", "......."]);
        let crowd = Crowd::new();
        let owners = Owners::new(&board, &[]);
        let ground = Reach {
            board: &board,
            rival: RivalId(0),
            open: BTreeSet::new(),
            owners: &owners,
            crowd: &crowd,
            i: 0,
        };
        let home: BTreeSet<Tile> = home_tiles(&ground, Tile::new(3, 1), 5, &BTreeSet::new())
            .into_keys()
            .collect();
        let expected: BTreeSet<Tile> = [(3, 1), (3, 0), (2, 1), (4, 1), (3, 2)]
            .into_iter()
            .map(|(x, y)| Tile::new(x, y))
            .collect();
        assert_eq!(home, expected);
    }

    #[test]
    fn followers_keep_within_the_gap() {
        let mut board = Board::from_rows(&[&".".repeat(30)]);
        let tuning = Tuning::default();
        let rival = RivalId(0);
        let mut group = Group::new(GroupId(1), rival, Tile::new(10, 0));
        for x in [9, 8] {
            group.members.push(Member::at(Tile::new(x, 0), 0));
        }
        let mut trail = Trail::default();
        for x in 0..=10 {
            board.set_open(Tile::new(x, 0), Some(rival));
            trail.push(Tile::new(x, 0), x as u64);
        }
        let line = Line((11..20).map(|x| Tile::new(x, 0)).collect());
        let mut bodies = vec![body(group, trail, line)];
        for step in 0..2000 {
            advance(&mut board, &tuning, step, &mut bodies, &BTreeMap::new());
            let g = &bodies[0].group;
            let mut xs: Vec<i32> = g.tiles().map(|t| t.x).collect();
            xs.sort();
            assert!(xs[2] - xs[1] <= 2 + 1 && xs[1] - xs[0] <= 4 + 1, "{xs:?}");
        }
        let g = &bodies[0].group;
        assert_eq!(g.tile(), Tile::new(19, 0));
    }
}
