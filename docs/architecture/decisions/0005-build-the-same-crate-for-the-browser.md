---
tags: [adr]
status: accepted
date: 2026-11-20
sources:
  - Cargo.toml
  - .mise/config.toml
  - .mise/tasks/web/build
  - .mise/tasks/web/serve
  - web/index.html
  - .github/workflows/pages.yml
  - src/main.rs
  - ../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md
  - 0003-run-project-tasks-with-mise.md
---

# 0005 — Build the same crate for the browser

## Context

The [Rivalen im Wald pitch](../../pitches/rivalen-im-wald/pitch--rivalen-im-wald.md)
asks for the first playable natively and in a desktop browser, and says the
browser build follows Car Drive's (`waldstatt-lab/carrrgh`). Car Drive builds its one
crate for `wasm32-unknown-unknown` with WebGL2, binds it with `wasm-bindgen`,
shrinks it with `wasm-opt`, and publishes a static page to GitHub Pages.

Alternatives considered:

- **A separate web crate or a bundler such as Trunk.** More moving parts for a
  page that only loads one module and a canvas.
- **WebGPU.** Not yet available in every desktop browser the game should reach;
  WebGL2 is, and the `2d` feature set already renders with it.
- **Car Drive's approach**, with the same tool versions, so both games are
  built and served the same way.

## Decision

The crate builds unchanged for `wasm32-unknown-unknown`; only the window
differs (`src/main.rs`): natively a 1280 × 800 window, in the browser the canvas
`#waldstatt-go` of [web/index.html](../../../web/index.html), sized by the page.
Bevy's `web` feature is enabled for that target only. A `web` Cargo profile
optimizes for size with fat LTO.

`mise run web:build` (ADR 0003's tasks) builds it, runs `wasm-bindgen` and
`wasm-opt`, and writes `dist/web/`; `mise run web:serve` serves it on
`127.0.0.1:8080`. `.mise/config.toml` pins the wasm target, `wasm-bindgen`
0.2.130 (it must match the crate in `Cargo.lock`), binaryen and miniserve.
[.github/workflows/pages.yml](../../../.github/workflows/pages.yml) runs the build
on every push to `main` and publishes `dist/web` to GitHub Pages.

The page loads nothing but itself and the game: the water pages and the fonts
are compiled in (ADR 0004), and nothing is saved.

## Consequences

- One code base and one set of tests for both targets; the rules are the same
  code, so a game from a seed plays the same in the browser.
- The browser build is slow to compile (fat LTO) and is checked only by its
  workflow and by hand; the native checks do not build it.
- Updating Bevy or `wasm-bindgen` means updating the pinned `wasm-bindgen` CLI
  in `.mise/config.toml` together.
- The browser keeps its own keys away from the canvas, so Tab and Space play.
  Touch input is not supported, as the pitch excludes phones.

## The tell

- `cargo clippy --locked --target wasm32-unknown-unknown -- -D warnings` passes.
- `mise run web:serve`, then a desktop browser at `http://127.0.0.1:8080` shows
  «Der Wald wird geladen …» and then the start menu; this needs a manual check.
- `rg -n "wasm-bindgen\"" .mise/config.toml` and the `wasm-bindgen` version in
  `Cargo.lock` agree.
