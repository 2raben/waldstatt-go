//! The Waldleute on the board, in both views: every Waldmensch of every group
//! stands where the rules put it, gliding from tile to tile.
//!
//! The lead swings its axe while it cuts, and so does anyone cutting their
//! way to the group; the others walk while they move and stand when they do
//! not. In a river they wade; on a lake they sit in the group's boat, which
//! appears on the shore while it is being built. Figures are drawing entities
//! tied to their group by [`FigureOf`] and despawn with it; the rules never
//! see them.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::art::{self, Art};
use crate::ground::Watcher;
use crate::markers::{Drawn, PlaceGroups, member_at};
use crate::rules::group::{Activity, activity, plan_step};
use crate::rules::sight::Sight;
use crate::rules::{Board, Game, Ground, Group, Line, Rival, Tile, Tuning};
use crate::view::{BoardView, Z_UPRIGHT};

#[derive(Component)]
#[relationship(relationship_target = Figures)]
struct FigureOf(Entity);

#[derive(Component, Default)]
#[relationship_target(relationship = FigureOf, linked_spawn)]
struct Figures(Vec<Entity>);

/// One Waldmensch drawn: which member of its group, where it was drawn last,
/// how far it has walked (for the steps) and which way it faces.
#[derive(Component)]
struct Figure {
    index: usize,
    at: Vec2,
    stride: f32,
    left: bool,
}

#[derive(Component)]
struct Boat;

pub struct PeoplePlugin;

impl Plugin for PeoplePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (equip, muster).chain())
            .add_systems(
                PostUpdate,
                march.after(PlaceGroups).before(TransformSystems::Propagate),
            );
    }
}

/// A group the markers have decorated that has no boat yet.
type NewGroup = (With<Group>, With<Drawn>, Without<Figures>);

/// Gives a new group its boat.
fn equip(mut commands: Commands, art: Res<Art>, groups: Query<Entity, NewGroup>) {
    for entity in &groups {
        let (sprite, anchor) = art.sprite(art.boat, 1.0);
        commands.spawn((
            Name::new("Boat"),
            Boat,
            FigureOf(entity),
            sprite,
            anchor,
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// Spawns or despawns figures so each group shows all its Waldleute.
fn muster(
    mut commands: Commands,
    art: Res<Art>,
    groups: Query<(Entity, &Group, &Drawn, &Figures)>,
    figures: Query<&Figure>,
) {
    for (entity, group, drawn, members) in &groups {
        let want = group.members.len();
        let mut have = 0;
        for &member in &members.0 {
            if let Ok(figure) = figures.get(member) {
                have += 1;
                if figure.index >= want {
                    commands.entity(member).despawn();
                }
            }
        }
        for index in have..want {
            let (sprite, anchor) = art.sprite(art.figure(group.rival, art::STAND), 1.0);
            commands.spawn((
                Name::new("Waldmensch"),
                Figure {
                    index,
                    at: drawn.0,
                    stride: 0.0,
                    left: false,
                },
                FigureOf(entity),
                sprite,
                anchor,
                Transform::default(),
                Visibility::Hidden,
            ));
        }
    }
}

/// Seat `k` in a boat whose foot is at `foot` (world coordinates): five a
/// side, turned into tile coordinates; and whether it is in the front row.
fn seat(view: BoardView, foot: Vec2, k: usize) -> (Vec2, bool) {
    let (column, back) = (k % 5, (k / 5) % 2 == 1);
    let offset = Vec2::new(-8.0 + 4.0 * column as f32, if back { 2.4 } else { 1.0 });
    (view.unproject(foot + offset * view.figure_scale()), !back)
}

type FigureParts = (
    &'static mut Figure,
    &'static mut Sprite,
    &'static mut Anchor,
    &'static mut Transform,
    &'static mut Visibility,
);

type BoatParts = (
    &'static mut Sprite,
    &'static mut Anchor,
    &'static mut Transform,
    &'static mut Visibility,
);

/// Places, poses and shows every group's Waldleute and boat.
#[allow(clippy::too_many_arguments)]
fn march(
    view: Res<BoardView>,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    watcher: Res<Watcher>,
    board: Option<Res<Board>>,
    game: Option<Res<Game>>,
    tuning: Res<Tuning>,
    art: Res<Art>,
    sights: Query<(&Rival, &Sight)>,
    groups: Query<(&Group, &Line, &Drawn, &Figures)>,
    mut figures: Query<FigureParts, Without<Boat>>,
    mut boats: Query<BoatParts, (With<Boat>, Without<Figure>)>,
) {
    let (Some(board), Some(game)) = (board, game) else {
        return;
    };
    let (dt, t) = (time.delta_secs(), time.elapsed_secs());
    let overstep = fixed.overstep_fraction();
    let scale = view.figure_scale();
    let iso = *view == BoardView::Isometric;
    let sight = watcher
        .0
        .and_then(|id| sights.iter().find(|(r, _)| r.id == id).map(|(_, s)| s));
    let sees = |tile: Tile| sight.is_none_or(|s| s.sees(&board, tile));
    // Which way is left on screen for a move in tile coordinates.
    let leftwards = |d: Vec2| if iso { d.x - d.y < 0.0 } else { d.x < 0.0 };
    for (group, line, drawn, members) in &groups {
        let own = Some(group.rival) == watcher.0;
        let doing = activity(&board, group, line);
        let lead_tile = group.tile();
        let ahead = line
            .0
            .front()
            .map(|&next| (next.centre() - lead_tile.centre()).normalize_or_zero());
        let left_ahead = ahead.is_some_and(leftwards);

        // The boat: where Waldleute are on a lake, or on the shore being built.
        let afloat_at = if matches!(board.ground(lead_tile), Ground::Lake(_)) {
            Some(drawn.0)
        } else {
            group
                .tiles()
                .find(|&t| matches!(board.ground(t), Ground::Lake(_)))
                .map(Tile::centre)
        };
        let boat = match (afloat_at, ahead) {
            (Some(at), _) => Some((at, 1.0)),
            (None, Some(dir)) if doing == Activity::BuildingShip => {
                let work = plan_step(&board, &tuning, group, &line.0, false).map_or(1, |p| p.work);
                let done = group.lead().progress as f32 / work.max(1) as f32;
                Some((drawn.0 + dir * 0.45, 0.25 + 0.75 * done))
            }
            _ => None,
        };
        let boat_tile = afloat_at.map(Tile::at);
        let shown_boat = boat.filter(|&(at, _)| own || sees(Tile::at(at)));
        for &member in &members.0 {
            if let Ok((mut sprite, mut anchor, mut transform, mut visibility)) =
                boats.get_mut(member)
            {
                match shown_boat {
                    Some((at, alpha)) => {
                        sprite.color = Color::srgba(1.0, 1.0, 1.0, alpha);
                        sprite.custom_size = Some(art.boat.size * scale);
                        sprite.flip_x = left_ahead;
                        anchor.0 = flipped(art.boat.anchor, left_ahead);
                        transform.translation = view.project(at).extend(Z_UPRIGHT + view.depth(at));
                        visibility.set_if_neq(Visibility::Inherited);
                    }
                    None => {
                        visibility.set_if_neq(Visibility::Hidden);
                    }
                }
            }
        }

        let mut seated = 0;
        for &member in &members.0 {
            let Ok((mut figure, mut sprite, mut anchor, mut transform, mut visibility)) =
                figures.get_mut(member)
            else {
                continue;
            };
            let k = figure.index;
            let Some(waldmensch) = group.members.get(k) else {
                visibility.set_if_neq(Visibility::Hidden);
                continue;
            };
            let in_boat = boat_tile == Some(waldmensch.tile)
                && matches!(board.ground(waldmensch.tile), Ground::Lake(_));
            let (target, front_row) = match boat {
                Some((at, _)) if in_boat => {
                    seated += 1;
                    seat(*view, view.project(at), seated - 1)
                }
                _ => (
                    member_at(group, k, line, &game, &board, &tuning, overstep),
                    false,
                ),
            };
            let before = figure.at;
            // Close in quickly; jump only far.
            figure.at = if figure.at.distance(target) > 3.0 {
                target
            } else {
                figure.at + (target - figure.at) * (1.0 - (-dt * 20.0).exp())
            };
            let moved = figure.at - before;
            figure.stride += moved.length() * 9.0;
            let cuts_towards = waldmensch
                .toward
                .filter(|&next| !board.is_water(next) && board.open(next).is_none());
            let cutting = cuts_towards.is_some() && (k != group.lead || doing == Activity::Cutting);
            if let Some(next) = cuts_towards.filter(|_| cutting) {
                figure.left = leftwards(next.centre() - waldmensch.tile.centre());
            } else if moved.length() > 1e-4 {
                figure.left = leftwards(moved);
            }
            let walking = dt > 0.0 && moved.length() / dt > 0.25;
            let frame = if in_boat {
                art::SEATED
            } else if board.ground(waldmensch.tile) == Ground::River {
                art::WADE + (t * 3.0 + k as f32) as usize % 2
            } else if cutting {
                art::CHOP + (t * 7.0 + k as f32 * 1.7) as usize % 4
            } else if walking {
                art::WALK + figure.stride as usize % 4
            } else {
                art::STAND
            };
            let piece = art.figure(group.rival, frame);
            sprite.rect = Some(piece.rect);
            sprite.custom_size = Some(piece.size * scale);
            sprite.flip_x = figure.left;
            anchor.0 = flipped(piece.anchor, figure.left);
            let z = match boat {
                // In the boat: over it, the front row over the back.
                Some((at, _)) if in_boat => {
                    Z_UPRIGHT + view.depth(at) + if front_row { 0.0003 } else { 0.0002 }
                }
                _ => Z_UPRIGHT + view.depth(figure.at) + 0.0002,
            };
            transform.translation = view.project(figure.at).extend(z);
            visibility.set_if_neq(if own || sees(Tile::at(figure.at)) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

/// A sprite's anchor when it is drawn mirrored.
fn flipped(anchor: Vec2, flip: bool) -> Vec2 {
    if flip {
        Vec2::new(-anchor.x, anchor.y)
    } else {
        anchor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boat_seats_ten_in_two_rows_round_its_foot() {
        for view in [BoardView::TopDown, BoardView::Isometric] {
            let foot = view.project(Vec2::new(10.5, 10.5));
            let seats: Vec<(Vec2, bool)> = (0..10).map(|k| seat(view, foot, k)).collect();
            assert_eq!(seats.iter().filter(|(_, front)| *front).count(), 5);
            for (at, _) in &seats {
                assert!(at.distance(Vec2::new(10.5, 10.5)) < 1.5, "{view:?} {at}");
            }
        }
    }
}
