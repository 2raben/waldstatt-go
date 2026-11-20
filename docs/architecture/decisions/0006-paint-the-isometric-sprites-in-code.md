---
tags: [adr]
status: accepted
date: 2026-11-20
sources:
  - src/art.rs
  - src/scenery.rs
  - src/people.rs
  - src/ground.rs
  - 0004-compile-the-boards-from-copied-waldstatt-water-pages.md
  - 0005-build-the-same-crate-for-the-browser.md
---

# 0006 — Paint the isometric sprites in code

## Context

The isometric view should look like a classic isometric strategy game: trees
standing in the forest, each group shown as a file of Waldleute, and water
with more detail than a flat colour. That needs sprites: several trees, a
Waldmensch in every rival's colour with walking, felling and wading frames, a
boat and a glint. The game has no artist yet, loads nothing at run time
(ADRs 0004 and 0005), and must not copy another game's art.

Alternatives considered:

- **Free sprite packs.** Each needs its licence checked and credited, rarely
  matches the rivals' colours or the forest round Einsiedeln, and would be the
  first assets loaded from disk, or would need compiling in.
- **Drawing sprites by hand now.** The look is still settling; every change in
  proportions would mean redrawing every frame in every colour.
- **Paint them in code at start-up.** Shapes, light and colours are parameters;
  a rival's colour is just another input; nothing is loaded.

## Decision

[src/art.rs](../../../src/art.rs) paints every sprite when the app starts:
each drawing is a function from a point to a colour, sampled nine times per
pixel at three pixels per world unit, lit from the upper left, with smoothed
noise for needles, leaves and bark. The drawings are packed into one atlas,
so the board draws in few batches; a `Piece` names a drawing's rectangle, size
and the point that stands on the ground. Trees stand where the ground draws
their tops and stumps (`tree_spots` in [src/ground.rs](../../../src/ground.rs)),
planted once per game by [src/scenery.rs](../../../src/scenery.rs);
[src/people.rs](../../../src/people.rs) poses the Waldleute. The ground itself
is painted in chunks with the same helpers, at sixteen pixels per tile.

## Consequences

- No art files, no licences beyond the fonts', and the same pictures natively
  and in the browser.
- Painting costs some time at start-up and on a new game (the atlas, and the
  ground's shore distances and noise); switching views repaints the ground.
- The look is changed in Rust and judged by running the game; the tests only
  check that every sprite is painted and that sprites do not overlap in the
  atlas.
- Hand-drawn or commissioned art can replace a drawing later without changing
  where things stand, as long as it keeps the `Piece` (size and anchor).

## The tell

- `cargo test --locked every_sprite_is_painted_and_packed_apart` passes.
- `rg -n "asset_server|AssetServer" src/` finds nothing: no sprite is loaded.
- Pressing V in a game shows trees, files of Waldleute and the detailed water;
  this needs a look in the running game.
