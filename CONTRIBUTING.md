# Contributing

Keep changes focused and explain the problem, resulting behavior, and validation
in the pull request. Start with the [architecture documentation](docs/architecture/README.md)
and [architectural decision records](docs/architecture/decisions/README.md).
AI coding agents also follow [AGENTS.md](AGENTS.md).

## Shaping work

Shape substantial features before implementation: describe the problem, rough
solution, risks, and excluded work. Small fixes and documentation corrections do
not need a pitch. Bound each pitch by a useful playable experiment; add an appetite
or deadline only if your project wants one.

Save pitches as `docs/pitches/<topic>/pitch--<slug>.md` using the
[pitch template](docs/pitches/README.md#template), and add them to the pitch index.
The [Rivalen im Wald pitch](docs/pitches/rivalen-im-wald/pitch--rivalen-im-wald.md)
is a worked example of the sections: Problem, Solution, Security, Rabbit Holes, and
No-Gos. Explain relevant security concerns or why none apply. Keep Excalidraw
breadboards and rough sketches beside the pitch and link them from Solution when
they help explain interactions or layout.

Pitches describe intent. Architecture documentation describes implemented behavior
and conventions; ADRs preserve significant choices and tradeoffs. Update the
relevant documents with implementation changes. Do not treat the broader
[game idea](docs/idea.md) as a commitment to implement every direction it lists.

## Branches and commits

Use short-lived branches and reviewed pull requests. The default branch is `main`;
use the intended integration branch as the PR base. Prefer `feature/<slug>` or
`bugfix/<slug>`, including an issue ID when useful. Link the relevant issue or
pitch and use a draft PR for early feedback.

Use Conventional Commits: `<type>[optional scope][!]: <description>`. Common types
are `feat`, `fix`, `docs`, `refactor`, `test`, `build`, `ci`, and `chore`. Use a concise
description and separate optional bodies and footers with blank lines. Mark
breaking changes with `!` or a `BREAKING CHANGE:` footer and explain migration
requirements. Squash commit messages follow the same convention.

```text
feat(player): add a wall jump
fix(run): keep collected coins after falling out
docs: describe the level map format
```

## Setup and validation

Follow the [README requirements](README.md#requirements) for Rust and native
dependencies. `Cargo.toml` declares the minimum Rust version and Bevy features;
`Cargo.lock` pins dependency resolution; [.mise/config.toml](.mise/config.toml)
pins the toolchain. On x86_64 Linux, [.cargo/config.toml](.cargo/config.toml)
selects Clang and LLD; on NixOS, run the checks inside `nix-shell`
([shell.nix](shell.nix)). Keep dependency or toolchain changes deliberate, change the
Rust version in `Cargo.toml` and `.mise/config.toml` together, and explain such
changes in the PR.

Run from the repository root for code changes:

```sh
mise run check
```

which runs:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo test --locked
```

There is no CI workflow for pull requests. Tests include unit tests beside each
module and headless game tests in `src/tests.rs`, which step the real plugins at
the fixed timestep and play through commands. Prefer focused tests of game rules and ECS systems that can run
without a window. Include regression coverage for meaningful bugs; avoid tests that
merely duplicate implementation details. Run a subset with `mise run test <name>`.

For rendering, input, or gameplay changes, also run:

```sh
mise run run
```

Check the affected behavior manually in a game:

- The start menu leads to the setup; each board, two to four rivals, and each
  length can be chosen, and «Spiel beginnen» starts on the chosen board.
- Pressing on one of your groups selects it; dragging draws a line of straight
  steps that follows the pointer, no longer than the group's limit, and moving
  back takes it back. Releasing sends the group; a click beside the group draws
  from the selected group. Right click or X stops it.
- The group cuts tile by tile; its trail stays open behind it and the sapling
  marks the tile that grows back next. A group of four cuts visibly faster.
- A line onto another of your groups merges them; T splits the selected group.
- Forest next to another rival's trail is tinged red and cannot be cut; a line
  into it stops short.
- V switches between top-down and isometric at any time, even while a group is
  cutting: every group stays on its tile, its line unchanged, and picking tiles
  works the same in both.
- Black is unexplored, fog keeps the waters; other rivals show only near your
  groups. Wading into a river and building a ship on a lake (Hütten) cost trees.
- A ring closing round a group flashes the enclosed part and the ring, and the
  news says who enclosed whom; a trapped group of yours shows a red ring and the
  warning. Esc pauses; the game ends with the ranking when time is up.

For browser-only changes, also run
`cargo clippy --locked --target wasm32-unknown-unknown -- -D warnings` and
`mise run web:serve`, and check the same in a desktop browser at
<http://127.0.0.1:8080>. The [Pages workflow](.github/workflows/pages.yml) builds
and publishes the browser version from `main`.

Record what was checked; if no graphical environment is available, report that
limitation.

For documentation-only changes, verify relative links and claims against source,
then run `git diff --check`; a full Rust build is unnecessary. Report checks
actually run, failures, and skipped coverage in the PR. Avoid unrelated formatting
and changes to generated `target/` contents.

## Architecture decisions

Use the [ADR workflow and index](docs/architecture/decisions/README.md) for lasting
choices such as physics integration, input handling, persisted formats, or
simulation scheduling. Copy the [template](docs/architecture/decisions/_adr-template.md),
explain the alternatives and consequences, and identify how reviewers can detect
drift. Routine implementation details do not need an ADR.
