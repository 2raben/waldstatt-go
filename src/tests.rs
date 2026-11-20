//! Headless game tests: the real rule systems run at the fixed timestep, and
//! commands are written to [`Controls`] exactly as input and computer rivals
//! write them. No window or GPU is needed.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::ai::ComputerRivalsPlugin;
use crate::rules::game::{EndReason, GameEvent, STEPS_PER_SECOND};
use crate::rules::group::Member;
use crate::rules::waters::{BOARDS, build_board};
use crate::rules::{
    Board, Controls, Game, GameSetup, Group, GroupId, Line, Order, RivalId, RulesPlugin, Seat,
    Tile, Trail, Tuning, start_game,
};

const A: RivalId = RivalId(0);
const B: RivalId = RivalId(1);

struct Sim {
    app: App,
}

impl Sim {
    fn new(setup: GameSetup) -> Self {
        Self::build(setup, false)
    }

    fn with_computers(setup: GameSetup) -> Self {
        Self::build(setup, true)
    }

    fn build(setup: GameSetup, computers: bool) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, RulesPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / STEPS_PER_SECOND as f64,
            )));
        if computers {
            app.add_plugins(ComputerRivalsPlugin);
        }
        app.finish();
        app.cleanup();
        // The first update starts the clock; later ones advance one fixed step each.
        app.update();
        start_game(app.world_mut(), setup);
        Self { app }
    }

    fn steps(&mut self, count: u64) {
        for _ in 0..count {
            self.app.update();
        }
    }

    fn send(&mut self, rival: RivalId, order: Order) {
        self.app
            .world_mut()
            .resource_mut::<Controls>()
            .send(rival, order);
    }

    /// A line from the group's lead.
    fn line(&mut self, rival: RivalId, group: GroupId, tiles: &[(i32, i32)]) {
        let from = self.group(group).tile();
        self.line_from(rival, group, (from.x, from.y), tiles);
    }

    /// A line from the group's Waldmensch on `from`.
    fn line_from(
        &mut self,
        rival: RivalId,
        group: GroupId,
        from: (i32, i32),
        tiles: &[(i32, i32)],
    ) {
        let tiles = tiles.iter().map(|&(x, y)| Tile::new(x, y)).collect();
        let from = Tile::new(from.0, from.1);
        self.send(rival, Order::Line { group, from, tiles });
    }

    /// Adds Waldleute to a group on these tiles, opened for its rival.
    fn add_members(&mut self, id: GroupId, tiles: &[(i32, i32)]) {
        let rival = self.group(id).rival;
        for &(x, y) in tiles {
            let tile = Tile::new(x, y);
            if !self.board().is_water(tile) {
                self.board_mut().set_open(tile, Some(rival));
            }
        }
        let world = self.app.world_mut();
        let mut query = world.query::<(&mut Group, &mut Trail)>();
        if let Some((mut group, mut trail)) = query.iter_mut(world).find(|(g, _)| g.id == id) {
            for &(x, y) in tiles {
                let tile = Tile::new(x, y);
                group.members.push(Member::at(tile, 0));
                trail.push(tile, 0);
            }
        }
    }

    fn groups(&mut self) -> Vec<(Group, Trail, Line)> {
        let world = self.app.world_mut();
        let mut groups: Vec<_> = world
            .query::<(&Group, &Trail, &Line)>()
            .iter(world)
            .map(|(g, t, l)| (g.clone(), t.clone(), l.clone()))
            .collect();
        groups.sort_by_key(|(g, ..)| g.id);
        groups
    }

    fn group(&mut self, id: GroupId) -> Group {
        self.groups()
            .into_iter()
            .find(|(g, ..)| g.id == id)
            .map(|(g, ..)| g)
            .expect("group exists")
    }

    fn trail(&mut self, id: GroupId) -> Trail {
        self.groups()
            .into_iter()
            .find(|(g, ..)| g.id == id)
            .map(|(_, t, _)| t)
            .expect("group exists")
    }

    fn set_group(&mut self, id: GroupId, change: impl FnOnce(&mut Group)) {
        let world = self.app.world_mut();
        let mut query = world.query::<&mut Group>();
        if let Some(mut group) = query.iter_mut(world).find(|g| g.id == id) {
            change(&mut group);
        }
    }

    fn board(&self) -> &Board {
        self.app.world().resource::<Board>()
    }

    fn board_mut(&mut self) -> Mut<'_, Board> {
        self.app.world_mut().resource_mut::<Board>()
    }

    fn game(&self) -> &Game {
        self.app.world().resource::<Game>()
    }

    fn tuning(&self) -> Tuning {
        self.app.world().resource::<Tuning>().clone()
    }

    /// Steps until the group stands on the tile, or panics after `limit` steps.
    fn until_at(&mut self, id: GroupId, tile: (i32, i32), limit: u64) -> u64 {
        for step in 1..=limit {
            self.steps(1);
            if self.group(id).tile() == Tile::new(tile.0, tile.1) {
                return step;
            }
        }
        panic!("group {id:?} did not reach {tile:?} in {limit} steps");
    }

    /// A summary of the whole game state, to compare runs.
    fn fingerprint(&mut self) -> String {
        let board = self.board();
        let open: String = board
            .tiles()
            .map(|t| match board.open(t) {
                Some(r) => char::from(b'a' + r.0),
                None => '.',
            })
            .collect();
        let game = self.game();
        let summary = format!("{} {:?} {}", game.step, game.outcome, game.events.len());
        let groups: Vec<String> = self
            .groups()
            .into_iter()
            .map(|(g, t, l)| format!("{g:?} {} {:?}", t.0.len(), l.0))
            .collect();
        format!("{summary}\n{open}\n{}", groups.join("\n"))
    }
}

fn setup(rows: &[&str], placements: Vec<Vec<(i32, i32)>>) -> GameSetup {
    let placements: Vec<Vec<Tile>> = placements
        .into_iter()
        .map(|tiles| tiles.into_iter().map(|(x, y)| Tile::new(x, y)).collect())
        .collect();
    GameSetup {
        board: Board::from_rows(rows),
        seats: vec![Seat::Player; placements.len()],
        minutes: 10,
        seed: 1,
        placements: Some(placements),
    }
}

fn field(width: usize, height: usize) -> Vec<String> {
    vec![".".repeat(width); height]
}

fn rows(field: &[String]) -> Vec<&str> {
    field.iter().map(String::as_str).collect()
}

/// Groups are numbered from 1 in the order they were placed.
const FIRST: GroupId = GroupId(1);
const SECOND: GroupId = GroupId(2);

fn tiles_of(group: &Group) -> Vec<(i32, i32)> {
    let mut tiles: Vec<(i32, i32)> = group.tiles().map(|t| (t.x, t.y)).collect();
    tiles.sort();
    tiles
}

#[test]
fn cuts_along_a_line_one_tile_after_another() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    let tuning = sim.tuning();
    let per_tile = u64::from(tuning.cut_work.div_ceil(tuning.speed(1)));
    sim.line(A, FIRST, &[(1, 0), (2, 0), (3, 0)]);
    sim.steps(per_tile - 1);
    assert_eq!(sim.group(FIRST).tile(), Tile::new(0, 0));
    sim.steps(1);
    assert_eq!(sim.group(FIRST).tile(), Tile::new(1, 0));
    assert_eq!(sim.board().open(Tile::new(1, 0)), Some(A));
    assert_eq!(sim.group(FIRST).trees, sim.board().trees(Tile::new(1, 0)));
    sim.steps(2 * per_tile);
    assert_eq!(sim.group(FIRST).tile(), Tile::new(3, 0));
    assert!(sim.groups()[0].2.0.is_empty(), "the line is done");
}

#[test]
fn a_bigger_group_cuts_faster() {
    let f = field(30, 3);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(3, 0), (3, 2)]]));
    sim.add_members(SECOND, &[(2, 2), (1, 2), (0, 2)]);
    sim.line(A, FIRST, &[(4, 0)]);
    sim.line(A, SECOND, &[(4, 2)]);
    let big = sim.until_at(SECOND, (4, 2), 200);
    let small = big + sim.until_at(FIRST, (4, 0), 200);
    assert!(small > 2 * big, "one alone takes {small} steps, four {big}");
}

#[test]
fn only_the_last_tiles_of_a_trail_stay_open_while_cutting() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    let line: Vec<(i32, i32)> = (1..=10).map(|x| (x, 0)).collect();
    sim.line(A, FIRST, &line);
    sim.until_at(FIRST, (10, 0), 2000);
    // Eleven tiles cut, counting the first: the first has grown back.
    assert_eq!(sim.trail(FIRST).0.len(), 10);
    assert_eq!(sim.board().open(Tile::new(0, 0)), None);
    assert_eq!(sim.board().open(Tile::new(1, 0)), Some(A));
    // The sapling is worked out four times a second.
    sim.steps(16);
    assert_eq!(sim.group(FIRST).regrows, Some(Tile::new(1, 0)));
}

#[test]
fn standing_still_the_trail_grows_back_to_the_group() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    let every = u64::from(sim.tuning().regrow_steps);
    let after = u64::from(sim.tuning().regrow_after);
    let line: Vec<(i32, i32)> = (1..=6).map(|x| (x, 0)).collect();
    sim.line(A, FIRST, &line);
    sim.until_at(FIRST, (6, 0), 2000);
    assert_eq!(sim.trail(FIRST).0.len(), 7);
    sim.steps(after - 2);
    assert_eq!(sim.trail(FIRST).0.len(), 7, "not at once");
    sim.steps(every * 2 + 1);
    assert_eq!(
        sim.trail(FIRST).0.len(),
        4,
        "one tile a while, the one untouched longest"
    );
    assert_eq!(sim.board().open(Tile::new(2, 0)), None);
    assert_eq!(sim.board().open(Tile::new(3, 0)), Some(A));
    sim.steps(every * 10);
    let trail: Vec<Tile> = sim.trail(FIRST).tiles().collect();
    assert_eq!(
        trail,
        vec![Tile::new(6, 0)],
        "back to the group, and no further"
    );
}

#[test]
fn a_click_sends_the_group_the_fastest_way() {
    let f = field(20, 20);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    let tuning = sim.tuning();
    let per_tile = u64::from(tuning.cut_work.div_ceil(tuning.speed(1)));
    let goto = |sim: &mut Sim, x, y| {
        let from = sim.group(FIRST).tile();
        sim.send(
            A,
            Order::Goto {
                group: FIRST,
                from,
                to: Tile::new(x, y),
            },
        );
    };
    goto(&mut sim, 5, 5);
    let took = sim.until_at(FIRST, (5, 5), 2000);
    assert!(
        took <= 8 * per_tile + 2,
        "eight tiles, one per step: {took} steps"
    );
    // No further than the tiles a group of one may go: ten.
    goto(&mut sim, 19, 5);
    sim.steps(20 * per_tile);
    assert_eq!(sim.group(FIRST).tile(), Tile::new(15, 5));
}

#[test]
fn a_blocked_line_finds_another_way_to_its_end() {
    let f = field(14, 7);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 3)], vec![(6, 3)]]));
    sim.add_members(FIRST, &[(0, 4)]);
    let line: Vec<(i32, i32)> = (1..=11).map(|x| (x, 3)).collect();
    sim.line(A, FIRST, &line);
    sim.until_at(FIRST, (11, 3), 3000);
    for x in [5, 7] {
        assert_eq!(sim.board().open(Tile::new(x, 3)), None, "round B's margin");
    }
}

#[test]
fn a_line_with_no_way_round_stops_short() {
    let f = field(12, 3);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)], vec![(6, 1)]]));
    sim.line(
        A,
        FIRST,
        &[(1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0), (7, 0)],
    );
    sim.steps(2000);
    let group = sim.group(FIRST);
    // (6, 0) touches B's tile (6, 1), which lies south-east of (5, 0)'s
    // neighbour; the margin closes the row and there is no way round.
    assert_eq!(group.tile(), Tile::new(5, 0));
    assert!(sim.groups()[0].2.0.is_empty());
    assert_eq!(sim.board().open(Tile::new(6, 0)), None);
}

#[test]
fn cutting_next_to_another_clearing_merges_the_groups() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0), (4, 0)]]));
    sim.line(A, FIRST, &[(1, 0), (2, 0)]);
    sim.until_at(FIRST, (2, 0), 1000);
    assert_eq!(sim.groups().len(), 2, "not yet touching");
    sim.line(A, FIRST, &[(3, 0)]);
    for _ in 0..1000 {
        sim.steps(1);
        if sim.groups().len() == 1 {
            break;
        }
    }
    let groups = sim.groups();
    assert_eq!(groups.len(), 1);
    let (group, trail, _) = &groups[0];
    assert_eq!(group.id, FIRST, "the group that cut takes the other in");
    assert_eq!(group.size(), 2);
    assert_eq!(trail.0.len(), 5);
    assert!(sim.game().events.iter().any(
        |e| matches!(e, GameEvent::Merged { from, into, .. } if *from == SECOND && *into == FIRST)
    ));
}

#[test]
fn the_waldleute_follow_whoever_leads() {
    let f = field(10, 8);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(5, 0)]]));
    sim.add_members(FIRST, &[(4, 0), (3, 0), (2, 0)]);
    // Drag the one in the middle south.
    sim.line_from(A, FIRST, (3, 0), &[(3, 1), (3, 2), (3, 3), (3, 4), (3, 5)]);
    sim.steps(1);
    assert_eq!(sim.group(FIRST).tile(), Tile::new(3, 0), "it leads now");
    sim.until_at(FIRST, (3, 5), 3000);
    sim.steps(STEPS_PER_SECOND * 10);
    let group = sim.group(FIRST);
    assert_eq!(group.tile(), Tile::new(3, 5));
    for (x, y) in tiles_of(&group) {
        assert!(
            (x - 3).abs() + (y - 5).abs() <= 2,
            "gathered round the end: {x},{y}"
        );
    }
}

#[test]
fn standing_still_they_gather_round_the_end_of_their_line() {
    let f = field(20, 9);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(4, 4)]]));
    sim.add_members(FIRST, &[(3, 4), (2, 4), (1, 4), (0, 4)]);
    let line: Vec<(i32, i32)> = (5..=12).map(|x| (x, 4)).collect();
    sim.line(A, FIRST, &line);
    sim.until_at(FIRST, (12, 4), 3000);
    sim.steps(STEPS_PER_SECOND * 30);
    let group = sim.group(FIRST);
    // The end and four of its six neighbours; of those equally near, the
    // northern ones.
    let expected = vec![(11, 3), (11, 4), (12, 3), (12, 4), (13, 4)];
    assert_eq!(tiles_of(&group), expected, "as near to the end as they can");
    let mut trail: Vec<(i32, i32)> = sim.trail(FIRST).tiles().map(|t| (t.x, t.y)).collect();
    trail.sort();
    assert_eq!(trail, expected, "and the rest of the trail has grown back");
}

#[test]
fn gathering_stops_at_another_clearing_of_the_same_rival() {
    let f = field(12, 5);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(5, 2), (7, 2)]]));
    sim.add_members(FIRST, &[(4, 2), (3, 2)]);
    sim.steps(STEPS_PER_SECOND * 10);
    assert_eq!(sim.groups().len(), 2, "gathering never merges");
    // (6, 2) touches the second group, so the first gathers round its other sides.
    assert_eq!(sim.board().open(Tile::new(6, 2)), None);
    assert!(tiles_of(&sim.group(FIRST)).contains(&(5, 2)));
}

#[test]
fn splitting_sends_the_rear_half_away() {
    let f = field(30, 5);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(4, 0)]]));
    sim.add_members(FIRST, &[(3, 0), (2, 0), (1, 0), (0, 0)]);
    sim.set_group(FIRST, |g| g.trees = 10);
    sim.send(A, Order::split(FIRST));
    sim.steps(1);
    let groups = sim.groups();
    assert_eq!(groups.len(), 2);
    assert_eq!((groups[0].0.size(), groups[1].0.size()), (3, 2));
    assert_eq!((groups[0].0.trees, groups[1].0.trees), (6, 4));
    assert_eq!(tiles_of(&groups[1].0), vec![(0, 0), (1, 0)]);
}

#[test]
fn a_split_with_a_line_sends_the_half_pressed_along_it() {
    let f = field(30, 9);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(4, 4)]]));
    sim.add_members(FIRST, &[(3, 4), (2, 4), (1, 4), (0, 4)]);
    // From the Waldmensch at the back, down and away.
    let tiles: Vec<Tile> = (5..=8).map(|y| Tile::new(0, y)).collect();
    sim.send(
        A,
        Order::Split {
            group: FIRST,
            from: Some(Tile::new(0, 4)),
            tiles: tiles.clone(),
        },
    );
    sim.steps(1);
    let groups = sim.groups();
    assert_eq!(groups.len(), 2);
    let (new, _, line) = &groups[1];
    assert_eq!(tiles_of(new), vec![(0, 4), (1, 4)], "the rear half goes");
    assert_eq!(new.tile(), Tile::new(0, 4), "led by the one pressed");
    assert_eq!(line.0, tiles);
    assert_eq!(groups[0].0.tile(), Tile::new(4, 4), "the lead stays");
    sim.until_at(groups[1].0.id, (0, 8), 3000);
}

#[test]
fn a_split_with_a_line_from_the_lead_leaves_the_rest_behind() {
    let f = field(30, 9);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(4, 4)]]));
    sim.add_members(FIRST, &[(3, 4), (2, 4), (1, 4)]);
    sim.line(A, FIRST, &[(5, 4), (6, 4)]);
    sim.send(
        A,
        Order::Split {
            group: FIRST,
            from: Some(Tile::new(4, 4)),
            tiles: vec![Tile::new(4, 3), Tile::new(4, 2)],
        },
    );
    sim.steps(1);
    let groups = sim.groups();
    let (stay, _, stay_line) = &groups[0];
    let (new, _, new_line) = &groups[1];
    assert_eq!(tiles_of(new), vec![(3, 4), (4, 4)]);
    assert_eq!(tiles_of(stay), vec![(1, 4), (2, 4)]);
    assert_eq!(stay.tile(), Tile::new(2, 4), "the one nearest the old lead");
    assert!(stay_line.0.is_empty(), "the rest wait where they are");
    assert_eq!(new_line.0.len(), 2);
}

#[test]
fn the_half_sent_away_does_not_merge_back_on_its_first_cut() {
    let f = field(20, 12);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(4, 4)]]));
    sim.add_members(FIRST, &[(3, 4)]);
    // Cutting (3, 5) touches the other half's tile (4, 4).
    let tiles: Vec<Tile> = (5..=8).map(|y| Tile::new(3, y)).collect();
    sim.send(
        A,
        Order::Split {
            group: FIRST,
            from: Some(Tile::new(3, 4)),
            tiles,
        },
    );
    sim.steps(1);
    let new = sim.groups()[1].0.id;
    sim.until_at(new, (3, 8), 3000);
    assert_eq!(sim.groups().len(), 2, "still two groups");
    assert!(
        !sim.game()
            .events
            .iter()
            .any(|e| matches!(e, GameEvent::Merged { .. }))
    );
}

#[test]
fn a_group_hemmed_in_cannot_split() {
    // The board's edge and a lake leave less room than a split needs.
    let mut sim = Sim::new(setup(&["........~~", "........~~"], vec![vec![(4, 0)]]));
    sim.add_members(FIRST, &[(3, 0), (2, 0)]);
    sim.send(A, Order::split(FIRST));
    sim.steps(1);
    assert_eq!(sim.groups().len(), 1);
    assert!(
        !sim.game()
            .events
            .iter()
            .any(|e| matches!(e, GameEvent::Split { .. }))
    );
}

#[test]
fn going_to_another_clearing_merges_the_groups() {
    // A double click on the second group sends the first the fastest way there.
    let f = field(30, 5);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 2), (8, 2)]]));
    let from = sim.group(FIRST).tile();
    sim.send(
        A,
        Order::Goto {
            group: FIRST,
            from,
            to: Tile::new(8, 2),
        },
    );
    for _ in 0..3000 {
        sim.steps(1);
        if sim.groups().len() == 1 {
            break;
        }
    }
    assert_eq!(sim.groups().len(), 1, "the groups merged");
    assert_eq!(sim.groups()[0].0.size(), 2);
}

/// A 12 × 12 field where A has opened a ring round B's group at (5, 5):
/// rows 2 and 8 and columns 2 and 8, which zigzag down the rows, except at
/// (8, 4) and (8, 5). A's group waits outside at (10, 4).
fn ring_with_a_gap() -> Sim {
    let f = field(12, 12);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(10, 4)], vec![(5, 5)]]));
    let mut board = sim.board_mut();
    for i in 2..=8 {
        for tile in [(i, 2), (i, 8), (2, i), (8, i)] {
            if tile != (8, 4) && tile != (8, 5) {
                board.set_open(Tile::new(tile.0, tile.1), Some(A));
            }
        }
    }
    sim
}

#[test]
fn closing_a_ring_round_a_clearing_takes_the_whole_group() {
    let mut sim = ring_with_a_gap();
    sim.add_members(SECOND, &[(4, 5), (4, 4)]);
    sim.set_group(SECOND, |g| g.trees = 7);
    sim.steps(5);
    assert_eq!(sim.group(SECOND).rival, B, "the open ring encloses nobody");
    sim.line(A, FIRST, &[(9, 4), (8, 4)]);
    sim.until_at(FIRST, (8, 4), 400);
    assert_eq!(sim.group(SECOND).rival, B, "one tile is still open");
    sim.line(A, FIRST, &[(8, 5)]);
    sim.until_at(FIRST, (8, 5), 200);
    let groups = sim.groups();
    assert_eq!(groups.len(), 1, "B's group is gone");
    let won = &groups[0].0;
    assert_eq!(won.id, FIRST, "into the group that closed the ring");
    assert_eq!(won.size(), 4);
    assert!(won.trees >= 7, "with its trees");
    assert_eq!(
        sim.board().open(Tile::new(5, 5)),
        Some(A),
        "and its clearing"
    );
    let enclosed = sim.game().events.iter().find_map(|e| match e {
        GameEvent::Enclosed {
            by,
            from,
            into,
            size,
            part,
            ring,
            ..
        } => Some((*by, *from, *into, *size, part.len(), ring.len())),
        _ => None,
    });
    let (by, from, into, size, part, ring) = enclosed.expect("the enclosure is recorded");
    assert_eq!((by, from, into, size), (A, B, FIRST, 3));
    assert_eq!(part, 25);
    assert_eq!(ring, 22, "the ring round it is recorded");
    // B has no Waldleute left: the game is over.
    sim.steps(1);
    let outcome = sim.game().outcome.clone().expect("the game ended");
    assert_eq!(outcome.reason, EndReason::LastRival(A));
    assert_eq!(outcome.ranking[0].waldleute, 4);
}

#[test]
fn a_trapped_group_cannot_cut_its_way_out() {
    let mut sim = ring_with_a_gap();
    // B tries to leave through the gap, which lies in A's margin.
    sim.line(B, SECOND, &[(6, 5), (7, 5), (8, 5), (9, 5)]);
    sim.steps(1000);
    assert_eq!(sim.group(SECOND).tile(), Tile::new(6, 5));
}

#[test]
fn waldleute_in_a_river_stay_free_when_the_bank_is_enclosed() {
    let ground = [
        "......~.....",
        "..####~.....",
        "..#...~.....",
        "..#...~.....",
        "..#...~.....",
        "..#...~.....",
        "..#...~.....",
        "..##..~.....",
        "......~.....",
    ];
    let plain: Vec<String> = ground.iter().map(|r| r.replace('#', ".")).collect();
    let mut sim = Sim::new(setup(&rows(&plain), vec![vec![(4, 8)], vec![(5, 4)]]));
    {
        let mut board = sim.board_mut();
        for (y, row) in ground.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if c == '#' {
                    board.set_open(Tile::new(x as i32, y as i32), Some(A));
                }
            }
        }
    }
    // B's group wades: its lead in the river, one still on the bank.
    sim.set_group(SECOND, |g| {
        g.members.push(Member::at(Tile::new(6, 4), 0));
        g.lead = 1;
        g.way.push(Tile::new(6, 4));
    });
    sim.line(A, FIRST, &[(4, 7), (5, 7)]);
    sim.until_at(FIRST, (5, 7), 1000);
    let b = sim.group(SECOND);
    assert_eq!(b.rival, B);
    assert_eq!(
        tiles_of(&b),
        vec![(6, 4)],
        "the one in the river stays free"
    );
    assert_eq!(sim.group(FIRST).size(), 2, "the one on the bank is taken");
}

#[test]
fn a_ring_round_a_lake_takes_the_boat_too() {
    let rows_ = [
        "............",
        "............",
        "..AA..AAA...",
        "..A.....A...",
        "..A.111.A...",
        "..A.111.A...",
        "..A.111.A...",
        "..A.....A...",
        "..AAAAAAA...",
        "............",
    ];
    let ground: Vec<String> = rows_.iter().map(|r| r.replace('A', ".")).collect();
    let mut sim = Sim::new(setup(
        &rows(&ground),
        vec![vec![(4, 0)], vec![(3, 5), (7, 5)]],
    ));
    {
        let mut board = sim.board_mut();
        for (y, row) in rows_.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if c == 'A' {
                    board.set_open(Tile::new(x as i32, y as i32), Some(A));
                }
            }
        }
    }
    let tuning = sim.tuning();
    sim.set_group(FIRST, |g| g.trees = 20);
    sim.set_group(SECOND, |g| g.trees = tuning.ship_trees);
    sim.line(B, SECOND, &[(4, 5), (5, 5)]);
    sim.until_at(SECOND, (4, 5), 2000);
    assert_eq!(sim.group(SECOND).trees, 0, "the ship cost its trees");
    sim.line(A, FIRST, &[(4, 1), (4, 2), (5, 2)]);
    sim.until_at(FIRST, (5, 2), 400);
    let groups = sim.groups();
    assert!(
        groups.iter().all(|(g, ..)| g.rival == A),
        "the boat and the group on the shore are taken"
    );
    assert_eq!(sim.group(FIRST).size(), 3);
}

#[test]
fn wading_across_a_river_costs_time_and_trees() {
    let mut sim = Sim::new(setup(&["..~.."], vec![vec![(1, 0)]]));
    let tuning = sim.tuning();
    sim.line(A, FIRST, &[(2, 0), (3, 0)]);
    sim.steps(10);
    assert_eq!(
        sim.group(FIRST).tile(),
        Tile::new(1, 0),
        "no trees to wade with"
    );
    sim.set_group(FIRST, |g| g.trees = 5);
    sim.line(A, FIRST, &[(2, 0), (3, 0)]);
    let wade = sim.until_at(FIRST, (2, 0), 5000);
    assert_eq!(wade, u64::from(tuning.river_work.div_ceil(tuning.speed(1))));
    assert_eq!(sim.group(FIRST).trees, 5 - tuning.river_trees);
    sim.until_at(FIRST, (3, 0), 500);
}

#[test]
fn the_whole_group_boards_the_ship_and_follows_ashore() {
    let ground = ["...........", "....111....", "..........."];
    let mut sim = Sim::new(setup(&ground, vec![vec![(3, 1)]]));
    sim.add_members(FIRST, &[(2, 1), (1, 1)]);
    sim.set_group(FIRST, |g| g.trees = 100);
    sim.line(A, FIRST, &[(4, 1), (5, 1), (6, 1), (7, 1)]);
    sim.until_at(FIRST, (4, 1), 5000);
    assert_eq!(tiles_of(&sim.group(FIRST)), vec![(4, 1); 3], "all aboard");
    sim.until_at(FIRST, (7, 1), 5000);
    sim.steps(STEPS_PER_SECOND * 10);
    let group = sim.group(FIRST);
    assert!(
        group.tiles().all(|t| !sim.board().is_water(t)),
        "all ashore: {:?}",
        tiles_of(&group)
    );
}

#[test]
fn a_new_waldmensch_turns_up_somewhere_in_the_forest() {
    let f = field(30, 30);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)], vec![(29, 29)]]));
    let interval = sim.tuning().waldmensch_interval;
    sim.steps(interval);
    assert_eq!(sim.groups().len(), 2);
    sim.steps(1);
    let groups = sim.groups();
    assert_eq!(groups.len(), 4, "one more for each rival");
    for rival in [A, B] {
        let fresh: Vec<&Group> = groups
            .iter()
            .map(|(g, ..)| g)
            .filter(|g| g.rival == rival && g.id.0 > 2)
            .collect();
        assert_eq!(fresh.len(), 1);
        let tile = fresh[0].tile();
        assert_eq!(fresh[0].size(), 1, "a group of one");
        assert_eq!(sim.board().open(tile), Some(rival));
        for (other, ..) in &groups {
            if other.id != fresh[0].id {
                assert!(other.tile().distance(tile) > 1, "away from everyone");
            }
        }
    }
}

#[test]
fn the_game_ends_when_its_time_is_up() {
    let f = field(30, 3);
    let mut game = setup(&rows(&f), vec![vec![(0, 0)], vec![(20, 2), (25, 2)]]);
    game.minutes = 1;
    let mut sim = Sim::new(game);
    sim.add_members(FIRST, &[(1, 0)]);
    sim.set_group(GroupId(3), |g| g.trees = 4);
    sim.steps(60 * STEPS_PER_SECOND - 1);
    assert!(sim.game().outcome.is_none());
    sim.steps(1);
    let outcome = sim.game().outcome.clone().expect("time is up");
    assert_eq!(outcome.reason, EndReason::TimeUp);
    // Equal in Waldleute, B ranks first by its trees.
    let order: Vec<RivalId> = outcome.ranking.iter().map(|s| s.rival).collect();
    assert_eq!(order, vec![B, A]);
    let step = sim.game().step;
    sim.steps(10);
    assert_eq!(sim.game().step, step, "the rules stop with the game");
}

#[test]
fn rivals_start_scattered_at_random_over_a_real_board() {
    let start = |seed| {
        let mut sim = Sim::new(GameSetup {
            board: build_board(&BOARDS[0]).expect("board builds"),
            seats: vec![Seat::Player, Seat::Computer, Seat::Computer, Seat::Computer],
            minutes: 10,
            seed,
            placements: None,
        });
        sim.groups()
    };
    let tuning = Tuning::default();
    let groups = start(42);
    assert_eq!(groups.len(), 4 * tuning.start_waldleute as usize);
    let board = build_board(&BOARDS[0]).expect("board builds");
    for (a, ..) in &groups {
        assert!(!board.is_water(a.tile()));
        assert_eq!(a.size(), 1);
        for (b, ..) in &groups {
            if a.id != b.id {
                assert!(a.tile().distance(b.tile()) > 1, "nobody next to anybody");
            }
        }
    }
    let tiles = |g: &[(Group, Trail, Line)]| g.iter().map(|(g, ..)| g.tile()).collect::<Vec<_>>();
    assert_eq!(tiles(&start(42)), tiles(&groups), "the seed decides");
    assert_ne!(tiles(&start(43)), tiles(&groups));
}

fn real_game(seed: u64) -> GameSetup {
    GameSetup {
        board: build_board(&BOARDS[1]).expect("board builds"),
        seats: vec![Seat::Computer; 4],
        minutes: 6,
        seed,
        placements: None,
    }
}

#[test]
fn the_same_seed_and_commands_play_the_same_game() {
    let mut first = Sim::with_computers(real_game(7));
    let mut second = Sim::with_computers(real_game(7));
    for _ in 0..12 {
        first.steps(STEPS_PER_SECOND * 5);
        second.steps(STEPS_PER_SECOND * 5);
        assert_eq!(first.fingerprint(), second.fingerprint());
    }
}

#[test]
fn computer_rivals_expand_and_enclose() {
    let mut enclosures = 0;
    for seed in 1..=3 {
        let mut sim = Sim::with_computers(real_game(seed));
        sim.steps(STEPS_PER_SECOND * 60 * 6 + 1);
        let game = sim.game();
        assert!(game.outcome.is_some(), "seed {seed}: the game ends");
        enclosures += game
            .events
            .iter()
            .filter(|e| matches!(e, GameEvent::Enclosed { .. }))
            .count();
        // Trails grow back to the Waldleute, so count what they cut: trees.
        let felled: u32 = sim.groups().iter().map(|(g, ..)| g.trees).sum();
        assert!(
            felled > 200,
            "seed {seed}: rivals felled only {felled} trees"
        );
    }
    assert!(enclosures > 0, "computer rivals enclose each other");
}

/// A computer rival's group in the corner of a field, and A's group of four at
/// the west edge, nine tiles down.
fn corner(computer: bool) -> Sim {
    let f = field(30, 30);
    let mut game = setup(&rows(&f), vec![vec![(0, 9)], vec![(2, 2)]]);
    if computer {
        game.seats = vec![Seat::Player, Seat::Computer];
    }
    let mut sim = if computer {
        Sim::with_computers(game)
    } else {
        Sim::new(game)
    };
    sim.add_members(FIRST, &[(0, 10), (0, 11), (0, 12)]);
    sim
}

fn until_taken(sim: &mut Sim, limit: u64) -> bool {
    for _ in 0..limit {
        sim.steps(1);
        if sim.groups().iter().all(|(g, ..)| g.rival == A) {
            return true;
        }
    }
    false
}

#[test]
fn a_ring_from_edge_to_edge_takes_a_group_in_the_corner() {
    let mut sim = corner(true);
    let mut line: Vec<(i32, i32)> = (1..=7).map(|x| (x, 9)).collect();
    line.extend((0..=8).rev().map(|y| (7, y)));
    sim.line(A, FIRST, &line);
    assert!(until_taken(&mut sim, 3000), "even while it tries to escape");
}

#[test]
fn a_ring_drawn_in_two_lines_holds_through_a_pause() {
    let mut sim = corner(false);
    let line: Vec<(i32, i32)> = (1..=7).map(|x| (x, 9)).collect();
    sim.line(A, FIRST, &line);
    sim.until_at(FIRST, (7, 9), 3000);
    // A few seconds to draw the next line.
    sim.steps(STEPS_PER_SECOND * 5);
    let from = sim.group(FIRST).tile();
    let line: Vec<(i32, i32)> = (0..=8).rev().map(|y| (7, y)).collect();
    sim.line_from(A, FIRST, (from.x, from.y), &line);
    assert!(until_taken(&mut sim, 3000));
}

/// A 30 × 30 field with water down column 12 (a river, or a lake when
/// `lake`), a computer rival's group between it and the west edge, and A's
/// group of four on the west edge.
fn by_the_water(lake: bool) -> Sim {
    let water = if lake { '1' } else { '~' };
    let f: Vec<String> = (0..30)
        .map(|_| (0..30).map(|x| if x == 12 { water } else { '.' }).collect())
        .collect();
    let mut game = setup(&rows(&f), vec![vec![(0, 9)], vec![(5, 3)]]);
    game.seats = vec![Seat::Player, Seat::Computer];
    let mut sim = Sim::with_computers(game);
    sim.add_members(FIRST, &[(0, 10), (0, 11), (0, 12)]);
    sim
}

#[test]
fn a_ring_from_the_edge_to_the_water_holds_through_a_pause() {
    for lake in [false, true] {
        let mut sim = by_the_water(lake);
        let line: Vec<(i32, i32)> = (1..=6).map(|x| (x, 9)).collect();
        sim.line(A, FIRST, &line);
        sim.until_at(FIRST, (6, 9), 3000);
        sim.steps(STEPS_PER_SECOND * 5);
        let from = sim.group(FIRST).tile();
        let line: Vec<(i32, i32)> = (7..=11).map(|x| (x, from.y)).collect();
        sim.line_from(A, FIRST, (from.x, from.y), &line);
        assert!(until_taken(&mut sim, 3000), "lake: {lake}");
    }
}
