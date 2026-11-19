---
tags: [adr]
status: proposed
date: 2026-11-20
sources:
  - ../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md
  - 0001-move-groups-tile-by-tile-on-the-board.md
---

# 0002 — Keep the rules headless and drive every rival through commands

## Context

The [Rivalen im Wald pitch](../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md)
asks for rules that run headless and deterministically, so tests can play whole
games through, and for computer rivals that play by the same rules as the player.
A later multiplayer will run the same steps on every machine. Enclosure, regrowth
and ties are where the bugs will be, so they must be testable without a window,
a GPU or fake devices.

The rules also decide what each rival can see: computer rivals see only what
their groups see, and the player's fog shows the same thing. Visibility is
therefore part of the game, not of the drawing.

Alternatives considered:

- **Read devices and the pointer directly in rule systems.** Least code, but
  untestable headless, tied to one view, and a click that lands on a frame without
  a fixed step is lost.
- **Let computer rivals change the game state directly.** Simple at first, but they
  could do what the player cannot, and every rule would have two entry points.
- **An input-action crate** such as `leafwing-input-manager`. Useful for rebinding,
  but the game has few actions; it can be adopted later behind the same boundary.
- **One command type for every rival**, collected in the `Controls` resource and
  applied by the rules in a fixed order.

## Decision

[main.rs](../../../src/main.rs) composes two layers of plugins.

- **Rules**: the board, rivals, groups, lines and trails, enclosure, water,
  visibility, new Waldleute, the clock and the ranking. They add no rendering,
  windowing or device input, run under `MinimalPlugins` in `FixedUpdate`, and
  hold all state in components and resources.
- **Presentation**: input, the board views, fog, markers, camera, menus and
  HUD. Input samples devices in `PreUpdate` after `InputSystems`, turns the
  pointer into tiles through the active view
  ([ADR 0001](0001-move-groups-tile-by-tile-on-the-board.md)), and writes
  commands for the player's rival. Presentation reads rule state but never
  changes it.

A command names its rival and says what to do: draw a line for a group, split a
group, or choose the group that receives the next Waldmensch. Merging is a line
onto one of the rival's own groups. The player's input and the computer rivals
write the same commands to `Controls`; computer rivals read only their own
visibility. Each fixed step drains the queued commands in a rival order derived
from the game's seed, never in system or arrival order, and resolves ties in the
same order. Pausing, view switching and menus are presentation and app state, not
commands.

A game is its board, its seed and its sequence of commands per step. Given the
same three, it ends the same way.

## Consequences

- Tests play scripted games by writing commands, exactly as input and computer
  rivals do, and can assert enclosures, regrowth and the ranking headless.
- A later multiplayer can exchange commands with their step number instead of game
  state.
- Computer rivals cannot cheat: they act only through commands and see only what
  their groups see.
- Adding a device, a view or rebinding touches only presentation; the tests do not
  catch view or input-mapping bugs, so check those in the game.
- Any randomness in the rules comes from the seed; nothing reads the wall clock or
  an unseeded generator.

## The tell

- `rg -n "ButtonInput|Gamepad|KeyCode|CursorMoved|Window" <rule modules>` finds
  nothing; device input appears only in the input module.
- `rg -n "Sprite|Camera2d|Camera3d|Text::|Node \{|Transform" <rule modules>`
  finds nothing.
- A test plays the same seed and commands twice and gets identical results; a
  difference means order or randomness leaked in.
- Computer rival systems query rule components read-only and write only
  `Controls`; review any `&mut` on rule state there.
- `cargo test --locked` builds the rules without `DefaultPlugins`.
