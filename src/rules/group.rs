//! Groups of Waldleute: where they stand, the line they cut along, and the
//! trail they leave (ADR 0001).
//!
//! A group's position is a whole tile. Each step it adds work towards the next
//! tile of its line; when the work reaches that tile's cost it enters it. What
//! it has cut is its trail, and only the last `line_limit(size)` tiles of a
//! trail stay open: the oldest grows back as a new one is cut.

use std::collections::{BTreeSet, VecDeque};

use bevy::prelude::*;

use super::board::{Board, Ground, RivalId, Tile};
use super::game::{Game, GameEvent, Rival};
use super::tuning::{Tuning, line_limit};

/// A group's number, counted per game; stable and ordered, unlike entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub rival: RivalId,
    /// Waldleute in the group.
    pub size: u32,
    pub trees: u32,
    pub tile: Tile,
    /// The tile it came from, for drawing the step.
    pub from: Tile,
    /// The step it entered its tile.
    pub entered: u64,
    /// Work done towards the next tile of its line.
    pub progress: u32,
}

/// The tiles a group has cut and that are still open, oldest first, with the
/// step each was cut.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Trail(pub VecDeque<(Tile, u64)>);

impl Trail {
    pub fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        self.0.iter().map(|&(tile, _)| tile)
    }

    /// The tile that grows back when the group cuts its next one.
    pub fn closes_next(&self, size: u32) -> Option<Tile> {
        (self.0.len() >= line_limit(size))
            .then(|| self.0.front().map(|&(tile, _)| tile))
            .flatten()
    }
}

/// The tiles still to go, each one straight step from the one before.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Line(pub VecDeque<Tile>);

/// What entering the next tile means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// Cutting a forest tile, which opens it and earns its trees.
    Cut,
    /// Walking onto a tile the rival holds open.
    Walk,
    /// Wading into a river.
    Wade,
    /// Building a ship on the shore and launching it.
    Launch,
    /// Sailing on a lake.
    Sail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepPlan {
    pub kind: Move,
    pub work: u32,
    pub trees: u32,
}

/// What a group is doing, for showing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Cutting,
    Walking,
    Wading,
    InRiver,
    BuildingShip,
    Afloat,
}

impl Activity {
    pub fn label(self) -> &'static str {
        match self {
            Activity::Idle => "wartet",
            Activity::Cutting => "fällt Bäume",
            Activity::Walking => "geht",
            Activity::Wading => "watet in den Fluss",
            Activity::InRiver => "im Fluss",
            Activity::BuildingShip => "baut ein Schiff",
            Activity::Afloat => "auf dem See",
        }
    }
}

/// Whether a group cannot be enclosed: it is afloat or crossing a river.
pub fn is_safe_on_water(board: &Board, group: &Group) -> bool {
    board.is_water(group.tile)
}

pub fn activity(board: &Board, group: &Group, line: &Line) -> Activity {
    let here = board.ground(group.tile);
    match (here, line.0.front().map(|&t| board.ground(t))) {
        (Ground::Lake(_), _) => Activity::Afloat,
        (Ground::River, _) => Activity::InRiver,
        (_, None) => Activity::Idle,
        (_, Some(Ground::River)) => Activity::Wading,
        (_, Some(Ground::Lake(_))) => Activity::BuildingShip,
        (_, Some(Ground::Land)) => {
            if line.0.front().and_then(|&t| board.open(t)) == Some(group.rival) {
                Activity::Walking
            } else {
                Activity::Cutting
            }
        }
    }
}

/// What it takes the group to enter the next tile of its line now, or `None`
/// if it may not: another rival's open tile, a forest tile in another rival's
/// margin, a tile another rival's group stands on, a ship too small, or too
/// few trees. Wading into a river pays the trees for every river tile ahead
/// at once, so a group never gets stuck midstream for want of trees.
pub fn plan_step(
    board: &Board,
    tuning: &Tuning,
    group: &Group,
    ahead: &VecDeque<Tile>,
    other_rival_there: bool,
) -> Option<StepPlan> {
    let &to = ahead.front()?;
    if !board.contains(to) || !group.tile.is_next_to(to) || other_rival_there {
        return None;
    }
    let plan = |kind, work, trees| Some(StepPlan { kind, work, trees });
    let here = board.ground(group.tile);
    match board.ground(to) {
        Ground::Land => match board.open(to) {
            Some(rival) if rival == group.rival => plan(Move::Walk, tuning.walk_work, 0),
            Some(_) => None,
            None if board.in_margin_of_other(to, group.rival) => None,
            None => plan(Move::Cut, tuning.cut_work, 0),
        },
        Ground::River if here == Ground::River => plan(Move::Wade, tuning.river_work, 0),
        Ground::River => {
            let crossing = ahead
                .iter()
                .take_while(|&&t| board.ground(t) == Ground::River)
                .count() as u32;
            let trees = tuning.river_trees * crossing;
            (group.trees >= trees).then_some(StepPlan {
                kind: Move::Wade,
                work: tuning.river_work,
                trees,
            })
        }
        Ground::Lake(_) => {
            if matches!(here, Ground::Lake(_)) {
                plan(Move::Sail, tuning.sail_work, 0)
            } else if group.size <= tuning.ship_capacity && group.trees >= tuning.ship_trees {
                plan(Move::Launch, tuning.ship_work, tuning.ship_trees)
            } else {
                None
            }
        }
    }
}

/// The line a group gets for the tiles drawn from it: straight steps from its
/// tile, at most `line_limit(size)` long, never onto a lake for a group too big
/// for a ship, and never ending in a river. Whether each tile can be cut is
/// only known when the group gets there; then the line stops short.
pub fn plan_line(board: &Board, tuning: &Tuning, group: &Group, tiles: &[Tile]) -> VecDeque<Tile> {
    let mut line = VecDeque::new();
    let mut at = group.tile;
    for &tile in tiles.iter().take(line_limit(group.size)) {
        let too_big =
            matches!(board.ground(tile), Ground::Lake(_)) && group.size > tuning.ship_capacity;
        if !board.contains(tile) || !at.is_next_to(tile) || too_big {
            break;
        }
        line.push_back(tile);
        at = tile;
    }
    while line
        .back()
        .is_some_and(|&t| board.ground(t) == Ground::River)
    {
        line.pop_back();
    }
    line
}

/// Lets the oldest tiles of a trail grow back until it is `limit` long.
pub fn trim_trail(board: &mut Board, rival: RivalId, trail: &mut Trail, limit: usize) {
    while trail.0.len() > limit {
        let Some((tile, _)) = trail.0.pop_front() else {
            break;
        };
        if board.open(tile) == Some(rival) {
            board.set_open(tile, None);
        }
    }
}

/// Joins two trails in the order their tiles were cut.
pub fn merge_trails(a: &Trail, b: &Trail) -> Trail {
    let mut tiles: Vec<(Tile, u64)> = a.0.iter().chain(&b.0).copied().collect();
    tiles.sort_by_key(|&(tile, step)| (step, tile));
    Trail(tiles.into())
}

/// Splits a group in two on its tile. The new group takes half the Waldleute
/// (rounded down) and that share of the trees. The old group keeps the newest
/// part of the trail it may hold; the new one takes the part before, up to its
/// own limit; anything older grows back.
pub fn split_group(
    board: &mut Board,
    group: &mut Group,
    trail: &mut Trail,
    id: GroupId,
) -> Option<(Group, Trail)> {
    if group.size < 2 {
        return None;
    }
    let size = group.size / 2;
    let trees = group.trees * size / group.size;
    group.size -= size;
    group.trees -= trees;
    let keep = trail.0.len().min(line_limit(group.size));
    let mut older: VecDeque<(Tile, u64)> = trail.0.drain(..trail.0.len() - keep).collect();
    let mut new_trail = Trail(older.split_off(older.len().saturating_sub(line_limit(size))));
    for (tile, _) in older {
        board.set_open(tile, None);
    }
    trim_trail(board, group.rival, &mut new_trail, line_limit(size));
    let new = Group {
        id,
        size,
        trees,
        from: group.tile,
        progress: 0,
        ..group.clone()
    };
    Some((new, new_trail))
}

/// Moves every group along its line, in the game's rival order and then by
/// group number, so two rivals reaching the same tile always resolve the same
/// way. A group whose line ends on a tile with another group of its own merges
/// into it.
pub fn advance_groups(
    mut commands: Commands,
    mut board: ResMut<Board>,
    tuning: Res<Tuning>,
    mut game: ResMut<Game>,
    mut rivals: Query<&mut Rival>,
    mut groups: Query<(Entity, &mut Group, &mut Trail, &mut Line)>,
) {
    let mut order: Vec<(usize, GroupId, Entity)> = groups
        .iter()
        .map(|(entity, group, ..)| (game.rank(group.rival), group.id, entity))
        .collect();
    order.sort();
    // Where every group stands, kept current as they move.
    let mut places: Vec<(Entity, GroupId, RivalId, Tile)> = groups
        .iter()
        .map(|(entity, group, ..)| (entity, group.id, group.rival, group.tile))
        .collect();
    let mut merged = BTreeSet::new();
    let step = game.step;

    for (_, _, entity) in order {
        if merged.contains(&entity) {
            continue;
        }
        let Ok((_, mut group, mut trail, mut line)) = groups.get_mut(entity) else {
            continue;
        };
        let Some(&next) = line.0.front() else {
            group.progress = 0;
            continue;
        };
        let rival = group.rival;
        let other_there = places
            .iter()
            .any(|&(e, _, r, t)| t == next && r != rival && !merged.contains(&e));
        let Some(plan) = plan_step(&board, &tuning, &group, &line.0, other_there) else {
            // The line runs into something it may not cross: it stops short.
            line.0.clear();
            group.progress = 0;
            continue;
        };
        group.progress += tuning.speed(group.size);
        if group.progress < plan.work {
            continue;
        }
        group.progress = 0;
        line.0.pop_front();
        group.from = group.tile;
        group.tile = next;
        group.entered = step;
        group.trees -= plan.trees;
        if plan.kind == Move::Cut {
            board.set_open(next, Some(rival));
            trail.0.push_back((next, step));
            group.trees += board.trees(next);
            trim_trail(&mut board, rival, &mut trail, line_limit(group.size));
        }
        if let Some(place) = places.iter_mut().find(|p| p.0 == entity) {
            place.3 = next;
        }
        if !line.0.is_empty() || board.is_water(next) {
            continue;
        }
        let target = places
            .iter()
            .filter(|&&(e, _, r, t)| e != entity && r == rival && t == next && !merged.contains(&e))
            .min_by_key(|&&(_, id, ..)| id)
            .map(|&(e, ..)| e);
        let Some(target) = target else {
            continue;
        };
        let Ok([(_, mover, mover_trail, _), (_, mut into, mut into_trail, _)]) =
            groups.get_many_mut([entity, target])
        else {
            continue;
        };
        into.size += mover.size;
        into.trees += mover.trees;
        *into_trail = merge_trails(&into_trail, &mover_trail);
        let limit = line_limit(into.size);
        trim_trail(&mut board, rival, &mut into_trail, limit);
        for mut owner in &mut rivals {
            if owner.id == rival && owner.receiver == Some(mover.id) {
                owner.receiver = Some(into.id);
            }
        }
        game.events.push(GameEvent::Merged {
            step,
            rival,
            from: mover.id,
            into: into.id,
        });
        merged.insert(entity);
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_at(tile: Tile, size: u32) -> Group {
        Group {
            id: GroupId(0),
            rival: RivalId(0),
            size,
            trees: 0,
            tile,
            from: tile,
            entered: 0,
            progress: 0,
        }
    }

    #[test]
    fn a_line_is_straight_steps_up_to_the_limit() {
        let board = Board::from_rows(&[&".".repeat(30)]);
        let tuning = Tuning::default();
        let group = group_at(Tile::new(0, 0), 1);
        let tiles: Vec<Tile> = (1..30).map(|x| Tile::new(x, 0)).collect();
        assert_eq!(plan_line(&board, &tuning, &group, &tiles).len(), 10);
        let jump = [Tile::new(1, 0), Tile::new(3, 0)];
        assert_eq!(plan_line(&board, &tuning, &group, &jump).len(), 1);
    }

    #[test]
    fn a_line_does_not_end_in_a_river() {
        let board = Board::from_rows(&["..~~.."]);
        let tuning = Tuning::default();
        let group = group_at(Tile::new(0, 0), 1);
        let into = [Tile::new(1, 0), Tile::new(2, 0), Tile::new(3, 0)];
        assert_eq!(plan_line(&board, &tuning, &group, &into).len(), 1);
        let across = [
            Tile::new(1, 0),
            Tile::new(2, 0),
            Tile::new(3, 0),
            Tile::new(4, 0),
        ];
        assert_eq!(plan_line(&board, &tuning, &group, &across).len(), 4);
    }

    #[test]
    fn margins_and_trees_decide_each_step() {
        let mut board = Board::from_rows(&["....~", "....."]);
        let tuning = Tuning::default();
        board.set_open(Tile::new(3, 1), Some(RivalId(1)));
        let mut group = group_at(Tile::new(1, 0), 1);
        let step = |board: &Board, group: &Group, to: Tile| {
            plan_step(board, &tuning, group, &VecDeque::from([to]), false)
        };
        assert_eq!(
            step(&board, &group, Tile::new(0, 0)).map(|p| p.kind),
            Some(Move::Cut)
        );
        // Diagonally next to the other rival's open tile: in its margin.
        assert_eq!(step(&board, &group, Tile::new(2, 0)), None);
        group.tile = Tile::new(3, 0);
        assert_eq!(
            step(&board, &group, Tile::new(4, 0)),
            None,
            "no trees to wade"
        );
        group.trees = tuning.river_trees;
        assert_eq!(
            step(&board, &group, Tile::new(4, 0)).map(|p| p.kind),
            Some(Move::Wade)
        );
    }

    #[test]
    fn a_crossing_is_paid_in_full_before_wading_in() {
        let board = Board::from_rows(&[".~~~."]);
        let tuning = Tuning::default();
        let mut group = group_at(Tile::new(0, 0), 1);
        let across: VecDeque<Tile> = (1..5).map(|x| Tile::new(x, 0)).collect();
        group.trees = tuning.river_trees * 2;
        assert_eq!(plan_step(&board, &tuning, &group, &across, false), None);
        group.trees = tuning.river_trees * 3;
        let plan = plan_step(&board, &tuning, &group, &across, false).expect("can cross");
        assert_eq!(plan.trees, tuning.river_trees * 3);
        group.tile = Tile::new(1, 0);
        group.trees = 0;
        let midstream: VecDeque<Tile> = (2..5).map(|x| Tile::new(x, 0)).collect();
        assert_eq!(
            plan_step(&board, &tuning, &group, &midstream, false).map(|p| p.trees),
            Some(0)
        );
    }

    #[test]
    fn splitting_shares_trees_and_trail() {
        let mut board = Board::from_rows(&[&".".repeat(40)]);
        let rival = RivalId(0);
        let mut trail = Trail::default();
        for x in 0..30 {
            board.set_open(Tile::new(x, 0), Some(rival));
            trail.0.push_back((Tile::new(x, 0), x as u64));
        }
        let mut group = group_at(Tile::new(29, 0), 4);
        group.trees = 10;
        let (new, new_trail) =
            split_group(&mut board, &mut group, &mut trail, GroupId(1)).expect("splits");
        assert_eq!((group.size, new.size), (2, 2));
        assert_eq!((group.trees, new.trees), (5, 5));
        assert_eq!(trail.0.len(), 20);
        assert_eq!(new_trail.0.len(), 10);
        assert_eq!(trail.0.front().map(|t| t.0), Some(Tile::new(10, 0)));
        assert_eq!(new_trail.0.front().map(|t| t.0), Some(Tile::new(0, 0)));
        assert_eq!(new.tile, group.tile);
    }
}
