---
tags: [adr]
status: accepted
date: 2026-11-20
amends: [0001-move-groups-tile-by-tile-on-the-board.md]
amended-by: [0008-lay-the-board-in-hexes.md]
sources:
  - src/rules/group.rs
  - src/rules/moves.rs
  - src/rules/enclosure.rs
  - src/rules/commands.rs
  - src/rules/game.rs
  - src/people.rs
  - src/input.rs
  - 0001-move-groups-tile-by-tile-on-the-board.md
  - 0002-keep-the-rules-headless-behind-rival-commands.md
---

# 0007 — Spread each group over the tiles of its Waldleute

Amended by [ADR 0008](0008-lay-the-board-in-hexes.md): a Waldmensch steps to any
of the six neighbours of its hex, with no corner rule; the rest holds as below.

## Context

As first built, a group stood on one tile and carried its size as a number;
the Waldleute were only drawn. After playing it, the wish was for the
Waldleute themselves to matter: the player drags any of them, the others follow
as if tied by a cord with a longest gap, a group that stands still gathers
round where it was sent and lets its trail grow back, groups merge when their
clearings join, and a ring takes a group by closing round its clearing, except
those already in or across a river; a ring round a lake takes the boat on it.

Alternatives considered:

- **Keep one tile per group and only draw a file of people.** Little change, but
  dragging a Waldmensch in the middle could not lead anything, and gathering,
  shared clearings and partial captures at a river have nothing to act on.
- **Free positions with a physical rope.** Closest to "bungee cord", but floats
  in the rules break ADR 0001's exact, deterministic stepping.
- **Every Waldmensch on a whole tile, moved by rules on tiles.**

## Decision

A `Group` holds its Waldleute (`Member`: tile, the tile before, the step it
entered, its work and the tile it works towards), one of them the lead, the
tile it gathers round, and the water tiles it crossed. One Waldmensch stands on
a tile of land; on water they share. `Order::Line` names the Waldmensch it
starts from, who leads from then on.

A Waldmensch steps one tile at a time, straight or diagonally; a diagonal step
needs land on both tiles beside it with no other rival's clearing, so it never
slips through a diagonal ring or past a river's corner, and it counts as one
tile against `line_limit(size)`. A click (`Order::Goto`) sends the lead the
fastest way there as its rival knows the board
([src/rules/route.rs](../../../src/rules/route.rs)); a line that turns out
blocked is routed round to its end the same way, within `line_limit(size)`
tiles.

Each step, in the game's rival order ([src/rules/moves.rs](../../../src/rules/moves.rs)):
the lead works along the line at the group's speed; a cut next to another
clearing of its rival, or a step into one, merges that group into its own.
While there is a line, the others follow over the clearing and the water it
crossed, each at most `Tuning::gap` tiles per place behind the one before it,
along the cheapest way (walking costs one, cutting four). Without a line they
gather on the tiles nearest its end, cutting where they must, bounded by water,
the edge, other rivals' margins and the rival's other clearings; settled ones
step inward to make room. Each trail tile remembers the last step a
Waldmensch of the group stood on it; the trail keeps `line_limit(size)` tiles
while the group cuts, and once it has stood still for `Tuning::regrow_after`
(time enough to draw a ring in several lines) grows back one tile every
`Tuning::regrow_steps`, the one untouched longest, never under a Waldmensch, on a tile
it gathers on, or where it would split the Waldleute apart. A piece of clearing
none of its Waldleute can reach grows back at once: no clearing is left empty.

A ring takes the Waldleute standing on land in an enclosed part; partitions with
the rivers alone as walls take those on an enclosed lake. They join the
encloser's group with the most tiles in the ring, with their share of the trees
and the trail inside ([src/rules/enclosure.rs](../../../src/rules/enclosure.rs)).

ADR 0001 still holds: every position is a whole tile and every progress integer
work; only what moves is now each Waldmensch.

## Consequences

- The rules do more per step: two searches over a group's surroundings and a
  check of which trail tile may grow back; about twice the time per step in the
  tests, well inside a frame.
- Groups grow, move and shrink as shapes on the board, so the player reads a
  group from its people; both views draw every Waldmensch (`people.rs`).
- Splitting leaves two touching clearings; they merge again only when one of
  them cuts next to the other or its lead steps into it.
- Captured Waldleute stand where they were taken, apart from their new group;
  they cut their way to it only where they can.
- Computer rivals still lead with the lead's tile and size; they do not drag
  from the middle.

## The tell

- `cargo test --locked the_waldleute_follow_whoever_leads standing_still` and
  `cargo test --locked waldleute_in_a_river_stay_free` pass.
- `rg -n "pub size:|pub tile:" src/rules/group.rs` finds only `Member`'s tile:
  a group has no size or tile of its own.
