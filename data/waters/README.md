# Water pages

The boards' rivers and lakes, copied unchanged from the
[Waldstatt](https://waldstatt-lab.ch/waldstatt/) (`waldstatt-lab/waldstatt`) and
compiled into the game by [src/rules/waters.rs](../../src/rules/waters.rs)
([ADR 0004](../../docs/architecture/decisions/0004-compile-the-boards-from-copied-waldstatt-water-pages.md)).

| File | Waldstatt tiles |
| --- | --- |
| `-1_-1.json` | x −200 to −1, y −200 to −1 |
| `0_-1.json` | x 0 to 199, y −200 to −1 |
| `-1_0.json` | x −200 to −1, y 0 to 199 |
| `0_0.json` | x 0 to 199, y 0 to 199 |

A page covers 200 × 200 tiles of 50 m; tile (0, 0) is the Klosterplatz
Einsiedeln and north is −y. Each file is one entry of the Waldstatt's
`src/waldstatt/data/water-pages.json`, the same as its published
`/waters/<px>_<py>.json`.

- Copied from `waldstatt-lab/waldstatt` at commit
  `f48fd4e9f04512903c92e87e96654e4b88bb97fe` (2026-11-19).
- The data were last generated there in commit
  `5e068fa9736ac4c2f230cdc5341d635bdcc2d908` (2026-11-18, "feat: choose rivers
  by BAFU's Strahler stream order"), by `src/waldstatt/tools/waters.mjs`.

Sources, credited in the game: swissTLM3D and swissBOUNDARIES3D © swisstopo;
«Flussordnungszahl nach Strahler» © BAFU. Do not edit these files by hand.
To take newer data, copy the pages again from the Waldstatt and update the
commits above. A board that needs another page must add it here and to the
`PAGES` list in `waters.rs`.
