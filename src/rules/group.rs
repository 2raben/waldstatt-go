//! Groups of Waldleute: where each of them stands, the line the group cuts
//! along, and the clearing it holds open (ADRs 0001 and 0007).
//!
//! Every Waldmensch stands on a whole tile of land, or with others on the
//! water. One of them, the lead, cuts along the group's line; the others
//! follow over the clearing, never more than [`Tuning::gap`] tiles apart, and
//! when the line is done they gather as near to its end as they can, cutting
//! where they must. What a group has cut is its trail: while it cuts, only
//! `line_limit(size)` tiles stay open, and while it stands still the trail
//! grows back tile by tile, where its Waldleute have not been for longest, but
//! never under them or between them. Moving them is in `moves.rs`.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use bevy::prelude::*;

use super::board::{Board, Ground, RivalId, Tile};
use super::tuning::{Tuning, line_limit};

/// A group's number, counted per game; stable and ordered, unlike entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

/// One Waldmensch: the tile it stands on, the tile it came from and the step
/// it entered (for drawing the step), and its work towards the next tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub tile: Tile,
    pub from: Tile,
    pub entered: u64,
    pub progress: u32,
    /// The tile its work goes towards.
    pub toward: Option<Tile>,
}

impl Member {
    pub fn at(tile: Tile, step: u64) -> Self {
        Self {
            tile,
            from: tile,
            entered: step,
            progress: 0,
            toward: None,
        }
    }

    /// Moves onto `tile` this step.
    pub fn enter(&mut self, tile: Tile, step: u64) {
        self.from = self.tile;
        self.tile = tile;
        self.entered = step;
        self.progress = 0;
        self.toward = None;
    }
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub rival: RivalId,
    pub trees: u32,
    /// The Waldleute; never empty.
    pub members: Vec<Member>,
    /// The Waldmensch the line starts from.
    pub lead: usize,
    /// Where the group gathers when its line is done.
    pub target: Tile,
    /// Steps it has stood without a line, for its trail growing back.
    pub still: u32,
    /// Water tiles it crossed that its Waldleute may still need to cross.
    pub way: Vec<Tile>,
    /// The tile of its trail that grows back next, when one is about to.
    pub regrows: Option<Tile>,
    /// Where its line leads, to find another way there when it is blocked.
    pub goal: Option<Tile>,
    /// Tiles its lead may still go towards the goal.
    pub budget: u32,
}

impl Group {
    /// A group of one on `tile`.
    pub fn new(id: GroupId, rival: RivalId, tile: Tile) -> Self {
        Self {
            id,
            rival,
            trees: 0,
            members: vec![Member::at(tile, 0)],
            lead: 0,
            target: tile,
            still: 0,
            way: Vec::new(),
            regrows: None,
            goal: None,
            budget: 0,
        }
    }

    /// Waldleute in the group.
    pub fn size(&self) -> u32 {
        self.members.len() as u32
    }

    pub fn lead(&self) -> &Member {
        &self.members[self.lead.min(self.members.len() - 1)]
    }

    /// Where the lead stands: where lines start and the group is shown.
    pub fn tile(&self) -> Tile {
        self.lead().tile
    }

    /// Every tile a Waldmensch of the group stands on.
    pub fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        self.members.iter().map(|m| m.tile)
    }

    /// The first Waldmensch on `tile`.
    pub fn member_on(&self, tile: Tile) -> Option<usize> {
        self.members.iter().position(|m| m.tile == tile)
    }
}

/// The tiles a group holds open, in the order they were cut, each with the
/// last step a Waldmensch of the group stood on it (cutting it counts).
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Trail(pub VecDeque<(Tile, u64)>);

impl Trail {
    pub fn tiles(&self) -> impl Iterator<Item = Tile> + '_ {
        self.0.iter().map(|&(tile, _)| tile)
    }

    pub fn contains(&self, tile: Tile) -> bool {
        self.0.iter().any(|&(t, _)| t == tile)
    }

    pub fn push(&mut self, tile: Tile, step: u64) {
        if !self.contains(tile) {
            self.0.push_back((tile, step));
        }
    }

    pub fn remove(&mut self, tile: Tile) {
        self.0.retain(|&(t, _)| t != tile);
    }
}

/// The tiles still to go for the lead, each one step from the one before.
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

/// What the group's lead is doing.
pub fn activity(board: &Board, group: &Group, line: &Line) -> Activity {
    let here = board.ground(group.tile());
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

/// What it takes the lead to enter the next tile of its line now, or `None`
/// if it may not: another rival's open tile, a forest tile in another rival's
/// margin, a tile another rival's Waldmensch stands on, a ship too small, or
/// too few trees. Wading into a river pays the trees for every river tile
/// ahead at once, so a group never gets stuck midstream for want of trees.
pub fn plan_step(
    board: &Board,
    tuning: &Tuning,
    group: &Group,
    ahead: &VecDeque<Tile>,
    other_rival_there: bool,
) -> Option<StepPlan> {
    let &to = ahead.front()?;
    let at = group.tile();
    if !board.can_step(at, to) || other_rival_there {
        return None;
    }
    let plan = |kind, work, trees| Some(StepPlan { kind, work, trees });
    let here = board.ground(at);
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
            } else if group.size() <= tuning.ship_capacity && group.trees >= tuning.ship_trees {
                plan(Move::Launch, tuning.ship_work, tuning.ship_trees)
            } else {
                None
            }
        }
    }
}

/// The line a group gets for the tiles drawn from its lead: steps to a
/// neighbour, each one tile, at most `line_limit(size)` tiles long, never onto
/// a lake for a group too big for a ship, and never ending in a river.
/// Whether each tile can be cut is only known when the lead gets there.
pub fn plan_line(board: &Board, tuning: &Tuning, group: &Group, tiles: &[Tile]) -> VecDeque<Tile> {
    let limit = line_limit(group.size());
    let mut line = VecDeque::new();
    let mut at = group.tile();
    for &tile in tiles {
        let too_big =
            matches!(board.ground(tile), Ground::Lake(_)) && group.size() > tuning.ship_capacity;
        if line.len() >= limit || too_big {
            break;
        }
        if !board.can_step(at, tile) {
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

/// Joins two trails in the order their tiles were cut.
pub fn merge_trails(a: &Trail, b: &Trail) -> Trail {
    let mut tiles: Vec<(Tile, u64)> = a.0.iter().chain(&b.0).copied().collect();
    tiles.sort_by_key(|&(tile, step)| (step, tile));
    tiles.dedup_by_key(|&mut (tile, _)| tile);
    Trail(tiles.into())
}

/// Shortest ways over the board, cheapest first, ties by hops and tile.
pub struct Paths {
    /// For each tile reached: its cost, its hops, and the tile before it on
    /// the way from the nearest source.
    pub reached: BTreeMap<Tile, (u32, u32, Tile)>,
}

impl Paths {
    /// Searches from `sources` over `steps` until every tile in `wanted` is
    /// reached or `limit` tiles are. `cost` is what entering a tile costs,
    /// `None` if it cannot be entered; tiles where `through` is false are
    /// reached but not gone through.
    pub fn search<S: IntoIterator<Item = Tile>>(
        sources: &[Tile],
        steps: impl Fn(Tile) -> S,
        cost: impl Fn(Tile) -> Option<u32>,
        through: impl Fn(Tile) -> bool,
        wanted: &BTreeSet<Tile>,
        limit: usize,
    ) -> Self {
        let mut reached = BTreeMap::new();
        let mut heap = BinaryHeap::new();
        for &s in sources {
            heap.push(Reverse((0u32, 0u32, s, s)));
        }
        let mut found = 0;
        while let Some(Reverse((c, hops, tile, before))) = heap.pop() {
            if reached.contains_key(&tile) {
                continue;
            }
            reached.insert(tile, (c, hops, before));
            if wanted.contains(&tile) {
                found += 1;
                if found >= wanted.len() {
                    break;
                }
            }
            if reached.len() >= limit || (hops > 0 && !through(tile)) {
                continue;
            }
            for next in steps(tile) {
                if !reached.contains_key(&next)
                    && let Some(step) = cost(next)
                {
                    heap.push(Reverse((c + step, hops + 1, next, tile)));
                }
            }
        }
        Self { reached }
    }

    pub fn cost(&self, tile: Tile) -> Option<u32> {
        self.reached.get(&tile).map(|r| r.0)
    }

    pub fn hops(&self, tile: Tile) -> Option<u32> {
        self.reached.get(&tile).map(|r| r.1)
    }

    /// The next tile from `tile` towards the sources.
    pub fn next(&self, tile: Tile) -> Option<Tile> {
        self.reached
            .get(&tile)
            .map(|r| r.2)
            .filter(|&before| before != tile)
    }
}

/// The members of a group in the order they follow the lead: nearest first
/// over its clearing and the water it crossed. Members that cannot be reached
/// come last.
pub fn file_order(board: &Board, group: &Group, trail: &Trail) -> Vec<usize> {
    file_order_from(board, group, trail, group.lead)
}

/// The members of a group nearest first to its Waldmensch `first` over its
/// clearing and the water it crossed, `first` first. Members that cannot be
/// reached come last.
fn file_order_from(board: &Board, group: &Group, trail: &Trail, first: usize) -> Vec<usize> {
    let open: BTreeSet<Tile> = trail.tiles().chain(group.way.iter().copied()).collect();
    let wanted: BTreeSet<Tile> = group.tiles().collect();
    let paths = Paths::search(
        &[group.members[first].tile],
        |t| board.steps(t),
        |t| open.contains(&t).then_some(1),
        |_| true,
        &wanted,
        usize::MAX,
    );
    let mut order: Vec<usize> = (0..group.members.len()).collect();
    order.sort_by_key(|&i| {
        let rank = if i == first { 0 } else { 1 };
        (
            rank,
            paths.hops(group.members[i].tile).unwrap_or(u32::MAX),
            i,
        )
    });
    order
}

/// Splits a group in two. The new group takes half of the Waldleute (rounded
/// down) and that share of the trees; each tile of the trail goes to the group
/// whose Waldmensch stands nearest. Without `leader`, the half furthest back
/// from the lead goes, led by its Waldmensch nearest the old lead. With
/// `leader`, the half nearest the Waldmensch standing there goes, led by it;
/// if that was the lead, the one nearest it of those staying leads them. Each
/// group gathers where its lead stands.
pub fn split_group(
    board: &Board,
    group: &mut Group,
    trail: &mut Trail,
    id: GroupId,
    leader: Option<Tile>,
) -> Option<(Group, Trail)> {
    if group.size() < 2 {
        return None;
    }
    let take = group.members.len() / 2;
    let (leaving, new_lead_index, staying_lead) =
        match leader.and_then(|tile| group.member_on(tile)) {
            Some(first) => {
                let order = file_order_from(board, group, trail, first);
                let staying_lead = if order[..take].contains(&group.lead) {
                    order[take]
                } else {
                    group.lead
                };
                (order[..take].to_vec(), first, staying_lead)
            }
            None => {
                let order = file_order(board, group, trail);
                let rear = order[order.len() - take..].to_vec();
                (rear, order[order.len() - take], group.lead)
            }
        };
    let leaving: BTreeSet<usize> = leaving.into_iter().collect();
    let trees = group.trees * take as u32 / group.size();
    group.trees -= trees;
    let mut staying = Vec::new();
    let mut moving = Vec::new();
    let (mut new_lead, mut old_lead) = (0, 0);
    for (i, member) in group.members.iter().enumerate() {
        if leaving.contains(&i) {
            if i == new_lead_index {
                new_lead = moving.len();
            }
            moving.push(*member);
        } else {
            if i == staying_lead {
                old_lead = staying.len();
            }
            staying.push(*member);
        }
    }
    group.members = staying;
    if group.lead != staying_lead {
        group.target = group.members[old_lead].tile;
    }
    group.lead = old_lead;
    let mut new = Group {
        id,
        rival: group.rival,
        trees,
        members: moving,
        lead: new_lead,
        target: group.target,
        still: 0,
        way: group.way.clone(),
        regrows: None,
        goal: None,
        budget: 0,
    };
    new.target = new.tile();
    let nearest = |tile: Tile, g: &Group| {
        g.tiles()
            .map(|t| t.distance(tile))
            .min()
            .unwrap_or(i32::MAX)
    };
    let (keep, give): (VecDeque<_>, VecDeque<_>) = trail
        .0
        .iter()
        .partition(|&&(tile, _)| nearest(tile, group) <= nearest(tile, &new));
    *trail = Trail(keep);
    Some((new, Trail(give)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_at(tile: Tile, size: u32) -> Group {
        let mut group = Group::new(GroupId(0), RivalId(0), tile);
        for k in 1..size {
            group
                .members
                .push(Member::at(Tile::new(tile.x - k as i32, tile.y), 0));
        }
        group
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
    fn a_line_steps_from_neighbour_to_neighbour() {
        let board = Board::from_rows(&["....", " ....", "~...", " ...."]);
        let tuning = Tuning::default();
        let group = group_at(Tile::new(0, 0), 1);
        // South-east twice, then into the river south-west: never ending there.
        let down = [Tile::new(0, 1), Tile::new(1, 2), Tile::new(1, 3)];
        assert_eq!(plan_line(&board, &tuning, &group, &down).len(), 3);
        let into = [Tile::new(0, 1), Tile::new(0, 2)];
        assert_eq!(plan_line(&board, &tuning, &group, &into).len(), 1);
        // A tile two steps away is no step.
        assert!(plan_line(&board, &tuning, &group, &[Tile::new(1, 1)]).is_empty());
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
        board.set_open(Tile::new(2, 1), Some(RivalId(1)));
        let mut group = group_at(Tile::new(1, 0), 1);
        let step = |board: &Board, group: &Group, to: Tile| {
            plan_step(board, &tuning, group, &VecDeque::from([to]), false)
        };
        assert_eq!(
            step(&board, &group, Tile::new(0, 0)).map(|p| p.kind),
            Some(Move::Cut)
        );
        // Next to the other rival's open tile: in its margin.
        assert_eq!(step(&board, &group, Tile::new(2, 0)), None);
        group.members[0].tile = Tile::new(3, 0);
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
        group.members[0].tile = Tile::new(1, 0);
        group.trees = 0;
        let midstream: VecDeque<Tile> = (2..5).map(|x| Tile::new(x, 0)).collect();
        assert_eq!(
            plan_step(&board, &tuning, &group, &midstream, false).map(|p| p.trees),
            Some(0)
        );
    }

    #[test]
    fn the_file_follows_the_clearing_from_the_lead() {
        // Lead at (6, 0), the others behind it along the trail.
        let mut group = group_at(Tile::new(6, 0), 3);
        group.members.swap(0, 2);
        group.lead = 2;
        let trail = Trail((0..=6).map(|x| (Tile::new(x, 0), x as u64)).collect());
        let board = Board::from_rows(&[&".".repeat(10)]);
        let order = file_order(&board, &group, &trail);
        let tiles: Vec<i32> = order.iter().map(|&i| group.members[i].tile.x).collect();
        assert_eq!(tiles, vec![6, 5, 4]);
    }

    #[test]
    fn splitting_sends_the_rear_half_away_with_its_share() {
        let mut group = group_at(Tile::new(9, 0), 5);
        group.trees = 10;
        let mut trail = Trail((0..=9).map(|x| (Tile::new(x, 0), x as u64)).collect());
        let board = Board::from_rows(&[&".".repeat(12)]);
        let (new, new_trail) =
            split_group(&board, &mut group, &mut trail, GroupId(1), None).expect("splits");
        assert_eq!((group.size(), new.size()), (3, 2));
        assert_eq!((group.trees, new.trees), (6, 4));
        assert_eq!(group.tile(), Tile::new(9, 0), "the lead stays");
        let rear: Vec<i32> = new.tiles().map(|t| t.x).collect();
        assert!(rear.contains(&5) && rear.contains(&6), "{rear:?}");
        assert_eq!(
            new.tile(),
            Tile::new(6, 0),
            "its lead is nearest the old one"
        );
        assert_eq!(trail.0.len() + new_trail.0.len(), 10);
        assert!(new_trail.contains(Tile::new(0, 0)));
        assert!(trail.contains(Tile::new(9, 0)));
    }

    #[test]
    fn splitting_from_a_waldmensch_sends_the_half_nearest_it() {
        let mut group = group_at(Tile::new(9, 0), 5);
        let mut trail = Trail((5..=9).map(|x| (Tile::new(x, 0), x as u64)).collect());
        let board = Board::from_rows(&[&".".repeat(12)]);
        let (new, _) = split_group(
            &board,
            &mut group,
            &mut trail,
            GroupId(1),
            Some(Tile::new(9, 0)),
        )
        .expect("splits");
        let mut leaving: Vec<i32> = new.tiles().map(|t| t.x).collect();
        leaving.sort();
        assert_eq!(leaving, vec![8, 9]);
        assert_eq!(new.tile(), Tile::new(9, 0), "led by the one pressed");
        assert_eq!(group.tile(), Tile::new(7, 0), "the nearest left leads");
        assert_eq!(group.target, Tile::new(7, 0));
    }
}
