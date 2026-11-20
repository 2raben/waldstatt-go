//! The places outside the board: the start menu, setting up a game (board,
//! two to four rivals, game length), the pause menu and the end with the
//! ranking. Every seat but the player's is a computer rival.
//!
//! Pausing and menus are app state, not commands: the rules stop because
//! virtual time stops, so nothing in the game changes.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button};

use crate::ground::{BoardScene, spawn_scene};
use crate::rules::game::{EndReason, RIVAL_NAMES, Standing, standings};
use crate::rules::rng::mix;
use crate::rules::waters::{BOARDS, build_board};
use crate::rules::{Game, GameSetup, Group, Rival, Seat, start_game};
use crate::style::{Fonts, clock, heading, palette, rival_colour, text};
use crate::view::BoardView;

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    Start,
    Setup,
    Game,
    End,
}

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pause {
    #[default]
    Off,
    On,
}

/// Run condition: the board takes input.
pub fn playing(screen: Res<State<Screen>>, pause: Res<State<Pause>>) -> bool {
    *screen.get() == Screen::Game && *pause.get() == Pause::Off
}

/// The game being set up.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Choice {
    pub board: usize,
    /// Seats at the board, the player's included.
    pub seats: usize,
    pub minutes: u32,
    games: u64,
}

impl Default for Choice {
    fn default() -> Self {
        Self {
            board: 0,
            seats: 4,
            minutes: 10,
            games: 0,
        }
    }
}

/// How the last game ended, for the end screen.
#[derive(Resource, Default, Clone, Debug)]
struct Ending {
    title: String,
    ranking: Vec<Standing>,
    player: Option<crate::rules::RivalId>,
}

/// What a button does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    NewGame,
    #[cfg(not(target_arch = "wasm32"))]
    Quit,
    Board(usize),
    Seats(usize),
    Minutes(u32),
    Begin,
    Back,
    Resume,
    ToggleView,
    GiveUp,
    MainMenu,
}

pub const CREDITS: &str = "Gewässer aus der Waldstatt: swissTLM3D © swisstopo, \
                           Flussordnungszahlen nach Strahler © BAFU";

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Screen>()
            .init_state::<Pause>()
            .init_resource::<Choice>()
            .init_resource::<Ending>()
            .add_observer(on_activate)
            .add_systems(OnEnter(Screen::Start), (spawn_start, stop_time))
            .add_systems(OnEnter(Screen::Setup), spawn_setup)
            .add_systems(OnEnter(Screen::End), (spawn_end, stop_time))
            .add_systems(OnEnter(Screen::Game), run_time)
            .add_systems(OnEnter(Pause::On), (spawn_pause, stop_time))
            .add_systems(OnExit(Pause::On), run_time.run_if(in_state(Screen::Game)))
            .add_systems(Update, style_buttons)
            .add_systems(
                Update,
                (watch_outcome, escape).run_if(in_state(Screen::Game)),
            );
    }
}

fn stop_time(mut time: ResMut<Time<Virtual>>) {
    time.pause();
}

fn run_time(mut time: ResMut<Time<Virtual>>) {
    time.unpause();
}

/// Starts a game from the choice and draws its board.
fn begin(world: &mut World, choice: Choice, seed: u64) {
    let spec = &BOARDS[choice.board.min(BOARDS.len() - 1)];
    let board = match build_board(spec) {
        Ok(board) => board,
        Err(error) => {
            error!("Die Karte {} lässt sich nicht bauen: {error}", spec.name);
            return;
        }
    };
    let mut seats = vec![Seat::Computer; choice.seats.clamp(2, 4)];
    seats[0] = Seat::Player;
    start_game(
        world,
        GameSetup {
            board,
            seats,
            minutes: choice.minutes,
            seed,
            placements: None,
        },
    );
    crate::input::reset(world);
    crate::hud::reset(world);
    spawn_scene(world);
}

/// Removes the game and its drawing.
fn leave(world: &mut World) {
    let old: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<Rival>, With<Group>, With<BoardScene>)>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    world.remove_resource::<Game>();
    world.remove_resource::<crate::rules::Board>();
}

#[allow(clippy::too_many_arguments)]
fn on_activate(
    activate: On<Activate>,
    actions: Query<&Action>,
    real: Res<Time<Real>>,
    mut choice: ResMut<Choice>,
    mut view: ResMut<BoardView>,
    mut ending: ResMut<Ending>,
    mut screen: ResMut<NextState<Screen>>,
    mut pause: ResMut<NextState<Pause>>,
    game: Option<Res<Game>>,
    rivals: Query<&Rival>,
    groups: Query<&Group>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    match *action {
        Action::NewGame => screen.set(Screen::Setup),
        #[cfg(not(target_arch = "wasm32"))]
        Action::Quit => {
            commands.write_message(AppExit::Success);
        }
        Action::Board(i) => choice.board = i,
        Action::Seats(n) => choice.seats = n,
        Action::Minutes(m) => choice.minutes = m,
        Action::Begin => {
            choice.games += 1;
            // The seed comes from when the player pressed the button; from
            // there on the game depends on nothing but its commands.
            let seed = mix(real.elapsed().as_nanos() as u64 ^ choice.games.rotate_left(32));
            let picked = *choice;
            commands.queue(move |world: &mut World| begin(world, picked, seed));
            pause.set(Pause::Off);
            screen.set(Screen::Game);
        }
        Action::Back => screen.set(Screen::Start),
        Action::Resume => pause.set(Pause::Off),
        Action::ToggleView => *view = view.toggled(),
        Action::GiveUp => {
            if let Some(game) = game {
                let player = rivals.iter().find(|r| r.seat == Seat::Player).map(|r| r.id);
                *ending = Ending {
                    title: "Aufgegeben".into(),
                    ranking: standings(&game, rivals.iter().map(|r| r.id), groups.iter()),
                    player,
                };
            }
            pause.set(Pause::Off);
            screen.set(Screen::End);
        }
        Action::MainMenu => {
            commands.queue(leave);
            pause.set(Pause::Off);
            screen.set(Screen::Start);
        }
    }
}

fn watch_outcome(
    game: Option<Res<Game>>,
    rivals: Query<&Rival>,
    mut ending: ResMut<Ending>,
    mut screen: ResMut<NextState<Screen>>,
) {
    let Some(outcome) = game.as_ref().and_then(|g| g.outcome.as_ref()) else {
        return;
    };
    let player = rivals.iter().find(|r| r.seat == Seat::Player).map(|r| r.id);
    let title = match outcome.reason {
        EndReason::TimeUp => "Die Zeit ist um".to_string(),
        EndReason::LastRival(r) if Some(r) == player => "Nur du bist übrig".to_string(),
        EndReason::LastRival(r) => format!("Nur {} ist übrig", RIVAL_NAMES[r.index()]),
    };
    *ending = Ending {
        title,
        ranking: outcome.ranking.clone(),
        player,
    };
    screen.set(Screen::End);
}

fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<Pause>>,
    mut pause: ResMut<NextState<Pause>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        pause.set(match state.get() {
            Pause::Off => Pause::On,
            Pause::On => Pause::Off,
        });
    }
}

fn button(action: Action, label: impl Into<String>) -> impl Bundle {
    (
        Button,
        action,
        Hovered::default(),
        Node {
            padding: UiRect::axes(px(18), px(9)),
            margin: UiRect::all(px(4)),
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(palette::BUTTON),
        children![text(label, 18.0, palette::TEXT)],
    )
}

/// A full-screen layer that centres a column of content.
fn layer(screen: impl States + Copy, background: Color) -> impl Bundle {
    (
        DespawnOnExit(screen),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(10),
            ..default()
        },
        BackgroundColor(background),
        GlobalZIndex(10),
    )
}

fn row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

fn spawn_start(mut commands: Commands, fonts: Res<Fonts>) {
    let start = commands
        .spawn((
            layer(Screen::Start, palette::BACKGROUND),
            children![
                heading(&fonts, "Waldstatt Go", 56.0),
                text("Rivalen im Wald", 24.0, palette::TEXT),
                (
                    Node {
                        max_width: px(620),
                        margin: UiRect::vertical(px(16)),
                        ..default()
                    },
                    children![text(
                        "Rivalen fällen sich durch den Wald um Einsiedeln. Wer die Gruppe eines \
                     anderen mit einem Ring aus offenem Boden einschliesst, gewinnt ihre \
                     Waldleute, wie im Go.",
                        17.0,
                        palette::MUTED,
                    )],
                ),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: px(14),
                        ..default()
                    },
                    children![text(CREDITS, 13.0, palette::MUTED)],
                ),
            ],
        ))
        .id();
    let buttons = commands.spawn(button(Action::NewGame, "Neues Spiel")).id();
    let mut menu = commands.spawn(row());
    menu.add_child(buttons);
    // A page cannot be quit; only the native game offers it.
    #[cfg(not(target_arch = "wasm32"))]
    menu.with_child(button(Action::Quit, "Beenden"));
    let menu = menu.id();
    commands.entity(start).add_child(menu);
}

fn spawn_setup(mut commands: Commands, fonts: Res<Fonts>) {
    let boards: Vec<Entity> = BOARDS
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            commands
                .spawn(button(
                    Action::Board(i),
                    format!("{} – {}", spec.name, spec.description),
                ))
                .id()
        })
        .collect();
    let seats: Vec<Entity> = (2..=4)
        .map(|n| {
            let label = format!("{n} (du und {})", n - 1);
            commands.spawn(button(Action::Seats(n), label)).id()
        })
        .collect();
    let minutes: Vec<Entity> = [5, 10, 15, 20]
        .map(|m| {
            commands
                .spawn(button(Action::Minutes(m), format!("{m} Minuten")))
                .id()
        })
        .to_vec();
    let section = |commands: &mut Commands, title: &str, items: Vec<Entity>| {
        let title = commands.spawn(text(title, 20.0, palette::HEADING)).id();
        let mut list = commands.spawn(row());
        list.add_children(&items);
        let list = list.id();
        commands
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                margin: UiRect::vertical(px(8)),
                ..default()
            })
            .add_children(&[title, list])
            .id()
    };
    let parts = [
        section(&mut commands, "Karte", boards),
        section(&mut commands, "Rivalen am Tisch", seats),
        section(&mut commands, "Spieldauer", minutes),
    ];
    let buttons = commands
        .spawn((
            row(),
            children![
                button(Action::Back, "Zurück"),
                button(Action::Begin, "Spiel beginnen")
            ],
        ))
        .id();
    let title = commands.spawn(heading(&fonts, "Neues Spiel", 40.0)).id();
    commands
        .spawn(layer(Screen::Setup, palette::BACKGROUND))
        .add_children(&[title])
        .add_children(&parts)
        .add_children(&[buttons]);
}

fn spawn_pause(mut commands: Commands, fonts: Res<Fonts>, view: Res<BoardView>) {
    commands.spawn((
        layer(Pause::On, palette::PANEL),
        children![
            heading(&fonts, "Pause", 44.0),
            (
                row(),
                children![
                    button(Action::Resume, "Weiter"),
                    button(Action::ToggleView, "Ansicht wechseln"),
                    button(Action::GiveUp, "Spiel beenden"),
                    button(Action::MainMenu, "Hauptmenü"),
                ],
            ),
            text(
                format!("Ansicht: {} (Taste V)", view.label()),
                16.0,
                palette::MUTED
            ),
            (
                Node {
                    max_width: px(640),
                    margin: UiRect::top(px(12)),
                    ..default()
                },
                children![text(crate::hud::HELP, 16.0, palette::TEXT)],
            ),
            text(CREDITS, 13.0, palette::MUTED),
        ],
    ));
}

fn spawn_end(mut commands: Commands, fonts: Res<Fonts>, ending: Res<Ending>) {
    let title = commands.spawn(heading(&fonts, "Spielende", 44.0)).id();
    let reason = commands
        .spawn(text(ending.title.clone(), 22.0, palette::TEXT))
        .id();
    let mut lines = vec![title, reason];
    for (place, standing) in ending.ranking.iter().enumerate() {
        let name = RIVAL_NAMES[standing.rival.index()];
        let who = if Some(standing.rival) == ending.player {
            format!("{name} (du)")
        } else {
            name.to_string()
        };
        let label = format!(
            "{}. {who}: {} Waldleute, {} Bäume",
            place + 1,
            standing.waldleute,
            standing.trees
        );
        lines.push(
            commands
                .spawn(text(label, 22.0, rival_colour(standing.rival)))
                .id(),
        );
    }
    let buttons = commands
        .spawn((
            row(),
            children![
                button(Action::Begin, "Nochmals"),
                button(Action::MainMenu, "Hauptmenü")
            ],
        ))
        .id();
    lines.push(buttons);
    commands
        .spawn(layer(Screen::End, palette::PANEL))
        .add_children(&lines);
}

fn style_buttons(
    choice: Res<Choice>,
    mut buttons: Query<(&Action, &Hovered, Has<Pressed>, &mut BackgroundColor), With<Button>>,
) {
    for (action, hovered, pressed, mut colour) in &mut buttons {
        let chosen = match *action {
            Action::Board(i) => choice.board == i,
            Action::Seats(n) => choice.seats == n,
            Action::Minutes(m) => choice.minutes == m,
            _ => false,
        };
        let wanted = match (pressed, hovered.get(), chosen) {
            (true, ..) => palette::BUTTON_PRESSED,
            (_, true, _) => palette::BUTTON_HOVER,
            (_, _, true) => palette::BUTTON_CHOSEN,
            _ => palette::BUTTON,
        };
        if colour.0 != wanted {
            colour.0 = wanted;
        }
    }
}

/// The time left, for the HUD.
pub fn time_left(game: &Game) -> String {
    clock(game.seconds_left())
}
