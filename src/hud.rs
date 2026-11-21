//! On-screen text during a game: the clock and the next Waldmensch, each
//! rival's Waldleute and trees, the selected group, what happened, a warning
//! when one of the player's groups is being enclosed, and the keys.

use bevy::prelude::*;

use crate::ground::Watcher;
use crate::input::Selection;
use crate::rules::enclosure::{can_split, is_trapped};
use crate::rules::game::{GameEvent, RIVAL_NAMES, STEPS_PER_SECOND};
use crate::rules::group::activity;
use crate::rules::sight::Sight;
use crate::rules::{Board, Game, Group, Line, Rival, Trail, Tuning, line_limit};
use crate::screens::{Screen, time_left};
use crate::style::{clock, palette, rival_colour, text};

/// The keys, also shown in the pause menu.
pub const HELP: &str = "Klick auf eine Gruppe oder ihre Lichtung: Gruppe wählen · \
                        von dort ziehen: Linie ziehen · Klick daneben: schnellster Weg dorthin · \
                        Klick auf die eigene Spur: Die Spitze folgt dem gewählten Feld, \
                        auch wenn die Spur nachwächst · \
                        Doppelklick auf eine andere Gruppe: die gewählte geht zu ihr, sie vereinen sich · \
                        Rechtsklick auf eine Gruppe oder T: teilen · rechts ziehen: teilen und \
                        eine Hälfte losschicken · Rechtsklick daneben oder X: anhalten · \
                        Tab: nächste Gruppe · V: Ansicht wechseln · Leertaste: zur Gruppe · \
                        WASD, Pfeile, mittlere Maustaste: schieben · Mausrad, +/−: Zoom · \
                        Touchscreen: tippen wie klicken, lange drücken wie Rechtsklick, \
                        zwei Finger schieben und zoomen · Esc: Pause";

/// The reminder during a game; Esc shows all keys.
const SHORT_HELP: &str = "Klick: wählen oder hingehen · Klick auf die Spur: Ring schliessen · \
                          Ziehen: Linie · Doppelklick: zur Gruppe · \
                          Rechtsklick: teilen · Tab: nächste Gruppe · V: Ansicht · \
                          Esc: Pause und alle Tasten";

/// Notices kept on screen.
const NOTICES: usize = 6;

#[derive(Component)]
struct Status;

#[derive(Component)]
struct RivalRow(usize);

#[derive(Component)]
struct RivalSwatch(usize);

#[derive(Component)]
struct RivalText(usize);

#[derive(Component)]
struct GroupPanel;

#[derive(Component)]
struct NewsPanel;

#[derive(Component)]
struct Warning;

/// What happened, newest last.
#[derive(Resource, Default)]
struct News {
    events_seen: usize,
    notices: Vec<(String, Color)>,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<News>()
            .add_systems(OnEnter(Screen::Game), spawn_hud)
            .add_systems(
                Update,
                (read_news, show_status, show_group, show_news, show_warning)
                    .chain()
                    .run_if(in_state(Screen::Game)),
            );
    }
}

/// Forgets the news, for a new game.
pub fn reset(world: &mut World) {
    world.insert_resource(News::default());
}

fn panel(node: Node) -> impl Bundle {
    (
        DespawnOnExit(Screen::Game),
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(12), px(8)),
            row_gap: px(3),
            border_radius: BorderRadius::all(px(6)),
            ..node
        },
        BackgroundColor(palette::PANEL),
    )
}

fn spawn_hud(mut commands: Commands) {
    let status = commands
        .spawn(panel(Node {
            top: px(10),
            left: px(10),
            ..default()
        }))
        .with_child((Status, text("", 17.0, palette::TEXT)))
        .id();
    for i in 0..crate::rules::board::MAX_RIVALS {
        commands.entity(status).with_child((
            RivalRow(i),
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            },
            Visibility::Hidden,
            children![
                (
                    RivalSwatch(i),
                    Node {
                        width: px(12),
                        height: px(12),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                ),
                (RivalText(i), text("", 15.0, palette::TEXT)),
            ],
        ));
    }
    commands.spawn((
        panel(Node {
            bottom: px(10),
            left: px(10),
            max_width: px(460),
            ..default()
        }),
        children![(GroupPanel, text("", 15.0, palette::TEXT))],
    ));
    commands.spawn((
        panel(Node {
            top: px(10),
            right: px(10),
            max_width: px(380),
            ..default()
        }),
        NewsPanel,
    ));
    commands.spawn((
        panel(Node {
            bottom: px(10),
            right: px(10),
            max_width: px(420),
            ..default()
        }),
        children![text(SHORT_HELP, 13.0, palette::MUTED)],
    ));
    commands.spawn((
        DespawnOnExit(Screen::Game),
        Warning,
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Visibility::Hidden,
        children![(
            Node {
                padding: UiRect::axes(px(14), px(6)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.35, 0.03, 0.02, 0.9)),
            children![text(
                "Achtung: Eine deiner Gruppen wird eingeschlossen!",
                18.0,
                palette::TEXT
            )],
        )],
    ));
}

fn read_news(game: Option<Res<Game>>, watcher: Res<Watcher>, mut news: ResMut<News>) {
    let Some(game) = game else {
        return;
    };
    let me = watcher.0;
    let seen = news.events_seen.min(game.events.len());
    for event in &game.events[seen..] {
        let notice = match event {
            GameEvent::Enclosed {
                step,
                by,
                from,
                size,
                ..
            } => {
                let who = if *size == 1 {
                    "einen Waldmenschen".to_string()
                } else {
                    format!("{size} Waldleute")
                };
                let text = if Some(*from) == me {
                    format!("{} schliesst {who} von dir ein", RIVAL_NAMES[by.index()])
                } else if Some(*by) == me {
                    format!("Du schliesst {who} von {} ein", RIVAL_NAMES[from.index()])
                } else {
                    format!(
                        "{} schliesst {who} von {} ein",
                        RIVAL_NAMES[by.index()],
                        RIVAL_NAMES[from.index()]
                    )
                };
                let colour = if Some(*from) == me {
                    palette::WARNING
                } else {
                    rival_colour(*by)
                };
                Some((*step, text, colour))
            }
            GameEvent::Joined { step, rival, .. } if Some(*rival) == me => Some((
                *step,
                "Ein neuer Waldmensch taucht irgendwo im Wald auf".into(),
                palette::TEXT,
            )),
            GameEvent::Out { step, rival } => Some((
                *step,
                if Some(*rival) == me {
                    "Du hast alle Waldleute verloren".into()
                } else {
                    format!("{} hat alle Waldleute verloren", RIVAL_NAMES[rival.index()])
                },
                palette::MUTED,
            )),
            _ => None,
        };
        if let Some((step, text, colour)) = notice {
            news.notices.push((
                format!("{}  {text}", clock(step / STEPS_PER_SECOND)),
                colour,
            ));
        }
    }
    news.events_seen = game.events.len();
    let excess = news.notices.len().saturating_sub(NOTICES);
    news.notices.drain(..excess);
}

#[allow(clippy::too_many_arguments)]
fn show_status(
    game: Option<Res<Game>>,
    board: Option<Res<Board>>,
    tuning: Res<Tuning>,
    watcher: Res<Watcher>,
    rivals: Query<&Rival>,
    groups: Query<&Group>,
    mut status: Query<&mut Text, With<Status>>,
    mut rows: Query<(&RivalRow, &mut Visibility)>,
    mut swatches: Query<(&RivalSwatch, &mut BackgroundColor)>,
    mut texts: Query<(&RivalText, &mut Text), Without<Status>>,
) {
    let (Some(game), Some(board)) = (game, board) else {
        return;
    };
    let next = game.next_waldmensch.saturating_sub(game.step) / STEPS_PER_SECOND;
    let line = format!(
        "{} · noch {} · neuer Waldmensch in {}",
        board.name,
        time_left(&game),
        clock(next.min(tuning.waldmensch_interval / STEPS_PER_SECOND))
    );
    for mut text in &mut status {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
    let mut order: Vec<&Rival> = rivals.iter().collect();
    order.sort_by_key(|r| r.id);
    for (row, mut visibility) in &mut rows {
        let shown = row.0 < order.len();
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (swatch, mut colour) in &mut swatches {
        if let Some(rival) = order.get(swatch.0) {
            colour.0 = rival_colour(rival.id);
        }
    }
    for (row, mut text) in &mut texts {
        let Some(rival) = order.get(row.0) else {
            continue;
        };
        let own: Vec<&Group> = groups.iter().filter(|g| g.rival == rival.id).collect();
        let who = if Some(rival.id) == watcher.0 {
            format!("{} (du)", rival.name())
        } else {
            rival.name().to_string()
        };
        let line = if rival.out {
            format!("{who}: keine Waldleute mehr")
        } else {
            format!(
                "{who}: {} Waldleute · {} Bäume · {} Gruppen",
                own.iter().map(|g| g.size()).sum::<u32>(),
                own.iter().map(|g| g.trees).sum::<u32>(),
                own.len()
            )
        };
        if text.0 != line {
            text.0 = line;
        }
    }
}

fn show_group(
    board: Option<Res<Board>>,
    selection: Res<Selection>,
    tuning: Res<Tuning>,
    sights: Query<(&Rival, &Sight)>,
    groups: Query<(&Group, &Trail, &Line)>,
    mut panel: Query<&mut Text, With<GroupPanel>>,
) {
    let Some(board) = board else {
        return;
    };
    let selected = selection
        .group
        .and_then(|id| groups.iter().find(|(g, ..)| g.id == id));
    let line = match selected {
        None => "Keine Gruppe gewählt".to_string(),
        Some((group, trail, line)) => {
            let limit = line_limit(group.size());
            let mut out = format!(
                "Gewählte Gruppe: {} Waldleute · {} Bäume · {}\nSpur {}/{} offen · Linie {} von höchstens {} Feldern",
                group.size(),
                group.trees,
                activity(&board, group, line).label(),
                trail.0.len(),
                limit,
                line.0.len(),
                limit,
            );
            if group.regrows.is_some() {
                out.push_str("\nDer markierte Setzling wächst als Nächstes wieder zu.");
            }
            let sight = sights.iter().find(|(r, _)| r.id == group.rival);
            let sees = |t| sight.is_some_and(|(_, s)| s.sees(&board, t));
            if group.size() >= 2 && !can_split(&board, group, tuning.split_room, sees) {
                out.push_str("\nZu eng zum Teilen.");
            }
            if group.size() > tuning.ship_capacity {
                out.push_str(&format!(
                    "\nZu gross für ein Schiff (höchstens {}).",
                    tuning.ship_capacity
                ));
            }
            out.push_str(&format!(
                "\nFluss: {} Bäume je Feld · Schiff: {} Bäume",
                tuning.river_trees, tuning.ship_trees
            ));
            out
        }
    };
    for mut text in &mut panel {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
}

fn show_news(
    mut commands: Commands,
    news: Res<News>,
    panels: Query<(Entity, Option<&Children>), With<NewsPanel>>,
) {
    if !news.is_changed() {
        return;
    }
    for (panel, children) in &panels {
        for &child in children.into_iter().flatten() {
            commands.entity(child).despawn();
        }
        if news.notices.is_empty() {
            commands.entity(panel).with_child(text(
                "Noch ist nichts geschehen.",
                14.0,
                palette::MUTED,
            ));
        }
        for (notice, colour) in &news.notices {
            commands
                .entity(panel)
                .with_child(text(notice.clone(), 14.0, *colour));
        }
    }
}

fn show_warning(
    board: Option<Res<Board>>,
    tuning: Res<Tuning>,
    watcher: Res<Watcher>,
    sights: Query<(&Rival, &Sight)>,
    groups: Query<&Group>,
    mut warnings: Query<&mut Visibility, With<Warning>>,
) {
    let (Some(board), Some(me)) = (board, watcher.0) else {
        return;
    };
    let Some((_, sight)) = sights.iter().find(|(r, _)| r.id == me) else {
        return;
    };
    let trapped = groups
        .iter()
        .filter(|g| g.rival == me)
        .any(|g| is_trapped(&board, g, tuning.trapped_room, |t| sight.sees(&board, t)));
    for mut visibility in &mut warnings {
        *visibility = if trapped {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
