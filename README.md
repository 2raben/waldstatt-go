# Waldstatt Go

Rivals fell their way through the forest round Einsiedeln and win each other's
Waldleute by enclosing them, as in Go. The board is a piece of the real map, with
the waters of the [Waldstatt](https://2raben.github.io/waldstatt/); see the
[game idea](docs/idea.md). Built with [Bevy](https://bevy.org) 0.20 for native
apps and the browser.

The first playable implements the
[Rivalen im Wald pitch](docs/pitches/rivalen-im-wald/pitch--rivalen-im-wald.md).
The repository was created from the Bevy template (`2raben/template-bevy`), whose
documentation structure, agent skills and [mise](https://mise.jdx.dev) tasks it
keeps. All game text is German with Swiss spelling.

## What's inside

| Path | Purpose |
| --- | --- |
| `src/rules/` | The rules, headless: board, groups, lines, trails, enclosing, water, sight, the game |
| `src/ai.rs` | Computer rivals, playing through the same commands as the player |
| `src/` (others) | Presentation: views, ground, sprites painted in code, trees and Waldleute, markers, input, camera, menus, HUD |
| `data/waters/` | The Waldstatt's water pages the boards are cut from, compiled in |
| `assets/fonts/` | Fira Sans (SIL Open Font License), compiled in |
| `web/` | The page of the browser build |
| `docs/idea.md` | Game direction and longer-term possibilities |
| `docs/pitches/` | Shaped feature proposals (Problem, Solution, Security, Rabbit Holes, No-Gos) |
| `docs/architecture/` | Current structure and conventions, and [decision records](docs/architecture/decisions/README.md) |
| `.agents/skills/` | Bevy 0.20 skills for coding agents; versions in `skills-lock.json` |
| `shell.nix` | Native build and runtime libraries for NixOS (`nix-shell`) |
| `.mise/` | Pinned toolchain and `build`, `test`, `run`, `check`, `fmt`, `web:build`, `web:serve`, `agents:init-claude` tasks |
| `AGENTS.md`, `CONTRIBUTING.md` | Working brief for agents; workflow and validation for everyone |

## How to play

Choose a board (Einsiedeln, Biberbrugg or Hütten), how many rivals sit at the
board (two to four, you included; the others are computer rivals) and how long the
game lasts. Every rival starts with five Waldleute, each a group of one, scattered
at random over the forest.

- **Cut.** Press on any of your groups (a Waldmensch, its badge or its clearing) and
  drag a line over the forest, or click where the selected group should go and it
  finds the fastest way. A group of one goes ten tiles, two twenty, four thirty:
  `(log2(size) + 1) × 10`, one tile per step to a neighbouring hex. Where a line
  is blocked, it finds another way. The one you pressed (or the one nearest the
  clearing you pressed) leads: it cuts tile by tile and earns the trees, faster
  the bigger its group; the others follow over the clearing, never more than two
  tiles apart. Click the selected group's own trail to close a ring. Its lead
  cuts across the forest towards the field you clicked. If you picked the second
  field from the tail end, it keeps aiming for the second field as the tail moves.
- **Gather.** When the line is done, the Waldleute gather as near to its end as
  they can, one to a tile, cutting where they must; water, the edge, other rivals'
  margins and your other clearings stop them.
- **Trails grow back.** While a group cuts, only the last tiles of its trail, as
  many as it may draw, stay open; once it has stood still for eight seconds, the
  trail grows back tile by tile until only its gathering is left. A sapling marks
  the next.
- **Margins.** Nobody can cut next to another rival's open tile (red-tinged forest),
  so a ring that is closing traps a group before it closes.
- **Enclose.** Your open tiles, the water and the board's edge split the board; a
  ring round another rival's clearing takes its Waldleute, its trees and its
  clearing into the group that closed it. The board is cut in hexes, which meet
  side to side, so a ring holds wherever it closes. Waldleute in a river, or
  already across it, stay free; a ring round a lake takes the boat on it.
- **Water.** Wading through a river takes ten times as long as cutting and costs
  trees; a lake needs a ship (twenty times as long, trees, at most ten Waldleute,
  who all go aboard).
- **Grow.** Every 40 seconds each rival gets a new Waldmensch, who turns up as a
  group of one somewhere at random in the forest, away from everyone and outside
  other rivals' rings. Groups merge when a cut joins their clearings, or a lead walks
  into the other's: double-click another group to send the selected one there.
  Right-click a group (or press T) to split it, and the half furthest back goes its
  own way; drag with the right button to send the half nearest the Waldmensch you
  pressed along a line instead. A group hemmed in by other rivals, water or the
  board's edge, with fewer than 120 tiles it could reach, cannot split.
- **Win.** When the time is up or only one rival is left, rivals rank by Waldleute,
  then trees.

| Action | Input |
| --- | --- |
| Select a group | Click on a Waldmensch, its group's badge, or its clearing |
| Draw a line from one of its Waldleute | Left mouse button on the group, drag, release |
| Send the selected group the fastest way | Click beside it |
| Close a ring with the selected group | Click its own trail, away from its Waldleute |
| Draw from the selected group's lead | Left mouse button beside it, drag |
| Send the selected group to another one, to merge | Double-click the other group |
| Split a group | Right-click it, or T for the selected group |
| Split a group and send one half along a line | Right mouse button on the group, drag, release |
| Stop the selected group | Right mouse button beside it, or X |
| Next / previous group | Tab / Shift+Tab |
| Switch top-down / isometric view | V |
| Centre on the selected group | Space |
| Move the view / zoom | WASD, arrows, middle mouse button / mouse wheel, + and − |
| Pause, all keys | Esc |

On a touch screen, a tap is a click, holding a finger still for half a second is
the right mouse button, and two fingers move and zoom the view.

## Requirements

- [mise](https://mise.jdx.dev), recommended: `mise install` provides Rust 1.97.1
  with rustfmt, Clippy and the `wasm32-unknown-unknown` target, plus
  `wasm-bindgen`, `wasm-opt` and `miniserve` for the browser build, as pinned in
  [.mise/config.toml](.mise/config.toml).
  Without mise, install Rust 1.97.1 or newer (edition 2024) with rustup.
- Bevy 0.20.1, downloaded automatically by Cargo. `Cargo.lock` pins dependencies.
- Clang and LLD on x86_64 Linux, used by [.cargo/config.toml](.cargo/config.toml)
  for faster linking. Remove that file to use the default linker instead.
- A working Vulkan (Linux, Windows) or Metal (macOS) graphics driver.

### Linux native libraries

Bevy needs ALSA, udev, X11 and Wayland libraries, and pkg-config. On Arch Linux:

```sh
sudo pacman -S --needed base-devel pkgconf alsa-lib systemd \
  libx11 libxcursor libxrandr libxi libxkbcommon wayland clang lld \
  vulkan-icd-loader
```

On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libasound2-dev libudev-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev \
  libwayland-dev clang lld
```

On NixOS, adding these packages to your profile usually does not make them
visible to builds or the dynamic loader. If your system configuration does not
expose them to Cargo builds, use the project's [shell.nix](shell.nix); it provides
pkg-config, Clang, LLD, and the libraries, and puts the runtime libraries on
`LD_LIBRARY_PATH`. The Vulkan driver comes from the system
(`hardware.graphics.enable = true`):

```sh
nix-shell          # then: mise run run
nix-shell --run 'mise run check'
```

`nix-shell` is not entered automatically when you open the folder. To load it on
`cd`, install [direnv](https://direnv.net) with
[nix-direnv](https://github.com/nix-community/nix-direnv), add a `.envrc` containing
`use nix`, and run `direnv allow` once.

Install a Vulkan driver for your GPU on every distribution. If audio is routed through PipeWire, keep its ALSA plugin
(`pipewire-alsa`) installed.

See Bevy's [Linux dependency instructions](https://github.com/bevyengine/bevy/blob/latest/docs/linux_dependencies.md)
for other distributions and its [setup guide](https://bevy.org/learn/quick-start/getting-started/setup/).

## Development

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution workflow and validation,
the [architecture documentation](docs/architecture/README.md) for current structure
and conventions, and the [ADRs](docs/architecture/decisions/README.md) for recorded
architectural choices. AI coding agents should start with [AGENTS.md](AGENTS.md).

```sh
mise install     # once: install the pinned toolchain
mise run run     # build and start the game
mise run test    # headless tests
mise run build   # debug build; `mise run build --release` for release
mise run check   # format, Clippy, build, and tests before a PR
mise run web:build  # browser version into dist/web
mise run web:serve  # build it and serve it on http://127.0.0.1:8080
mise run arena   # computer rivals play each other headless; see --help
mise tasks       # list every task
```

Each task but `arena` passes extra arguments to Cargo, for example `mise run test ring`;
`arena` passes them to the arena. Computer rivals start hunting once their largest
group has three Waldleute, while the other groups move to join it. For example,
`mise run arena --games 12 rally_size=0` plays the distributed opening against
the default on the same sequence of boards and seeds. The
tasks are plain scripts in [.mise/tasks/](.mise/tasks/), so the equivalent Cargo
commands also work:

```sh
cargo run --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked
cargo test --locked
```

The first build downloads and compiles Bevy and can take several minutes.
Dependencies are optimized in development builds for usable runtime performance.

The browser version is the same crate built for `wasm32-unknown-unknown` with
WebGL2 and a size-optimized `web` profile; [web/index.html](web/index.html) is its
page ([ADR 0005](docs/architecture/decisions/0005-build-the-same-crate-for-the-browser.md)).
The [Pages workflow](.github/workflows/pages.yml) publishes it from `main`; Pages
must use "GitHub Actions" as its source in the repository settings.

The boards' waters are the Waldstatt's, from swissTLM3D (© swisstopo) and BAFU's
stream orders (© BAFU); see [data/waters/](data/waters/README.md).

## Agent skills

`.agents/skills/` holds Bevy 0.20 skills from
[chrisgliddon/bevy-skills](https://github.com/chrisgliddon/bevy-skills): ECS, input,
cameras, UI, assets, audio, animation, physics, rendering, testing, WebAssembly,
and more, plus `similarity-rs` for finding duplicated Rust code. Start with the
`bevy` router skill. [skills-lock.json](skills-lock.json) records each skill's
source and content hash; update them together.

Codex and other agents that follow the `AGENTS.md` convention read these files
directly. Claude Code reads `CLAUDE.md` and `.claude/skills/` instead; link them to
the shared files once per checkout:

```sh
mise run agents:init-claude
```

This creates `CLAUDE.md -> AGENTS.md` and `.claude/skills -> ../.agents/skills`.
Rerunning it is safe, and it never replaces an existing file or a link that points
elsewhere.
