//! The rules of Waldstatt Go, headless (ADR 0002).
//!
//! Rivals cut through the forest with groups of Waldleute and win each other's
//! groups by closing rings of open ground round them. Everything here runs
//! under `MinimalPlugins` on the fixed timestep: no rendering, windowing or
//! device input. Presentation reads these components and resources and writes
//! only [`Controls`]; computer rivals do the same.

pub mod board;
pub mod commands;
pub mod enclosure;
pub mod game;
pub mod group;
pub mod moves;
pub mod rng;
pub mod route;
pub mod sight;
pub mod tuning;
pub mod waters;

use bevy::prelude::*;

pub use board::{Board, Ground, RivalId, Tile};
pub use commands::{Controls, Order};
pub use game::{Game, GameSetup, Rival, Seat, start_game};
pub use group::{Group, GroupId, Line, Trail};
pub use tuning::{Tuning, line_limit};

/// The fixed-step rule systems, in order. Computer rivals run before this set;
/// presentation that depends on its results orders itself after.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleSystems;

pub struct RulesPlugin;

impl Plugin for RulesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tuning>()
            .init_resource::<Controls>()
            .add_systems(
                FixedUpdate,
                (
                    commands::apply_commands,
                    moves::advance_groups,
                    enclosure::resolve_enclosures,
                    game::grant_waldleute,
                    sight::update_sight,
                    game::finish_step,
                )
                    .chain()
                    .in_set(RuleSystems)
                    .run_if(game::game_running),
            );
    }
}
