---
tags: [adr]
status: accepted
date: 2026-11-20
amends:
  - 0001-move-groups-tile-by-tile-on-the-board.md
  - 0004-compile-the-boards-from-copied-waldstatt-water-pages.md
  - 0007-spread-each-group-over-the-tiles-of-its-waldleute.md
sources:
  - src/rules/board.rs
  - src/rules/waters.rs
  - src/rules/enclosure.rs
  - src/rules/route.rs
  - src/view.rs
  - src/ground.rs
  - src/ai.rs
  - ../../pitches/hex-board/pitch--hex-board.md
---

# 0008 — Lay the board in hexes

## Context

The first board was cut in squares, and a Waldmensch stepped to any of the eight
tiles round it. A diagonal step cost as much as a straight one but went 1.4 times
as far, so groups crossed the board faster diagonally, a line reached further, and
the same trail closed a diamond twice the area of a square ring. Three rules
existed only because squares meet at corners: rings connected diagonally while
enclosed parts were found with straight steps only; a diagonal step needed land
and no other rival's clearing on both tiles beside it, and otherwise was made as
two straight steps; and a brook was rasterized half a tile wide so straight steps
could not slip through it. The [hex board pitch](../../pitches/hex-board/pitch--hex-board.md)
shaped the change.

Alternatives considered:

- **Keep squares and charge diagonals √2.** Movement would be fair, but rings
  would still connect at corners, and the corner rules and the mixed rule for
  rings and parts would stay.
- **Hexes as an option beside squares.** Every rule, the computer rivals, the
  picking, the ground and both views would need both grids behind one
  interface, for twice the tests. The pitch rules it out.
- **Flat-top hexes, or turning the board.** Point-up hexes lie in rows, so the
  board stays a rectangle of rows that the water pages, the ground's chunks and
  the storage already use.

## Decision

Tiles are hexes standing point up, in rows; every odd row is shifted half a tile
east. [`Tile`](../../../src/rules/board.rs) keeps its column and row, so the
board's storage, its rows of text in tests and its chunks are unchanged, and
converts to axial coordinates in one place for its six neighbours, its distance
in steps, the straightest line to another tile, the tiles within a radius, and the
tile under a point. A tile's centre lies in board units of one step (50 m): rows
are √3 / 2 apart, so a board of 100 columns needs 115 rows to cover about the
same ground.

Every rule uses the six neighbours: stepping, a line's steps, rings and the parts
they split the board into, the margin between rivals, gathering, merging, sight
(a hex disc of `view_radius` steps) and the computer rivals' searches. The corner
rule for diagonal steps and the mixed rule for rings and parts are gone.

[src/rules/waters.rs](../../../src/rules/waters.rs) rasterizes the same water
pages into hexes: a hex is lake where its centre lies inside a lake, and river
where its centre lies within half a step of a river's line (0.6 for wide rivers).
Two neighbouring hexes on either side of a line are one step apart, so one of them
lies within half a step of it; a test checks on every board that no step between
land crosses a river's line.

Both views keep north up. [src/view.rs](../../../src/view.rs) draws the top-down
board at 16 world units per step and the isometric one as the same hexes squashed
to half their height, at 24 units per step east, instead of turning the board
45°. The pointer picks the nearest hex through the inverted projection.
[src/ground.rs](../../../src/ground.rs) paints each pixel of the ground from the
hex it lies in.

## Consequences

- One step is the same in every direction: travel time, line length and the area
  a ring holds no longer depend on the grid's directions. The same trail holds
  about two thirds of what a diamond held on squares and four thirds of a square
  ring, so rings are harder to close; tune the line limit and regrowth rather than
  how rings work.
- The rules lost code: no corner rule, no detour of two straight steps, one
  neighbourhood for rings and parts. The tests describe hex boards in text, odd
  rows written with a leading space.
- Hexes 16 pixels wide have ragged edges on the ground; the linear sampler softens
  them. The board's sides are ragged too, half a tile on alternate rows.
- In the isometric view north is now up, not upper right, and trees hide the
  tiles north of them rather than north-west.
- Distances in the computer rivals and the tuning are counted in steps; their
  ranges were tuned for squares and are play-test values.
- Rectangles of rows and columns are still closed rings: consecutive rows of one
  column always touch, so the original rectangular hunts also held. The rivals now plan along the
  three hex axes, at least two steps from the visible enemy trail.

## The tell

Search the code for square-grid geometry: `rg -n "neighbours4|neighbours8|chebyshev|manhattan|FRAC_PI_4" src`
should find nothing. Run `cargo test --locked`: `no_step_on_a_board_crosses_a_river`
proves the brooks hold on every board, `every_tile_maps_to_itself_and_back` and
`the_pointer_picks_the_nearest_hex` check picking in both views, and
`six_neighbours_make_the_smallest_ring` checks enclosing. None of them shows
whether the game plays well on hexes; judge that in the running game.
