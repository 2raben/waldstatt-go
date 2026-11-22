---
tags: [adr]
status: accepted
date: 2026-11-20
sources:
  - src/ai.rs
  - src/arena.rs
  - src/main.rs
  - .mise/tasks/arena
  - 0002-keep-the-rules-headless-behind-rival-commands.md
  - 0008-lay-the-board-in-hexes.md
---

# 0009 — Measure computer rivals in a headless arena

## Context

The computer rivals decide by a few rules whose ranges, ring sizes and
exploring scores were guessed and then carried over from square tiles to hexes
([ADR 0008](0008-lay-the-board-in-hexes.md)). Playing against them shows how
they feel, but not whether one change makes them stronger: one game depends
too much on where the rivals start. Judging a change needs many whole games
from many seeds, and the rules already run headless and deterministically
behind commands ([ADR 0002](0002-keep-the-rules-headless-behind-rival-commands.md)).

Alternatives considered:

- **Play-test only.** Keeps the code as it is, but a human can't play enough
  games to tell a better rival from a lucky one.
- **Train a policy with reinforcement learning.** It could find play that the
  rules miss, but it needs an ML runtime or a bridge to one, a great deal of
  compute, and a model that nobody can read or tune for fun. Its results must
  also stay the same natively and in the browser.
- **A separate binary or a library crate for the arena.** Cleaner to run, but
  the crate is one binary, and splitting it into a library would change every
  module's visibility to serve one developer tool.

## Decision

The computer rivals' judgement values are a `Temperament` in
[src/ai.rs](../../../src/ai.rs). Each rival reads its own from the `Temperaments`
resource, which gives the default to every rival the game doesn't name. The
game uses the default temperament for all computer seats. The opening now
gathers a hunting group of three, with new recruits joining it. The arena
retains the distributed opening as `rally_size=0` for comparison.

`waldstatt-go arena` ([src/arena.rs](../../../src/arena.rs), `mise run arena`)
plays whole games of computer rivals with the rules and the computer rivals
only, each in its own app stepped like the game tests, several on separate
threads. Game *k* uses seed *first + k* and the boards in turn. Seat *k mod
rivals* plays the challenger: the default temperament changed by `name=value`
arguments. The other seats play the default. The report gives the
challenger's mean place with its standard error, its wins, the Waldleute it
took and lost, and the mean group captures and rivals out per game. A ring that
captures several groups produces one enclosure event per captured group. The arena is a
subcommand of the game's binary, compiled only for native targets.

Values the arena finds better become the new defaults by hand, after
playing against them. A search over temperaments, or a scored choice among
candidate orders, can build on the same arena later.

## Consequences

- Any change to a temperament can be measured over as many seeded games as
  needed, and the same arguments give the same report.
- The challenger plays the default, so the arena measures strength against
  today's rivals, not against a player, and not fun.
- Games are slow. A 10-minute game takes about 70 s on one core of a Ryzen 7
  8845HS, and 24 such games took about five minutes on its 16 threads. Most
  of the time is spent in the rules' per-step path searches for the Waldleute
  following their lead (`Paths::search` in `moves.rs`), not in the computer
  rivals.
- A difference in mean place smaller than about twice its standard error is
  noise; 48 games give a standard error of about 0.16 places with four rivals.
- The game binary reads its first argument. Anything other than `arena`
  starts the game as before; the browser build has no arena.

## The tell

`rg -n "const [A-Z_]+: (i32|usize|u32|u64) =" src/ai.rs` lists only
`THINK_EVERY` and `SEARCH_LIMIT`: judgement values belong in `Temperament`.
`mise run test arena` checks that the arena's games are deterministic.
`mise run arena --games 24` with no changes should give a mean place within
about two standard errors of even. A search can't show whether a
temperament is fun; play against it.
