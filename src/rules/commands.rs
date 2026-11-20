//! Commands: the only way into the rules (ADR 0002).
//!
//! The player's input and the computer rivals queue the same commands in
//! [`Controls`]. Each fixed step applies them in the game's rival order, never
//! in the order systems ran or commands arrived.

use bevy::prelude::*;

use super::board::{Board, RivalId, Tile};
use super::game::{Game, GameEvent, Rival};
use super::group::{Group, GroupId, Line, Trail, plan_line, split_group};
use super::tuning::Tuning;

/// What a rival tells one of its groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    /// Cut along these tiles from where the group stands, replacing the rest
    /// of its line; no tiles stops it. Drawing onto another group of the same
    /// rival merges the two when the line ends there.
    Line { group: GroupId, tiles: Vec<Tile> },
    /// Split the group into two on its tile.
    Split { group: GroupId },
    /// The group that receives the rival's next Waldmensch.
    Receiver { group: GroupId },
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

/// Applies the queued commands. A command naming a group the rival does not
/// have (it was merged or enclosed meanwhile) does nothing.
pub fn apply_commands(
    mut commands: Commands,
    mut controls: ResMut<Controls>,
    mut board: ResMut<Board>,
    tuning: Res<Tuning>,
    mut game: ResMut<Game>,
    mut rivals: Query<&mut Rival>,
    mut groups: Query<(&mut Group, &mut Trail, &mut Line)>,
) {
    let mut queue = std::mem::take(&mut controls.queue);
    // Stable: each rival's own commands keep their order.
    queue.sort_by_key(|command| game.rank(command.rival));
    for Command { rival, order } in queue {
        let id = match &order {
            Order::Line { group, .. } | Order::Split { group } | Order::Receiver { group } => {
                *group
            }
        };
        let Some((mut group, mut trail, mut line)) = groups
            .iter_mut()
            .find(|(group, ..)| group.id == id && group.rival == rival)
        else {
            continue;
        };
        match order {
            Order::Line { tiles, .. } => {
                let planned = plan_line(&board, &tuning, &group, &tiles);
                // Work towards the same next tile carries on.
                if planned.front() != line.0.front() {
                    group.progress = 0;
                }
                line.0 = planned;
            }
            Order::Split { .. } => {
                let new_id = game.next_group_id();
                if let Some((new, new_trail)) =
                    split_group(&mut board, &mut group, &mut trail, new_id)
                {
                    let step = game.step;
                    game.events.push(GameEvent::Split {
                        step,
                        rival,
                        from: group.id,
                        new: new_id,
                    });
                    commands.spawn((new, new_trail, Line::default()));
                }
            }
            Order::Receiver { .. } => {
                if let Some(mut owner) = rivals.iter_mut().find(|r| r.id == rival) {
                    owner.receiver = Some(id);
                }
            }
        }
    }
}
