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

- `board.rs`: `Tile`, a hex standing point up in column `x` (east) of row `y`
  (south), every odd row shifted half a tile east, with its six neighbours, its
  distance in steps, lines, discs, its centre in board units and the tile under a
  point ([ADR 0008](decisions/0008-lay-the-board-in-hexes.md)); `Ground` (land,
  river, lake by number); and the `Board` resource: ground, the land regions water
  and the edge divide it into, trees per tile, and which rival holds each tile open.
- `waters.rs`: the `BOARDS` (Einsiedeln, Biberbrugg, Hütten, 100 hexes by 115
  rows each, about 5 × 5 km) and `build_board`, which rasterizes the compiled-in
  water pages from [data/waters/](../../data/waters/README.md) into hexes by their
  centres.
- `tuning.rs`: the `Tuning` table of play-test values and `line_limit(size)`,
  `(log2(size) + 1) × 10` computed exactly in integers.
- `game.rs`: `Rival`, `Seat`, `GameSetup`, `start_game`, the `Game` resource (seed,
  step, length, the rival order from the seed, events, outcome), placing the rivals,
  new Waldleute, the clock, the end and `standings`.
- `group.rs`: the `Group` (its Waldleute as `Member`s, the lead, where it
  gathers), `Trail` and `Line` components, `plan_step` (what entering the next
  tile costs the lead or why it may not), `plan_line`, `Paths` (cheapest ways over
  tiles), the file order and splitting.
- `route.rs`: `route`, the fastest way for a lead to a tile as its rival knows
  the board, for clicks and for lines that turn out blocked; to the group's own
  trail, a way round that closes a ring.
- `moves.rs`: `advance_groups`: the lead, merging, following and gathering, and
  the trail growing back ([ADR 0007](decisions/0007-spread-each-group-over-the-tiles-of-its-waldleute.md)).
- `commands.rs`: `Order` (`Line`, `Goto`, `Close`, `Split`), `Controls`, and
  `apply_commands`.
- `enclosure.rs`: `Partition` (how one rival's open tiles divide the board, with
  all water or only rivers as walls), `resolve_enclosures`, and
  `room`/`is_trapped`/`can_split` (how far a group can still cut).
- `sight.rs`: each rival's `Sight`: what it sees now and what it has seen.
- `rng.rs`: SplitMix64 and tile hashes, the only randomness.

**Computer rivals**, also headless: [src/ai.rs](../../src/ai.rs)
(`ComputerRivalsPlugin`) decides for every computer seat four times a second, before
the rules, from that rival's `Sight` only, and writes `Controls`. Its ranges, ring
sizes and exploring scores are a `Temperament`: one per rival from the
`Temperaments` resource, the default unless the arena sets another.

**Arena**, native only: [src/arena.rs](../../src/arena.rs). `waldstatt-go arena`
(`mise run arena`) plays whole games of computer rivals, each in its own headless
app with the rules and the computer rivals stepped like the game tests, several at
once on separate threads. In each game one seat, a different one each game, plays
a changed temperament and the others the default; the report gives its mean place
with its standard error, wins, Waldleute taken and lost, and each game's rings and
rivals out ([ADR 0009](decisions/0009-measure-computer-rivals-in-a-headless-arena.md)).

**Presentation**, which needs a window:

- [style.rs](../../src/style.rs): palette, rival colours, Fira Sans as the default
  font (Bevy's own has no umlauts), shapes drawn in code, text helpers.
- [view.rs](../../src/view.rs): `BoardView` (top-down or isometric, both with
  north up; the isometric view squashes the board to half its height), its
  projection and inverse, picking the nearest hex, and the transforms of the
  board's root and plane.
- [ground.rs](../../src/ground.rs): the board as square chunks of images,
  sixteen pixels per step, each pixel painted from the hex it lies in and from
  what the player's rival sees, with the flashes of enclosures; where each tile's
  trees stand (`tree_spots`).
- [art.rs](../../src/art.rs): the sprites, painted in code at start-up into one
  atlas ([ADR 0006](decisions/0006-paint-the-isometric-sprites-in-code.md)).
- [scenery.rs](../../src/scenery.rs): the trees, saplings and glints of the
  isometric view. [people.rs](../../src/people.rs): every Waldmensch and boat, in
  both views.
- [markers.rs](../../src/markers.rs): group badges, where each Waldmensch is drawn
  (`member_at`), selection, danger rings, line dots, the hovered tile and water
  names.
- [input.rs](../../src/input.rs): the pointer (the mouse, or one finger, a long
  press standing for the right button) and keys into commands; selection and the
  line being drawn. [camera.rs](../../src/camera.rs): pan, zoom (also with two
  fingers), centring.
- [screens.rs](../../src/screens.rs): the `Screen` and `Pause` states, start menu,
  setup, pause menu and end with the ranking. [hud.rs](../../src/hud.rs): the
  on-screen text during a game.

### Frame and fixed-step flow

1. `PreUpdate`, after Bevy's `InputSystems`: the mouse buttons, in the order they
   came, and the first finger are read into one pointer every frame, so clicks in
   menus never reach a game; then, only while playing, the tile under it is picked
   through the active view; the selection follows merges and enclosures; pointer and keys
   queue `Order::Line` (from the Waldmensch pressed on, or the one nearest the
   clearing pressed), `Order::Goto` (a click beside the selected group, or a
   double click on another group), `Order::Close` (a click on the selected
   group's trail) and `Order::Split` (the right button or a long
   press on a group, with a line when dragged) for the player's rival.
2. `FixedUpdate` at 64 Hz, while a game runs: computer rivals decide; then, chained
   in `RuleSystems`, `apply_commands`, `advance_groups`, `resolve_enclosures`,
   `grant_waldleute`, `update_sight`, and `finish_step` (step count, rivals out, end).
3. `Update`: group entities get their badge, a boat and a figure for each
   Waldmensch; a new board gets its trees and glints;
   line dots are rebuilt when the player's lines, the drawn line or the view
   change; menus, HUD and camera update.
4. `PostUpdate`, before transform propagation: the view's transforms, badges placed
   over the interpolated lead (`Drawn`), then selection and the Waldleute; hover,
   names;
   the ground is repainted where a tile's look changed, then each tile's trees
   follow its look; glints twinkle.

Menus and pausing stop `Time<Virtual>`, so no fixed step runs outside a game.

### The rules

A game starts from a `GameSetup`: board, seats, minutes and seed. The seed sows the
trees (one to three per tile), orders the rivals, and places each rival's five
Waldleute, each a group of one, on random forest tiles anywhere on the board, rival
by rival, never next to another Waldmensch. The order decides every tie: commands, moves and
rings resolve rival by rival in that order, then by group number.

Every Waldmensch of a group stands on a tile of its own on land, or with others
on the water ([ADR 0007](decisions/0007-spread-each-group-over-the-tiles-of-its-waldleute.md)).
A line starts from one of them, the lead: steps of one tile to any of the six
neighbours of its hex, which share a side with it, so no step slips past a
corner ([ADR 0008](decisions/0008-lay-the-board-in-hexes.md)), at most
`line_limit(size)` tiles, never onto a lake for a group
too big for a ship and never ending in a river. A click asks for the fastest way
instead (`route.rs`: A* over the costs below, as the rival knows the board:
others' clearings and margins only where it sees them now, unexplored land as
forest; never next to the rival's other clearings; of ways equally fast, the one
nearest the straight line). Clicking the selected group's trail away from its
Waldleute records the field's position counted from the oldest end. The lead
aims for the field at that position in the current trail. When regrowth moves
it, the lead replans with the tiles left in its movement allowance. Crossing
open ground of the same rival costs twice a cut in this search, which favours
a route through the forest to close the ring. When a line turns out
blocked the lead finds another way to its end the same way, within
`line_limit(size)` tiles in all; with no way round it stops short. Each step the lead adds `speed(size)` work towards the
next tile; on reaching that tile's cost it enters. Cutting forest opens the tile,
earns its trees and adds it to the trail. Walking on the rival's own open tiles is
cheaper. A tile next to another rival's open tile cannot be cut,
nor one another rival's Waldmensch stands on; the line then stops short. Wading
into a river costs ten cuts per tile and, up front, the trees for every river tile
ahead; launching a ship costs twenty cuts and its trees, carries at most ten, and
takes the whole group aboard; sailing is cheap; landing cuts as usual.

While the group has a line, the others follow over the clearing and the water
it crossed, each at most `gap` tiles per place behind the one before it, cutting a
shortcut only where it is much shorter. Without a line they gather on the tiles
nearest its end, one to a tile, cutting where they must; water, the edge, other
rivals' margins and the rival's other clearings bound them. Each trail tile
remembers the last step a Waldmensch of the group stood on it. The trail keeps
`line_limit(size)` tiles while the group cuts, and once it has stood still for
`regrow_after` (eight seconds, so a ring can be drawn in several lines) one tile
grows back every `regrow_steps`: the one untouched longest, never one a
Waldmensch stands on, one it gathers on, or one that joins its Waldleute. A piece
of clearing none of its Waldleute can reach grows back at once, so no clearing is
ever empty. When the lead cuts next to
another clearing of the rival, or walks into one, the two groups merge: Waldleute,
trees, trails and water add. Clearings that already touch, as the two halves of a
split do, are not joined by a cut, only by walking into the other. A click's way to a tile in or beside another clearing
of the rival goes there, so the groups merge. Splitting sends the half furthest
back from the lead away with that share of the trees; each trail tile goes with the
nearer half. A split with a line sends instead the half nearest the Waldmensch it
starts from, which leads that half along the line (as long as the half may draw);
if that was the lead, the staying half's Waldmensch nearest it leads them, and
they gather where they stand. A group on the water, or one that can reach fewer
than `split_room` tiles from its lead (as its rival sees the board, so other
rivals' margins, water and the edge hem it in), does not split.

After the moves, each rival whose open tiles changed splits the board again: its
open tiles and the water are walls, parts are found through the six neighbours of
each hex, as rings are, and within each land region every part smaller than its largest is
enclosed. Every other rival's Waldmensch standing on land in such a part is taken,
and so is one on a lake in a part enclosed with the rivers alone as water walls;
those in a river, or across it, stay free. The taken join the encloser's group with
the most tiles of the ring, with their share of the trees and the trail inside; a
group with nobody left is gone. Rivals are tested in order until nobody is taken,
each group at most once per step.

Every `waldmensch_interval`, each rival still in the game gets one more Waldmensch:
a group of one on a random forest tile, from a generator seeded by the game's seed
and step, with no clearing or Waldmensch round it and outside every other rival's
rings. A rival without groups is out. The game ends
when its time is up or only one rival is left; the ranking orders rivals by
Waldleute, then trees, then the order.

### Drawing

The ground has sixteen pixels per step, each pixel painted from the hex it lies
in; seen from above, open hexes are outlined. Each rival sees a hex disc of
`view_radius` steps round each of its Waldleute. Unexplored tiles are black, tiles seen
before are in fog with their waters, other rivals' trails and groups show only
within sight of the player's groups. Forest in another rival's margin is tinged red,
so a closing ring shows before it closes; a red ring round a badge marks a group
that can reach fewer than `trapped_room` tiles, and the HUD warns when it is one of
the player's. The tile each of the player's trails closes next carries a sapling.
When a ring closes, the enclosed part and the ring flash in the colour of the rival
that closed it for six seconds, also under fog, and the HUD lists who enclosed whom.

Water is deeper away from the shore, with foam along it and waves; land along it
has a bank; cut tiles show the stumps of their trees. Seen from above, forest
shows its treetops. In the isometric view the ground shows the forest floor and
the trees stand on it: as many as the tile yields, dimmed in fog, reddened in a
margin, tinted by a flash, and see-through where they stand in front of open
ground, so trails stay readable; the sapling stands on the tile that grows back
next. In both views every Waldmensch stands where the rules put it, gliding from
tile to tile: whoever cuts swings an axe, the others walk or stand, they wade in
rivers and sit in a boat on a lake, which appears on the shore while it is being
built. A badge over the lead shows the group's size, and the selection is a ring
on the ground round the lead. In the isometric view, north is up as seen from
above; upright things are drawn back to front by row, trees turn see-through
where open ground in sight lies north of them, line dots and the hovered hex are
drawn over the trees, and water names lie on the water. Draw heights are
constants in `view.rs`.

## Code conventions and boundaries

Use Bevy entities and components for game objects, resources for shared state, and
systems for behavior. Keep application composition easy to locate in `main.rs`.
Keep the rules and computer rivals free of rendering and device input so they run
headless; presentation reads rule state and writes only `Controls`. Rule positions
are whole tiles and progress is integer work; nothing in the rules reads the clock
or an unseeded generator, and iteration that decides an outcome is ordered.
Presentation may add drawing components (badges, children, and figures tied to
a group by `FigureOf`) to group entities but never changes rule components.

Follow rustfmt and nearby Rust naming and import conventions. Keep pure logic, such
as rasterizing, partitions, line planning and projections, in plain functions that
unit tests can call directly. Read device input only in `input.rs` and `camera.rs`.

New dependencies, simulation timing, and persisted formats can create lasting
constraints; explain such choices in an ADR. Balance values belong in `Tuning`, and
the computer rivals' judgement values in `Temperament`.

## Validation

The [contribution guide](../../CONTRIBUTING.md#setup-and-validation) lists the
formatting, Clippy, build, test, and manual-run checks. There is no CI workflow
for pull requests; the Pages workflow builds the browser version on `main`.

- **Unit tests** beside each module cover the line limit, regions, rasterizing the
  water pages and every board (no step between land crosses a river's line),
  hex neighbours, distances and lines, partitions (rings, the smallest ring of
  six, gaps, water and edge, water alone), trapping, line planning, margins, river
  costs, splitting (also from a Waldmensch), projections and picking the nearest
  hex in both views, the ground's hexes and shores, drawn lines, and the computer
  rivals' ring runs.
- **Game tests** in [src/tests.rs](../../src/tests.rs) step the real plugins headless,
  one fixed step per update, with commands written to `Controls`: cutting speed by
  size, trails growing back, margins, merging (also by a click on another
  clearing), splitting (with a line, the half sent away staying apart, and not
  when hemmed in), closing a ring (also by going to the own trail), a trapped group, safety on water and not on the shore, wading, ships, new Waldleute, the end
  and ranking, placement on a real board, determinism of a whole game with computer
  rivals, and computer rivals expanding and enclosing each other.

- **Arena tests** in [src/arena.rs](../../src/arena.rs) check its arguments and that
  a game with a changed temperament plays the same every time.

These tests guard rules, not feel. Play after any tuning change, and check views,
drawing and input by hand. The arena measures how temperaments place against each
other, not whether they are fun to play against.

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
