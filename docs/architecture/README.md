# Architecture

This document describes the current implementation and conventions for extending
it. [AGENTS.md](../../AGENTS.md) is the agent working brief;
[CONTRIBUTING.md](../../CONTRIBUTING.md) covers shaping, review, and validation.
[ADRs](decisions/README.md) record significant architectural choices and rationale.

## Current structure and runtime

The project is one Rust binary crate implementing the
[Rivalen im Wald pitch](../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md): two to
four rivals cut through the forest round Einsiedeln with groups of Waldleute and win
each other's groups by closing rings of open ground round them. Every seat but the
player's is a computer rival. [Cargo.toml](../../Cargo.toml) uses Rust edition 2024
and Bevy 0.20 with default features disabled and the `2d`, `audio`, and `ui` feature
sets, plus `web` for the browser target, and `serde` and `serde_json` to read the
water pages ([ADR 0004](decisions/0004-compile-the-boards-from-copied-waldstatt-water-pages.md)).
Dependencies have a higher development optimization level than the application.
[Cargo.lock](../../Cargo.lock) pins dependency resolution;
[.cargo/config.toml](../../.cargo/config.toml) configures Clang and LLD for x86_64
Linux, and [.mise/](../../.mise/config.toml) pins the toolchain and defines tasks
([ADR 0003](decisions/0003-run-project-tasks-with-mise.md)). The same crate builds
for the browser ([ADR 0005](decisions/0005-build-the-same-crate-for-the-browser.md)).

[src/main.rs](../../src/main.rs) composes the app from plugins in two layers
([ADR 0002](decisions/0002-keep-the-rules-headless-behind-rival-commands.md)).

**Rules**, which run without a window and are used by the tests, in
[src/rules/](../../src/rules/mod.rs) (`RulesPlugin`):

- `board.rs`: `Tile` (board coordinates, `x` east, `y` south), `Ground` (land,
  river, lake by number), and the `Board` resource: ground, the land regions water
  and the edge divide it into, trees per tile, and which rival holds each tile open.
- `waters.rs`: the `BOARDS` (Einsiedeln, Biberbrugg, Hütten, 100 × 100 tiles each)
  and `build_board`, which rasterizes the compiled-in water pages from
  [data/waters/](../../data/waters/README.md) as the Waldstatt's `world.ts` does.
- `tuning.rs`: the `Tuning` table of play-test values and `line_limit(size)`,
  `(log2(size) + 1) × 10` computed exactly in integers.
- `game.rs`: `Rival`, `Seat`, `GameSetup`, `start_game`, the `Game` resource (seed,
  step, length, the rival order from the seed, events, outcome), placing the rivals,
  new Waldleute, the clock, the end and `standings`.
- `group.rs`: the `Group`, `Trail` and `Line` components, `plan_step` (what
  entering the next tile costs or why it may not), `plan_line`, merging, splitting,
  and `advance_groups`.
- `commands.rs`: `Order` (`Line`, `Split`, `Receiver`), `Controls`, and
  `apply_commands`.
- `enclosure.rs`: `Partition` (how one rival's open tiles divide the board),
  `resolve_enclosures`, and `room`/`is_trapped` (how far a group can still cut).
- `sight.rs`: each rival's `Sight`: what it sees now and what it has seen.
- `rng.rs`: SplitMix64 and tile hashes, the only randomness.

**Computer rivals**, also headless: [src/ai.rs](../../src/ai.rs)
(`ComputerRivalsPlugin`) decides for every computer seat four times a second, before
the rules, from that rival's `Sight` only, and writes `Controls`.

**Presentation**, which needs a window:

- [style.rs](../../src/style.rs): palette, rival colours, Fira Sans as the default
  font (Bevy's own has no umlauts), shapes drawn in code, text helpers.
- [view.rs](../../src/view.rs): `BoardView` (top-down or isometric), its projection
  and inverse, and the transforms of the board's root and plane.
- [ground.rs](../../src/ground.rs): the board as one image, a few pixels per tile,
  painted from what the player's rival sees, and the flashes of enclosures.
- [markers.rs](../../src/markers.rs): group discs, selection, danger rings, ships,
  line dots, the hovered tile and water names.
- [input.rs](../../src/input.rs): the pointer and keys into commands; selection
  and the line being drawn. [camera.rs](../../src/camera.rs): pan, zoom, centring.
- [screens.rs](../../src/screens.rs): the `Screen` and `Pause` states, start menu,
  setup, pause menu and end with the ranking. [hud.rs](../../src/hud.rs): the
  on-screen text during a game.

### Frame and fixed-step flow

1. `PreUpdate`, after Bevy's `InputSystems`, only while playing: the tile under the
   pointer is picked through the active view; the selection follows merges and
   enclosures and sends `Order::Receiver` when it changes; pointer and keys queue
   `Order::Line` and `Order::Split` for the player's rival.
2. `FixedUpdate` at 64 Hz, while a game runs: computer rivals decide; then, chained
   in `RuleSystems`, `apply_commands`, `advance_groups`, `resolve_enclosures`,
   `grant_waldleute`, `update_sight`, and `finish_step` (step count, rivals out, end).
3. `Update`: group entities get their discs; line dots are rebuilt when the
   player's lines or the drawn line change; menus, HUD and camera update.
4. `PostUpdate`, before transform propagation: the view's transforms, discs placed
   and interpolated, selection, hover, names; the ground image is repainted where a
   tile's look changed.

Menus and pausing stop `Time<Virtual>`, so no fixed step runs outside a game.

### The rules

A game starts from a `GameSetup`: board, seats, minutes and seed. The seed sows the
trees (one to three per tile), orders the rivals, and places each rival's five
Waldleute, each a group of one, within five tiles of a home in its own quarter of
the board's largest land region. The order decides every tie: commands, moves and
rings resolve rival by rival in that order, then by group number.

A group's line is straight steps from its tile, at most `line_limit(size)` long,
never onto a lake for a group too big for a ship and never ending in a river.
Each step the group adds `speed(size)` work towards the next tile; on reaching that
tile's cost it enters. Cutting forest opens the tile, earns its trees, and adds it
to the trail; the trail keeps its last `line_limit(size)` tiles open and the oldest
grows back. Walking on the rival's own open tiles is cheaper. A tile next to (or
diagonal to) another rival's open tile cannot be cut, nor one another rival's group
stands on; the line then stops short. Wading into a river costs ten cuts per tile
and, up front, the trees for every river tile ahead; launching a ship costs twenty
cuts and its trees and carries at most ten; sailing is cheap; landing cuts as usual.
A line that ends on another group of the same rival merges the two: sizes and trees
add, trails join in the order they were cut and are trimmed to the new limit.
Splitting gives the new group half the Waldleute, that share of the trees, and the
older part of the trail.

After the moves, each rival whose open tiles changed splits the board again: its
open tiles and the water are walls, parts are found with straight steps (so rings
hold diagonally), and within each land region every part smaller than its largest is
enclosed. Every other rival's group standing on land in such a part changes sides
with its Waldleute, trees and trail; groups on water are safe. Rivals are tested in
order until no group changes sides, each group at most once per step.

Every `waldmensch_interval`, each rival still in the game gets one more Waldmensch
for its chosen group or its largest. A rival without groups is out. The game ends
when its time is up or only one rival is left; the ranking orders rivals by
Waldleute, then trees, then the order.

### Drawing

The ground image has six pixels per tile. Unexplored tiles are black, tiles seen
before are in fog with their waters, other rivals' trails and groups show only
within sight of the player's groups. Forest in another rival's margin is tinged red,
so a closing ring shows before it closes; a red ring round a disc marks a group
that can reach fewer than `trapped_room` tiles, and the HUD warns when it is one of
the player's. The tile each of the player's trails closes next carries a sapling.
When a ring closes, the enclosed part and the ring flash in the colour of the rival
that closed it for six seconds, also under fog, and the HUD lists who enclosed whom.

## Code conventions and boundaries

Use Bevy entities and components for game objects, resources for shared state, and
systems for behavior. Keep application composition easy to locate in `main.rs`.
Keep the rules and computer rivals free of rendering and device input so they run
headless; presentation reads rule state and writes only `Controls`. Rule positions
are whole tiles and progress is integer work; nothing in the rules reads the clock
or an unseeded generator, and iteration that decides an outcome is ordered.
Presentation may add drawing components (discs, children) to group entities but
never changes rule components.

Follow rustfmt and nearby Rust naming and import conventions. Keep pure logic, such
as rasterizing, partitions, line planning and projections, in plain functions that
unit tests can call directly. Read device input only in `input.rs` and `camera.rs`.

New dependencies, simulation timing, and persisted formats can create lasting
constraints; explain such choices in an ADR. Balance values belong in `Tuning`.

## Validation

The [contribution guide](../../CONTRIBUTING.md#setup-and-validation) lists the
formatting, Clippy, build, test, and manual-run checks. There is no CI workflow
for pull requests; the Pages workflow builds the browser version on `main`.

- **Unit tests** beside each module cover the line limit, regions, rasterizing the
  water pages and every board, partitions (rings, diagonal rings, gaps, water and
  edge, water alone), trapping, line planning, margins, river costs, splitting,
  projections in both views, drawn lines, and the computer rivals' ring runs.
- **Game tests** in [src/tests.rs](../../src/tests.rs) step the real plugins headless,
  one fixed step per update, with commands written to `Controls`: cutting speed by
  size, trails growing back, margins, merging, splitting, closing a ring, a trapped
  group, safety on water and not on the shore, wading, ships, new Waldleute, the end
  and ranking, placement on a real board, determinism of a whole game with computer
  rivals, and computer rivals expanding and enclosing each other.

These tests guard rules, not feel. Play after any tuning change, and check views,
drawing and input by hand.

## Documentation boundaries

- [Game idea](../idea.md): direction and longer-term possibilities.
- [Pitches](../pitches/README.md): intended work, rough solutions, risks, and
  exclusions.
- Architecture documentation: current structure, behavior, and coding conventions.
- [ADRs](decisions/README.md): significant choices, alternatives, consequences, and
  checks that expose drift.

Update conventions with the code. Keep ideas clearly separate from implemented
behavior. Preserve decision history by amending or superseding an ADR when its
choice changes.
