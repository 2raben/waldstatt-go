//! Commands: the only way into the rules (ADR 0002).
//!
//! The player's input and the computer rivals queue the same commands in
//! [`Controls`]. Each fixed step applies them in the game's rival order, never
//! in the order systems ran or commands arrived.

use bevy::prelude::*;

use std::collections::{BTreeSet, VecDeque};

use super::board::{Board, RivalId, Tile};
use super::enclosure::can_split;
use super::game::{Game, GameEvent, Rival};
use super::group::{Group, GroupId, Line, Trail, plan_line, split_group};
use super::moves::regrowable;
use super::route::route;
use super::sight::Sight;
use super::tuning::{Tuning, line_limit};

/// What a rival tells one of its groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    /// Go along these tiles from the group's Waldmensch standing on `from`,
    /// which leads from now on, replacing the rest of the line; no tiles stops
    /// it. Where the line turns out blocked, the lead finds another way to its
    /// end. Cutting next to another clearing of the same rival, or walking into
    /// it, merges the two groups.
    Line {
        group: GroupId,
        from: Tile,
        tiles: Vec<Tile>,
    },
    /// Go the fastest way to `to` from the group's Waldmensch standing on
    /// `from`, which leads from then on: no more tiles than the group may draw,
    /// as its rival knows the board.
    Goto {
        group: GroupId,
        from: Tile,
        to: Tile,
    },
    /// Split the group in two. Without `from`, the half furthest back from
    /// the lead goes its own way; with it, the half nearest the Waldmensch
    /// standing on `from` goes, led by it along `tiles` as a drawn line. A
    /// group hemmed in by other rivals, water or the board's edge (less than
    /// `Tuning::split_room` tiles to reach) does not split.
    Split {
        group: GroupId,
        from: Option<Tile>,
        tiles: Vec<Tile>,
    },
}

impl Order {
    /// Splits the group, the half furthest back going its own way.
    pub fn split(group: GroupId) -> Self {
        Order::Split {
            group,
            from: None,
            tiles: Vec::new(),
        }
    }

    pub fn group(&self) -> GroupId {
        match self {
            Order::Line { group, .. } | Order::Goto { group, .. } | Order::Split { group, .. } => {
                *group
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub rival: RivalId,
    pub order: Order,
}

/// Commands waiting for the next fixed step.
#[derive(Resource, Default, Debug)]
pub struct Controls {
    pub queue: Vec<Command>,
}

impl Controls {
    pub fn send(&mut self, rival: RivalId, order: Order) {
        self.queue.push(Command { rival, order });
    }
}

/// The other clearings of a rival and the tiles round them, which a way the
/// group finds keeps clear of, so it does not merge by chance.
fn others_round(
    groups: &Query<(&mut Group, &mut Trail, &mut Line)>,
    rival: RivalId,
    id: GroupId,
) -> BTreeSet<Tile> {
    groups
        .iter()
        .filter(|(g, ..)| g.rival == rival && g.id != id)
        .flat_map(|(g, t, _)| t.tiles().chain(g.tiles()).collect::<Vec<_>>())
        .flat_map(|t| std::iter::once(t).chain(t.neighbours()))
        .collect()
}

/// Applies the queued commands. A command naming a group the rival does not
/// have (it was merged or enclosed meanwhile) does nothing.
#[allow(clippy::too_many_arguments)]
pub fn apply_commands(
    mut commands: Commands,
    mut controls: ResMut<Controls>,
    board: Res<Board>,
    tuning: Res<Tuning>,
    mut game: ResMut<Game>,
    sights: Query<(&Rival, &Sight)>,
    mut groups: Query<(&mut Group, &mut Trail, &mut Line)>,
) {
    let mut queue = std::mem::take(&mut controls.queue);
    // Stable: each rival's own commands keep their order.
    queue.sort_by_key(|command| game.rank(command.rival));
    for Command { rival, order } in queue {
        let id = order.group();
        let avoid = match order {
            Order::Goto { .. } => others_round(&groups, rival, id),
            _ => BTreeSet::new(),
        };
        let Some((mut group, mut trail, mut line)) = groups
            .iter_mut()
            .find(|(group, ..)| group.id == id && group.rival == rival)
        else {
            continue;
        };
        match order {
            Order::Line { from, .. } | Order::Goto { from, .. } => {
                let before = group.lead;
                if let Some(k) = group.member_on(from) {
                    group.lead = k;
                }
                let planned = match &order {
                    Order::Goto { to, .. } => {
                        let sight = sights.iter().find(|(r, _)| r.id == rival).map(|(_, s)| s);
                        let budget = line_limit(group.size()) as u32;
                        route(
                            &board,
                            &tuning,
                            &group,
                            sight,
                            *to,
                            |t| avoid.contains(&t),
                            budget,
                        )
                    }
                    Order::Line { tiles, .. } => plan_line(&board, &tuning, &group, tiles),
                    Order::Split { .. } => continue,
                };
                give_line(&mut group, &mut line, planned, before);
            }
            Order::Split { from, tiles, .. } => {
                let sight = sights.iter().find(|(r, _)| r.id == rival).map(|(_, s)| s);
                let sees = |t: Tile| sight.is_none_or(|s| s.sees(&board, t));
                if !can_split(&board, &group, tuning.split_room, sees) {
                    continue;
                }
                let lead = group.tile();
                let new_id = game.next_group_id();
                if let Some((mut new, new_trail)) =
                    split_group(&board, &mut group, &mut trail, new_id, from)
                {
                    if group.tile() != lead {
                        // The lead went with the other half: these gather.
                        give_line(&mut group, &mut line, VecDeque::new(), usize::MAX);
                    }
                    new.regrows = regrowable(&board, &new, &new_trail, &Default::default());
                    let mut new_line = Line::default();
                    if from.is_some() {
                        let planned = plan_line(&board, &tuning, &new, &tiles);
                        give_line(&mut new, &mut new_line, planned, usize::MAX);
                    }
                    let step = game.step;
                    game.events.push(GameEvent::Split {
                        step,
                        rival,
                        from: group.id,
                        new: new_id,
                    });
                    commands.spawn((new, new_trail, new_line));
                }
            }
        }
    }
}

/// Gives a group the line `planned` from its lead, which led before as
/// `before`: work towards the same next tile by the same lead carries on.
fn give_line(group: &mut Group, line: &mut Line, planned: VecDeque<Tile>, before: usize) {
    // A way round something blocking may use the whole allowance.
    group.goal = planned.back().copied();
    group.budget = line_limit(group.size()) as u32;
    let lead = group.lead;
    if lead != before || planned.front() != line.0.front() {
        group.members[lead].progress = 0;
        group.members[lead].toward = None;
    }
    if planned.is_empty() {
        group.target = group.tile();
    }
    line.0 = planned;
}
