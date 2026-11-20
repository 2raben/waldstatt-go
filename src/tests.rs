//! Headless game tests: the real rule systems run at the fixed timestep, and
//! commands are written to [`Controls`] exactly as input and computer rivals
//! write them. No window or GPU is needed.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::ai::ComputerRivalsPlugin;
use crate::rules::game::{EndReason, GameEvent, STEPS_PER_SECOND};
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

    fn line(&mut self, rival: RivalId, group: GroupId, tiles: &[(i32, i32)]) {
        let tiles = tiles.iter().map(|&(x, y)| Tile::new(x, y)).collect();
        self.send(rival, Order::Line { group, tiles });
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
            if self.group(id).tile == Tile::new(tile.0, tile.1) {
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

#[test]
fn cuts_along_a_line_one_tile_after_another() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    let tuning = sim.tuning();
    let per_tile = u64::from(tuning.cut_work.div_ceil(tuning.speed(1)));
    sim.line(A, FIRST, &[(1, 0), (2, 0), (3, 0)]);
    sim.steps(per_tile - 1);
    assert_eq!(sim.group(FIRST).tile, Tile::new(0, 0));
    sim.steps(1);
    assert_eq!(sim.group(FIRST).tile, Tile::new(1, 0));
    assert_eq!(sim.board().open(Tile::new(1, 0)), Some(A));
    assert_eq!(sim.group(FIRST).trees, sim.board().trees(Tile::new(1, 0)));
    sim.steps(2 * per_tile);
    assert_eq!(sim.group(FIRST).tile, Tile::new(3, 0));
    assert!(sim.groups()[0].2.0.is_empty(), "the line is done");
}

#[test]
fn a_bigger_group_cuts_faster() {
    let f = field(30, 3);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0), (0, 2)]]));
    sim.set_group(SECOND, |g| g.size = 4);
    sim.line(A, FIRST, &[(1, 0)]);
    sim.line(A, SECOND, &[(1, 2)]);
    let big = sim.until_at(SECOND, (1, 2), 200);
    let small = big + sim.until_at(FIRST, (1, 0), 200);
    assert!(small > 2 * big, "one alone takes {small} steps, four {big}");
}

#[test]
fn only_the_last_tiles_of_a_trail_stay_open() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    let line: Vec<(i32, i32)> = (1..=10).map(|x| (x, 0)).collect();
    sim.line(A, FIRST, &line);
    sim.until_at(FIRST, (10, 0), 2000);
    // Eleven tiles cut, counting the first: the first has grown back.
    assert_eq!(sim.trail(FIRST).0.len(), 10);
    assert_eq!(sim.board().open(Tile::new(0, 0)), None);
    assert_eq!(sim.board().open(Tile::new(1, 0)), Some(A));
    assert_eq!(sim.trail(FIRST).closes_next(1), Some(Tile::new(1, 0)));
}

#[test]
fn a_line_stops_short_at_another_rivals_margin() {
    let f = field(12, 3);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)], vec![(6, 1)]]));
    sim.line(
        A,
        FIRST,
        &[(1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0), (7, 0)],
    );
    sim.steps(2000);
    let group = sim.group(FIRST);
    // (5, 0) touches B's tile (6, 1) diagonally.
    assert_eq!(group.tile, Tile::new(4, 0));
    assert!(sim.groups()[0].2.0.is_empty());
    assert_eq!(sim.board().open(Tile::new(5, 0)), None);
}

#[test]
fn a_group_sent_onto_another_merges_with_it() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0), (3, 0)]]));
    sim.line(A, FIRST, &[(1, 0), (2, 0), (3, 0)]);
    sim.steps(1000);
    let groups = sim.groups();
    assert_eq!(groups.len(), 1);
    let (group, trail, _) = &groups[0];
    assert_eq!(
        group.id, SECOND,
        "the group sent merges into the one it reached"
    );
    assert_eq!(group.size, 2);
    // Both first tiles, and the two cut on the way.
    assert_eq!(trail.0.len(), 4);
    assert!(sim.game().events.iter().any(
        |e| matches!(e, GameEvent::Merged { from, into, .. } if *from == FIRST && *into == SECOND)
    ));
}

#[test]
fn splitting_shares_the_group() {
    let f = field(30, 1);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0)]]));
    sim.set_group(FIRST, |g| {
        g.size = 5;
        g.trees = 10;
    });
    sim.send(A, Order::Split { group: FIRST });
    sim.steps(1);
    let groups = sim.groups();
    assert_eq!(groups.len(), 2);
    assert_eq!((groups[0].0.size, groups[1].0.size), (3, 2));
    assert_eq!((groups[0].0.trees, groups[1].0.trees), (6, 4));
    assert_eq!(groups[0].0.tile, groups[1].0.tile);
}

/// A 12 × 12 field where A has opened a ring round B's group at (5, 5),
/// except at (8, 4) and (8, 5). A's group waits outside at (10, 4): a group
/// standing right beside a one-tile gap would already close the ring
/// diagonally.
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
fn closing_a_ring_wins_the_group_inside() {
    let mut sim = ring_with_a_gap();
    sim.set_group(SECOND, |g| g.trees = 7);
    sim.steps(5);
    assert_eq!(sim.group(SECOND).rival, B, "the open ring encloses nobody");
    sim.line(A, FIRST, &[(9, 4), (8, 4)]);
    sim.until_at(FIRST, (8, 4), 400);
    assert_eq!(sim.group(SECOND).rival, B, "one tile is still open");
    sim.line(A, FIRST, &[(8, 5)]);
    sim.until_at(FIRST, (8, 5), 200);
    let won = sim.group(SECOND);
    assert_eq!(won.rival, A, "the group changes sides");
    assert_eq!(won.trees, 7, "with its trees");
    assert_eq!(sim.board().open(Tile::new(5, 5)), Some(A), "and its trail");
    let enclosed = sim.game().events.iter().find_map(|e| match e {
        GameEvent::Enclosed {
            by,
            from,
            part,
            ring,
            ..
        } => Some((*by, *from, part.len(), ring.len())),
        _ => None,
    });
    let (by, from, part, ring) = enclosed.expect("the enclosure is recorded");
    assert_eq!((by, from), (A, B));
    assert_eq!(part, 25);
    assert!(ring >= 24, "the ring round it is recorded: {ring}");
    // B has no Waldleute left: the game is over.
    sim.steps(1);
    let outcome = sim.game().outcome.clone().expect("the game ended");
    assert_eq!(outcome.reason, EndReason::LastRival(A));
    assert_eq!(outcome.ranking[0].waldleute, 2);
}

#[test]
fn a_trapped_group_cannot_cut_its_way_out() {
    let mut sim = ring_with_a_gap();
    // B tries to leave through the gap, which lies in A's margin.
    sim.line(B, SECOND, &[(6, 5), (7, 5), (8, 5), (9, 5)]);
    sim.steps(1000);
    assert_eq!(sim.group(SECOND).tile, Tile::new(6, 5));
}

#[test]
fn a_group_on_water_is_safe_but_one_building_on_the_shore_is_not() {
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
    sim.set_group(GroupId(3), |g| g.trees = tuning.ship_trees);
    // The second group takes to the lake; the third keeps building on the shore.
    sim.line(B, SECOND, &[(4, 5), (5, 5)]);
    sim.until_at(SECOND, (4, 5), 2000);
    assert_eq!(sim.group(SECOND).trees, 0, "the ship cost its trees");
    sim.line(B, GroupId(3), &[(6, 5), (5, 5)]);
    sim.steps(10);
    sim.line(A, FIRST, &[(4, 1), (4, 2), (5, 2)]);
    sim.until_at(FIRST, (5, 2), 400);
    assert_eq!(sim.group(SECOND).rival, B, "afloat: safe");
    assert_eq!(sim.group(GroupId(3)).rival, A, "on the shore: enclosed");
}

#[test]
fn wading_across_a_river_costs_time_and_trees() {
    let mut sim = Sim::new(setup(&["..~.."], vec![vec![(1, 0)]]));
    let tuning = sim.tuning();
    sim.line(A, FIRST, &[(2, 0), (3, 0)]);
    sim.steps(10);
    assert_eq!(
        sim.group(FIRST).tile,
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
fn a_ship_carries_at_most_its_capacity() {
    let mut sim = Sim::new(setup(&[".111."], vec![vec![(0, 0)]]));
    let capacity = sim.tuning().ship_capacity;
    sim.set_group(FIRST, |g| {
        g.size = capacity + 1;
        g.trees = 100;
    });
    sim.line(A, FIRST, &[(1, 0), (2, 0), (3, 0), (4, 0)]);
    sim.steps(1);
    assert!(sim.groups()[0].2.0.is_empty(), "too many for a ship");
    sim.set_group(FIRST, |g| g.size = capacity);
    sim.line(A, FIRST, &[(1, 0), (2, 0), (3, 0), (4, 0)]);
    sim.until_at(FIRST, (4, 0), 5000);
}

#[test]
fn new_waldleute_join_the_chosen_group() {
    let f = field(30, 3);
    let mut sim = Sim::new(setup(&rows(&f), vec![vec![(0, 0), (0, 2)]]));
    let interval = sim.tuning().waldmensch_interval;
    sim.send(A, Order::Receiver { group: SECOND });
    sim.steps(interval);
    assert_eq!(sim.group(SECOND).size, 1);
    sim.steps(1);
    assert_eq!(sim.group(SECOND).size, 2);
    assert_eq!(sim.group(FIRST).size, 1);
}

#[test]
fn the_game_ends_when_its_time_is_up() {
    let f = field(30, 3);
    let mut game = setup(&rows(&f), vec![vec![(0, 0)], vec![(20, 2), (25, 2)]]);
    game.minutes = 1;
    let mut sim = Sim::new(game);
    sim.set_group(FIRST, |g| g.size = 2);
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
fn rivals_start_spread_out_on_a_real_board() {
    let board = build_board(&BOARDS[0]).expect("board builds");
    let tuning = Tuning::default();
    let sim = Sim::new(GameSetup {
        board,
        seats: vec![Seat::Player, Seat::Computer, Seat::Computer, Seat::Computer],
        minutes: 10,
        seed: 42,
        placements: None,
    });
    let mut sim = sim;
    let groups = sim.groups();
    assert_eq!(groups.len(), 4 * tuning.start_waldleute as usize);
    for (a, ..) in &groups {
        assert!(!sim.board().is_water(a.tile));
        for (b, ..) in &groups {
            if a.rival != b.rival {
                assert!(a.tile.chebyshev(b.tile) > tuning.start_margin);
            }
        }
    }
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
        let open = sim
            .board()
            .tiles()
            .filter(|&t| sim.board().open(t).is_some())
            .count();
        assert!(open > 80, "seed {seed}: rivals hold {open} tiles open");
    }
    assert!(enclosures > 0, "computer rivals enclose each other");
}
