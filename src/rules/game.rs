//! A game: its rivals, the fixed rival order derived from the seed, new
//! Waldleute at a fixed interval, the clock, the end and the ranking.
//!
//! A game is its board, its seed and the commands of each step; given the
//! same three it ends the same way.

use std::collections::BTreeSet;

use bevy::prelude::*;

use super::board::{Board, Ground, MAX_RIVALS, RivalId, Tile};
use super::commands::Controls;
use super::enclosure::Enclosures;
use super::group::{Group, GroupId, Line, Trail};
use super::rng::{Rng, mix};
use super::sight::Sight;
use super::tuning::Tuning;

/// The fixed timestep's rate: steps per second.
pub const STEPS_PER_SECOND: u64 = 64;

/// The rivals' names, by seat: the colours they are drawn in, which stand out
/// from the forest's green and the water's blue.
pub const RIVAL_NAMES: [&str; MAX_RIVALS] = ["Gold", "Rot", "Violett", "Weiss"];

/// Who plays a seat. The rules treat every seat alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    Player,
    Computer,
}

#[derive(Component, Clone, Debug)]
pub struct Rival {
    pub id: RivalId,
    pub seat: Seat,
    /// Where its first Waldmensch was placed.
    pub home: Tile,
    /// Whether it has lost all its Waldleute.
    pub out: bool,
}

impl Rival {
    pub fn name(&self) -> &'static str {
        RIVAL_NAMES[self.id.index()]
    }
}

/// Everything a game starts from.
#[derive(Clone, Debug)]
pub struct GameSetup {
    pub board: Board,
    /// Two to four seats; seat 0 is usually the player.
    pub seats: Vec<Seat>,
    pub minutes: u32,
    pub seed: u64,
    /// Where each rival's first groups stand; placed from the seed if `None`.
    pub placements: Option<Vec<Vec<Tile>>>,
}

/// Something that happened, for showing it and reading it afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameEvent {
    /// A ring closed: the group changed sides from `from` to `by`.
    Enclosed {
        step: u64,
        by: RivalId,
        from: RivalId,
        /// The group the Waldleute were taken from.
        group: GroupId,
        /// The encloser's group they joined.
        into: GroupId,
        /// Waldleute taken.
        size: u32,
        /// The enclosed part.
        part: Vec<Tile>,
        /// The rival's open tiles round it.
        ring: Vec<Tile>,
    },
    Merged {
        step: u64,
        rival: RivalId,
        from: GroupId,
        into: GroupId,
    },
    Split {
        step: u64,
        rival: RivalId,
        from: GroupId,
        new: GroupId,
    },
    /// A new Waldmensch joined a group.
    Joined {
        step: u64,
        rival: RivalId,
        group: GroupId,
    },
    /// A rival lost its last Waldleute.
    Out { step: u64, rival: RivalId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    TimeUp,
    LastRival(RivalId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    pub rival: RivalId,
    pub waldleute: u32,
    pub trees: u32,
    pub groups: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub reason: EndReason,
    pub ranking: Vec<Standing>,
}

#[derive(Resource, Clone, Debug)]
pub struct Game {
    pub seed: u64,
    /// Steps played.
    pub step: u64,
    /// Steps the game lasts.
    pub length: u64,
    /// The fixed order in which rivals' commands, moves and rings resolve.
    pub order: Vec<RivalId>,
    /// The step the next Waldleute come.
    pub next_waldmensch: u64,
    pub events: Vec<GameEvent>,
    pub outcome: Option<Outcome>,
    rivals: usize,
    next_group: u32,
}

impl Game {
    /// A rival's place in the order; rivals not in the game come last.
    pub fn rank(&self, rival: RivalId) -> usize {
        self.order
            .iter()
            .position(|&r| r == rival)
            .unwrap_or(MAX_RIVALS)
    }

    pub fn next_group_id(&mut self) -> GroupId {
        self.next_group += 1;
        GroupId(self.next_group)
    }

    pub fn rivals(&self) -> usize {
        self.rivals
    }

    pub fn seconds_left(&self) -> u64 {
        self.length
            .saturating_sub(self.step)
            .div_ceil(STEPS_PER_SECOND)
    }
}

/// Run condition: a game is on and not over.
pub fn game_running(game: Option<Res<Game>>) -> bool {
    game.is_some_and(|game| game.outcome.is_none())
}

/// Starts a game, replacing any running one.
pub fn start_game(world: &mut World, setup: GameSetup) {
    let old: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<Rival>, With<Group>)>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    world.init_resource::<Tuning>();
    let tuning = world.resource::<Tuning>().clone();
    world.insert_resource(Controls::default());

    let GameSetup {
        mut board,
        seats,
        minutes,
        seed,
        placements,
    } = setup;
    let seats = &seats[..seats.len().min(MAX_RIVALS)];
    board.grow_forest(seed);
    let mut rng = Rng::new(seed);
    let mut order: Vec<RivalId> = (0..seats.len()).map(|i| RivalId(i as u8)).collect();
    rng.shuffle(&mut order);
    let placements =
        placements.unwrap_or_else(|| place_rivals(&board, &tuning, seats.len(), &mut rng));
    for tile in placements.iter().flatten() {
        board.set_open(*tile, None);
    }

    let mut game = Game {
        seed,
        step: 0,
        length: u64::from(minutes) * 60 * STEPS_PER_SECOND,
        order,
        next_waldmensch: tuning.waldmensch_interval,
        events: Vec::new(),
        outcome: None,
        rivals: seats.len(),
        next_group: 0,
    };
    for (i, &seat) in seats.iter().enumerate() {
        let id = RivalId(i as u8);
        let tiles = placements.get(i).cloned().unwrap_or_default();
        for &tile in &tiles {
            board.set_open(tile, Some(id));
            world.spawn((
                Group::new(game.next_group_id(), id, tile),
                Trail(std::iter::once((tile, 0)).collect()),
                Line::default(),
            ));
        }
        let mut sight = Sight::new(&board);
        sight.look(&board, id, tiles.iter().copied(), tuning.view_radius);
        world.spawn((
            Rival {
                id,
                seat,
                home: tiles.first().copied().unwrap_or_default(),
                out: tiles.is_empty(),
            },
            sight,
            Enclosures::default(),
        ));
    }
    world.insert_resource(board);
    world.insert_resource(game);
}

/// Places each rival's first Waldleute at random over the board, one rival
/// after the other, each a group of one.
fn place_rivals(board: &Board, tuning: &Tuning, rivals: usize, rng: &mut Rng) -> Vec<Vec<Tile>> {
    let mut board = board.clone();
    let mut placed = vec![Vec::new(); rivals];
    for _ in 0..tuning.start_waldleute {
        for (i, own) in placed.iter_mut().enumerate() {
            if let Some(tile) = random_tile(&board, rng, |_| true) {
                board.set_open(tile, Some(RivalId(i as u8)));
                own.push(tile);
            }
        }
    }
    placed
}

/// A random forest tile with no open tile and no Waldmensch round it, where
/// `also` holds; `None` if a few thousand tries find none.
fn random_tile(board: &Board, rng: &mut Rng, also: impl Fn(Tile) -> bool) -> Option<Tile> {
    for _ in 0..4000 {
        let tile = Tile::new(
            rng.below(board.width as u32) as i32,
            rng.below(board.height as u32) as i32,
        );
        let clear = std::iter::once(tile)
            .chain(tile.neighbours())
            .all(|t| board.open(t).is_none());
        if board.ground(tile) == Ground::Land && clear && also(tile) {
            return Some(tile);
        }
    }
    None
}

/// Every interval, each rival still in the game gets one more Waldmensch: a
/// group of one on a random forest tile of the board, away from every
/// clearing and Waldmensch and outside every other rival's rings.
pub fn grant_waldleute(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut board: ResMut<Board>,
    tuning: Res<Tuning>,
    rivals: Query<(&Rival, &Enclosures)>,
    groups: Query<&Group>,
) {
    if game.step < game.next_waldmensch {
        return;
    }
    game.next_waldmensch += tuning.waldmensch_interval;
    let mut rng = Rng::new(game.seed ^ mix(game.step));
    let standing: BTreeSet<Tile> = groups
        .iter()
        .flat_map(|g| g.tiles().collect::<Vec<_>>())
        .collect();
    for rival in game.order.clone() {
        if !rivals.iter().any(|(r, _)| r.id == rival && !r.out) {
            continue;
        }
        let free = |tile: Tile| {
            !std::iter::once(tile)
                .chain(tile.neighbours())
                .any(|t| standing.contains(&t))
                && rivals.iter().all(|(other, rings)| {
                    other.id == rival || rings.0.as_ref().is_none_or(|p| !p.encloses(&board, tile))
                })
        };
        let Some(tile) = random_tile(&board, &mut rng, free) else {
            continue;
        };
        board.set_open(tile, Some(rival));
        let id = game.next_group_id();
        let step = game.step;
        let mut group = Group::new(id, rival, tile);
        group.members[0].entered = step;
        commands.spawn((group, Trail([(tile, step)].into()), Line::default()));
        game.events.push(GameEvent::Joined {
            step,
            rival,
            group: id,
        });
    }
}

/// The rivals ordered by Waldleute, then trees, then the game's order.
pub fn standings<'a>(
    game: &Game,
    rivals: impl Iterator<Item = RivalId>,
    groups: impl Iterator<Item = &'a Group> + Clone,
) -> Vec<Standing> {
    let mut ranking: Vec<Standing> = rivals
        .map(|rival| {
            let own = groups.clone().filter(|g| g.rival == rival);
            Standing {
                rival,
                waldleute: own.clone().map(|g| g.size()).sum(),
                trees: own.clone().map(|g| g.trees).sum(),
                groups: own.count(),
            }
        })
        .collect();
    ranking.sort_by_key(|s| {
        (
            std::cmp::Reverse(s.waldleute),
            std::cmp::Reverse(s.trees),
            game.rank(s.rival),
        )
    });
    ranking
}

/// Counts the step, marks rivals without Waldleute as out, and ends the game
/// when its time is up or only one rival is left.
pub fn finish_step(mut game: ResMut<Game>, mut rivals: Query<&mut Rival>, groups: Query<&Group>) {
    for mut rival in &mut rivals {
        if !rival.out && !groups.iter().any(|g| g.rival == rival.id) {
            rival.out = true;
            let event = GameEvent::Out {
                step: game.step,
                rival: rival.id,
            };
            game.events.push(event);
        }
    }
    game.step += 1;
    let left: Vec<RivalId> = rivals.iter().filter(|r| !r.out).map(|r| r.id).collect();
    let reason = if game.rivals() > 1 && left.len() <= 1 {
        Some(EndReason::LastRival(
            left.first().copied().unwrap_or(RivalId(0)),
        ))
    } else if game.step >= game.length {
        Some(EndReason::TimeUp)
    } else {
        None
    };
    if let Some(reason) = reason {
        let ranking = standings(&game, rivals.iter().map(|r| r.id), groups.iter());
        game.outcome = Some(Outcome { reason, ranking });
    }
}
