---
tags: [adr]
status: accepted
date: 2026-11-20
amended-by: [0008-lay-the-board-in-hexes.md]
sources:
  - data/waters/README.md
  - src/rules/waters.rs
  - src/style.rs
  - Cargo.toml
  - ../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md
---

# 0004 — Compile the boards from copied Waldstatt water pages

Amended by [ADR 0008](0008-lay-the-board-in-hexes.md): the pages are rasterized
into hexes by their centres, with the same half-step rule for rivers; copying
and compiling them in still holds as below.

## Context

The [Rivalen im Wald pitch](../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md)
cuts its boards from the real map, with the rivers and lakes of the Waldstatt
(`waldstatt-lab/waldstatt`). The Waldstatt generates them with `tools/waters.mjs` from
about 5 GB of swisstopo data and BAFU's stream orders, and publishes the result
as JSON pages of 200 × 200 tiles. The game must run natively and in the
browser, without network access, and its rules must run headless in tests.

Alternatives considered:

- **Port the water generator.** It reads gigabytes of GeoPackages; the pitch
  rules this out as a rabbit hole.
- **Fetch the Waldstatt's pages at run time.** Adds network access, which the
  pitch excludes, and makes tests depend on a server.
- **Load the pages as Bevy assets.** Works natively and in the browser, but the
  rules would wait for asset loading, and headless tests would need the asset
  server and files on disk.
- **Convert the pages to a format of our own.** Saves a JSON parser, but every
  update from the Waldstatt would need the converter run again, and the copy
  would no longer be checkable against the source.
- **Copy the pages unchanged and compile them in.** A few kilobytes per page;
  the board exists the moment a game starts, on every target and in tests.

## Decision

The pages a board needs are copied unchanged into
[data/waters/](../../../data/waters/README.md), with the Waldstatt commit they came
from. [src/rules/waters.rs](../../../src/rules/waters.rs) includes them with
`include_str!`, parses them with `serde_json`, and rasterizes each page into
tiles as the Waldstatt's `world.ts` does: lakes filled even-odd, and every tile
whose centre lies within half a tile of a river's line (0.6 for wide rivers) is
water. Distances are compared squared, so only exactly rounded arithmetic
decides a tile. The boards are windows onto that map (`BOARDS`), chosen to stay
clear of the Sihlsee, which the data know only as a reservoir's basin.

`serde` and `serde_json` are pinned to the versions Car Drive uses. The same
reasoning covers the font: Bevy's built-in font has no umlauts, so Fira Sans
(SIL Open Font License) is copied to `assets/fonts/` with its licence and
compiled in by [src/style.rs](../../../src/style.rs), replacing the default font.

## Consequences

- Games start instantly and identically natively, in the browser and in tests;
  there is no loading screen and no network access.
- The binary grows by the pages (about 25 kB) and the two fonts (about 900 kB).
- The pages are a copy: a fix in the Waldstatt's water data reaches the game
  only when someone copies the pages again and records the new commit.
- A board needing another page fails to build with a message until the page is
  added; the tests build every board.
- The swisstopo and BAFU credits must stay in the game; the start and pause
  screens show them.

## The tell

- `cargo test --locked every_board_builds_from_the_pages` passes.
- `rg -n "reqwest|ureq|http" src/` finds nothing: no board data is fetched.
- Comparing `data/waters/*.json` with the Waldstatt's published pages at the
  recorded commit shows no difference; a difference means the copy was edited.
- `rg -n "swisstopo" src/` finds the credit shown in the game.
