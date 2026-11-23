---
tags: [adr]
status: accepted
date: 2026-11-20
amended-by:
  - 0007-spread-each-group-over-the-tiles-of-its-waldleute.md
  - 0008-lay-the-board-in-hexes.md
sources:
  - src/rules/group.rs
  - src/rules/tuning.rs
  - src/view.rs
  - src/markers.rs
  - src/input.rs
  - ../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md
  - 0002-keep-the-rules-headless-behind-rival-commands.md
---

# 0001 — Move groups tile by tile on the board, whichever way it is viewed

Amended by [ADR 0007](0007-spread-each-group-over-the-tiles-of-its-waldleute.md):
each Waldmensch of a group now stands on its own tile and moves tile by tile;
everything below still holds for each of them.

Amended by [ADR 0008](0008-lay-the-board-in-hexes.md): tiles are hexes, and the
isometric view is the hex board squashed to half its height with north up, not
turned 45°; the projection, its inverse for picking and the test of every tile
still work as below.

## Context

Waldstatt Go is not a platformer. The
[Rivalen im Wald pitch](../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md)
puts groups of Waldleute on a board of 50 m tiles cut from the real map. A group
cuts along a line the player draws, one straight step from tile to tile, more
quickly the bigger it is, and much more slowly over water. Nothing jumps, falls or
slides, so the box kinematics of the Bevy template this project started from
([its movement record](https://github.com/waldstatt-lab/waldstatt-go/blob/3b174ac/docs/architecture/decisions/0001-move-the-player-with-hand-written-kinematics.md))
do not carry over.

The board is seen from above, and the player can switch between a top-down and an
isometric view while playing. Enclosure depends on exactly which tiles are open,
the rules must step deterministically for tests and a later multiplayer
([ADR 0002](0002-keep-the-rules-headless-behind-rival-commands.md)), and
a view switch must never change the game.

Alternatives considered:

- **Keep continuous positions** and derive the tile from them. Groups would glide
  freely, but every rule would have to round to tiles, and a step that changes how
  float rounding is done could open a different tile on another machine.
- **Simulate in screen or view coordinates.** Each view would need its own rules,
  and switching views would move groups.
- **A physics engine.** Nothing collides except with tiles that are open or not,
  so it would add compile time and version coupling to Bevy for nothing.
- **Discrete tile positions with integer progress on the fixed timestep.** The rules
  speak in tiles, the result is exact, and every view is just a way of drawing the
  same board.

## Decision

The simulation knows only board coordinates. A group's position is an integer
tile; its line is a queue of neighbouring tiles. Each `FixedUpdate` step adds an
integer amount of progress that depends on the group's size; when the progress
reaches the cost of the next tile (cutting forest, or crossing water at ten times
that), the group enters the tile and the trail updates. Speeds and costs are
play-test values in one tuning table. No rule uses floating-point positions.

Presentation owns the view. A view resource, top-down or isometric, chooses how a
tile is projected to a `Transform`, and the same projection, inverted, turns the
pointer into a tile for drawing lines. Switching views changes only that resource
and the drawing. Between steps, visuals glide a group's disc from the tile it left
to the tile it entered, as the template did with `PreviousPosition`.

As implemented, [src/rules/group.rs](../../../src/rules/group.rs) holds a group's
tile, the tile it came from, the step it entered, and its integer progress; the
costs and speeds are in [src/rules/tuning.rs](../../../src/rules/tuning.rs).
[src/view.rs](../../../src/view.rs) draws the isometric board as the top-down one
turned 45° and squashed to half its height: the ground, lines and
highlights are children of one plane whose transform is the view, and upright
things (discs, and in the isometric view trees, Waldleute and boats, see
[ADR 0006](0006-paint-the-isometric-sprites-in-code.md)) are placed with the
projection and drawn back to front by row and column.
A test maps every tile of a 100 × 100 board to its centre and back in both views.

## Consequences

- Rules and tests talk about tiles: a test can step a group along a line and assert
  the tile it reaches after a given number of steps, exactly.
- Deterministic stepping does not depend on float behaviour on native and in the
  browser.
- Groups visibly move in steps of whole tiles; smooth motion is presentation
  interpolation only, and a group is never between tiles in the rules.
- Every new view, such as zoom levels or a later 3D view, is presentation only;
  picking must use the inverse of the active projection.
- A group's speed has the granularity of a fixed step; very fast groups need costs
  and progress scaled up, not a smaller timestep.

## The tell

- Board rule tests step scripted groups and assert tiles and step counts; they
  pass headless without any view.
- `rg -n "f32|f64|Vec2|Vec3" src/rules/` finds no position or progress fields;
  the only hits are in `waters.rs`, which rasterizes the map data into tiles when
  a board is built. Review any other hit.
- `rg -n "Transform" src/rules/` finds nothing: only presentation code writes
  transforms.
- `cargo test --locked every_tile_maps_to_itself_and_back` passes.
- In the running game, switching views mid-move leaves every group on the same
  tile and its line unchanged. This needs a manual check; the searches cannot show
  that a projection and its inverse agree.
