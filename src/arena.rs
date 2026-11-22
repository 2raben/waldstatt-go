//! The arena: computer rivals play whole games against each other, headless
//! and as fast as the machine allows, to measure how a [`Temperament`] plays.
//!
//! `waldstatt-go arena [options] [name=value ...]` plays games from
//! consecutive seeds round the boards. In each game one seat, a different one
//! each game, is the challenger: the default temperament with the given
//! values changed. The other seats play the default. The report says how the
//! challenger places. Without changes the challenger is the default, and how
//! far its mean place lies from even shows the arena's noise.
//!
//! Every game is the rules and the computer rivals only, stepped like the
//! headless tests, so a game from a seed plays the same on every run.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::ai::{ComputerRivalsPlugin, Temperament, Temperaments};
use crate::rules::game::{EndReason, GameEvent, STEPS_PER_SECOND};
use crate::rules::waters::{BOARDS, build_board};
use crate::rules::{Board, Game, GameSetup, RivalId, RulesPlugin, Seat, start_game};

const USAGE: &str = "\
Usage: waldstatt-go arena [options] [name=value ...]

Plays computer rivals against each other, headless. One seat in each game,
a different one each game, plays the default temperament changed by the
name=value pairs; the others play the default.

Options:
  --games N      games to play (default 40)
  --rivals N     rivals in each game, 2 to 4 (default 4)
  --minutes N    length of each game (default 10)
  --board NAME   play only this board (default: all, in turn)
  --seed N       the first game's seed (default 1)
  --threads N    games played at once (default: all cores)
  --help         this text and the temperament's values

rally_size=3 starts the hunt with three Waldleute while the others join.
rally_size=0 uses the distributed opening (threat_range, company_range,
split_size and up to hunters groups on a ring).";

/// What the arena plays.
#[derive(Clone, Debug)]
struct Plan {
    games: usize,
    rivals: usize,
    minutes: u32,
    boards: Vec<usize>,
    seed: u64,
    threads: usize,
    challenger: Temperament,
}

/// One game's result, from the challenger's side.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Result {
    board: usize,
    seed: u64,
    challenger: RivalId,
    /// 1 for first.
    place: usize,
    /// Waldleute at the end, by rival.
    waldleute: Vec<u32>,
    /// Group captures. A ring can enclose Waldleute from several groups.
    enclosures: usize,
    /// Waldleute the challenger took, and lost, by enclosing.
    taken: u32,
    lost: u32,
    /// Rivals that lost all their Waldleute.
    out: usize,
    /// Whether one rival was left before time was up.
    early: bool,
}

/// Runs the arena from the arguments after `arena`.
pub fn run(args: impl Iterator<Item = String>) -> AppExit {
    let plan = match parse(args) {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            println!("{USAGE}\n\nThe default temperament:");
            for (name, value) in Temperament::default().values() {
                println!("  {name}={value}");
            }
            return AppExit::Success;
        }
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return AppExit::error();
        }
    };
    let boards: Vec<Board> = BOARDS
        .iter()
        .map(|spec| build_board(spec).expect("the compiled boards build"))
        .collect();
    let start = Instant::now();
    let results = play_all(&plan, &boards);
    report(&plan, &results, start.elapsed());
    AppExit::Success
}

fn parse(mut args: impl Iterator<Item = String>) -> std::result::Result<Option<Plan>, String> {
    let mut plan = Plan {
        games: 40,
        rivals: 4,
        minutes: 10,
        boards: (0..BOARDS.len()).collect(),
        seed: 1,
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        challenger: Temperament::default(),
    };
    fn number<T: std::str::FromStr>(
        flag: &str,
        value: Option<String>,
    ) -> std::result::Result<T, String> {
        let value = value.ok_or_else(|| format!("{flag} needs a value"))?;
        value
            .parse()
            .map_err(|_| format!("{flag}: {value} is not a number"))
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--games" => plan.games = number(&arg, args.next())?,
            "--rivals" => plan.rivals = number(&arg, args.next())?,
            "--minutes" => plan.minutes = number(&arg, args.next())?,
            "--seed" => plan.seed = number(&arg, args.next())?,
            "--threads" => plan.threads = number(&arg, args.next())?,
            "--board" => {
                let name = args.next().ok_or("--board needs a name")?;
                let index = BOARDS
                    .iter()
                    .position(|b| b.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| {
                        let names: Vec<_> = BOARDS.iter().map(|b| b.name).collect();
                        format!(
                            "no board called {name}; the boards are {}",
                            names.join(", ")
                        )
                    })?;
                plan.boards = vec![index];
            }
            _ => {
                let (name, value) = arg
                    .split_once('=')
                    .ok_or_else(|| format!("unknown argument {arg}"))?;
                let value = number(name, Some(value.to_owned()))?;
                plan.challenger.set(name, value)?;
            }
        }
    }
    if !(2..=4).contains(&plan.rivals) {
        return Err("--rivals must be 2, 3 or 4".into());
    }
    if plan.games == 0 || plan.minutes == 0 || plan.threads == 0 {
        return Err("--games, --minutes and --threads must be at least 1".into());
    }
    Ok(Some(plan))
}

/// Plays every game of the plan, several at once, in the order of their seeds.
fn play_all(plan: &Plan, boards: &[Board]) -> Vec<Result> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(plan.games));
    std::thread::scope(|scope| {
        for _ in 0..plan.threads.min(plan.games) {
            scope.spawn(|| {
                loop {
                    let game = next.fetch_add(1, Ordering::Relaxed);
                    if game >= plan.games {
                        break;
                    }
                    let result = play(plan, boards, game);
                    let mut results = results.lock().expect("no thread panics holding it");
                    results.push(result);
                    eprint!("\rplayed {}/{}", results.len(), plan.games);
                }
            });
        }
    });
    eprintln!();
    let mut results = results.into_inner().expect("no thread panicked holding it");
    results.sort_by_key(|r| r.seed);
    results
}

/// Plays the plan's game number `game` to its end.
fn play(plan: &Plan, boards: &[Board], game: usize) -> Result {
    let board = plan.boards[game % plan.boards.len()];
    let seed = plan.seed + game as u64;
    let challenger = RivalId((game % plan.rivals) as u8);
    let setup = GameSetup {
        board: boards[board].clone(),
        seats: vec![Seat::Computer; plan.rivals],
        minutes: plan.minutes,
        seed,
        placements: None,
    };
    let temperaments = Temperaments {
        by_rival: BTreeMap::from([(challenger, plan.challenger.clone())]),
        default: Temperament::default(),
    };
    let game = play_game(setup, temperaments);
    let outcome = game
        .outcome
        .as_ref()
        .expect("the arena plays every game to its end");
    let mut waldleute = vec![0; plan.rivals];
    for standing in &outcome.ranking {
        waldleute[standing.rival.index()] = standing.waldleute;
    }
    let (mut enclosures, mut taken, mut lost, mut out) = (0, 0, 0, 0);
    for event in &game.events {
        match event {
            GameEvent::Enclosed { by, from, size, .. } => {
                enclosures += 1;
                if *by == challenger {
                    taken += size;
                }
                if *from == challenger {
                    lost += size;
                }
            }
            GameEvent::Out { .. } => out += 1,
            _ => {}
        }
    }
    Result {
        board,
        seed,
        challenger,
        place: outcome
            .ranking
            .iter()
            .position(|s| s.rival == challenger)
            .map_or(plan.rivals, |i| i + 1),
        waldleute,
        enclosures,
        taken,
        lost,
        out,
        early: matches!(outcome.reason, EndReason::LastRival(_)),
    }
}

/// Plays a game of computer rivals headless, one fixed step per update, and
/// returns it ended.
fn play_game(setup: GameSetup, temperaments: Temperaments) -> Game {
    let length = u64::from(setup.minutes) * 60 * STEPS_PER_SECOND;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, RulesPlugin, ComputerRivalsPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / STEPS_PER_SECOND as f64,
        )))
        .insert_resource(temperaments);
    app.finish();
    app.cleanup();
    // The first update starts the clock; later ones advance one fixed step each.
    app.update();
    start_game(app.world_mut(), setup);
    for _ in 0..=length {
        if app.world().resource::<Game>().outcome.is_some() {
            break;
        }
        app.update();
    }
    app.world_mut()
        .remove_resource::<Game>()
        .expect("a game was started")
}

fn report(plan: &Plan, results: &[Result], took: Duration) {
    let n = results.len() as f64;
    let rivals = plan.rivals as f64;
    let boards: Vec<_> = plan.boards.iter().map(|&b| BOARDS[b].name).collect();
    println!(
        "{} games of {} minutes, {} rivals, on {}, seeds {} to {}",
        plan.games,
        plan.minutes,
        plan.rivals,
        boards.join(", "),
        plan.seed,
        plan.seed + plan.games as u64 - 1,
    );
    let changes: Vec<String> = plan
        .challenger
        .values()
        .into_iter()
        .zip(Temperament::default().values())
        .filter(|(mine, default)| mine != default)
        .map(|((name, value), (_, default))| format!("{name}={value} (default {default})"))
        .collect();
    if changes.is_empty() {
        println!("Challenger: the default temperament, against itself");
    } else {
        println!("Challenger: {}", changes.join(", "));
    }
    println!();

    let places: Vec<f64> = results.iter().map(|r| r.place as f64).collect();
    let (mean, error) = mean_and_error(&places);
    let even = (rivals + 1.0) / 2.0;
    let wins = results.iter().filter(|r| r.place == 1).count() as f64 / n;
    println!("Mean place     {mean:.2} ± {error:.2}   (even {even:.2}; lower is better)");
    println!(
        "Wins           {:.0} %   (even {:.0} %)",
        wins * 100.0,
        100.0 / rivals
    );
    let own: f64 = results
        .iter()
        .map(|r| f64::from(r.waldleute[r.challenger.index()]))
        .sum::<f64>()
        / n;
    let others: f64 = results
        .iter()
        .map(|r| {
            let all: u32 = r.waldleute.iter().sum();
            f64::from(all - r.waldleute[r.challenger.index()])
        })
        .sum::<f64>()
        / n
        / (rivals - 1.0);
    println!("Waldleute      {own:.1} at the end, against {others:.1} for each other rival");
    let taken = results.iter().map(|r| f64::from(r.taken)).sum::<f64>() / n;
    let lost = results.iter().map(|r| f64::from(r.lost)).sum::<f64>() / n;
    println!("Enclosing      took {taken:.1} Waldleute a game, lost {lost:.1}");
    if plan.boards.len() > 1 {
        let by_board: Vec<String> = plan
            .boards
            .iter()
            .map(|&b| {
                let places: Vec<f64> = results
                    .iter()
                    .filter(|r| r.board == b)
                    .map(|r| r.place as f64)
                    .collect();
                let (mean, _) = mean_and_error(&places);
                format!("{} {mean:.2} ({})", BOARDS[b].name, places.len())
            })
            .collect();
        println!("By board       {}", by_board.join(", "));
    }
    println!();

    let enclosures = results.iter().map(|r| r.enclosures as f64).sum::<f64>() / n;
    let out = results.iter().map(|r| r.out as f64).sum::<f64>() / n;
    let early = results.iter().filter(|r| r.early).count();
    let all = results
        .iter()
        .map(|r| f64::from(r.waldleute.iter().sum::<u32>()))
        .sum::<f64>()
        / n
        / rivals;
    println!(
        "Each game      {enclosures:.1} group captures, {out:.1} rivals out, {all:.1} Waldleute a rival at the end; {early} of {} ended early",
        results.len()
    );
    println!(
        "Played in {:.0} s, {:.1} s a game on {} threads",
        took.as_secs_f64(),
        took.as_secs_f64() / n,
        plan.threads.min(plan.games),
    );
}

/// The mean and its standard error.
fn mean_and_error(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    if n == 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let mean = values.iter().sum::<f64>() / n;
    if n < 2.0 {
        return (mean, f64::NAN);
    }
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, (variance / n).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(args: &[&str]) -> Plan {
        parse(args.iter().map(|a| a.to_string()))
            .expect("the arguments parse")
            .expect("not asking for help")
    }

    #[test]
    fn arguments_change_the_challenger_only_where_given() {
        let plan = plan(&["--games", "8", "--board", "hütten", "hunt_range=24"]);
        assert_eq!(plan.games, 8);
        assert_eq!(plan.boards, vec![2]);
        assert_eq!(plan.challenger.hunt_range, 24);
        assert_eq!(
            Temperament {
                hunt_range: Temperament::default().hunt_range,
                ..plan.challenger
            },
            Temperament::default()
        );
        assert!(parse(["no_such_value=3".to_string()].into_iter()).is_err());
        assert!(parse(["--rivals".to_string(), "5".to_string()].into_iter()).is_err());
    }

    #[test]
    fn a_game_in_the_arena_plays_the_same_every_time() {
        let mut plan = plan(&["--minutes", "1", "--rivals", "3", "rally_size=0"]);
        plan.boards = vec![0];
        let boards = vec![build_board(&BOARDS[0]).expect("board builds")];
        let first = play(&plan, &boards, 1);
        assert_eq!(first, play(&plan, &boards, 1));
        assert_eq!(first.challenger, RivalId(1));
        assert!((1..=3).contains(&first.place));
    }
}
