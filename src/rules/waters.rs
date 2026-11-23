//! The boards, cut from the Waldstatt's water pages (ADR 0004).
//!
//! The pages in `data/waters/` are copied unchanged from the Waldstatt
//! (`waldstatt-lab/waldstatt`), whose `tools/waters.mjs` generates them from
//! swisstopo's swissTLM3D and BAFU's stream orders. A page covers 200 × 200
//! tiles of 50 m; tile (0, 0) is the Klosterplatz Einsiedeln and north is −y.
//! Each page is rasterized into the board's hexes much as the Waldstatt's
//! `world.ts` rasterizes it into squares: lakes are filled even-odd, and every
//! hex whose centre lies within half a step of a river's line (0.6 for wide
//! rivers) is water, so even a brook is an unbroken line that no step can
//! slip through (ADR 0008).

use bevy::prelude::Vec2;
use serde::Deserialize;

use super::board::{Board, Ground, Tile, WaterLabel};

/// Tiles per page side.
const PAGE: i32 = 200;

/// Half widths in tiles: wide rivers, and the least any river or brook has.
const WIDE_RIVER: f64 = 0.6;
const BARRIER: f64 = 0.5;

/// The pages the boards need, compiled in, by page coordinates.
const PAGES: &[((i32, i32), &str)] = &[
    ((-1, -1), include_str!("../../data/waters/-1_-1.json")),
    ((0, -1), include_str!("../../data/waters/0_-1.json")),
    ((-1, 0), include_str!("../../data/waters/-1_0.json")),
    ((0, 0), include_str!("../../data/waters/0_0.json")),
];

/// A board to choose: a window onto the Waldstatt's map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// The Waldstatt tile of the board's north-west corner.
    pub origin: (i32, i32),
    /// Hexes per row, and rows; rows lie √3 / 2 of a step apart, so 115 rows
    /// cover about as much ground as 100 columns.
    pub width: i32,
    pub height: i32,
}

/// The first boards round Einsiedeln. They leave out the Sihlsee, which the
/// water data knows only as a reservoir's basin.
pub const BOARDS: &[BoardSpec] = &[
    BoardSpec {
        name: "Einsiedeln",
        description: "Das Klosterdorf und die Alp",
        origin: (-92, -50),
        width: 100,
        height: 115,
    },
    BoardSpec {
        name: "Biberbrugg",
        description: "Wo die Alp in die Sihl fliesst",
        origin: (-95, -140),
        width: 100,
        height: 115,
    },
    BoardSpec {
        name: "Hütten",
        description: "Die Sihl und der Hüttnersee",
        origin: (-170, -170),
        width: 100,
        height: 115,
    },
];

#[derive(Deserialize)]
struct Page {
    lines: Vec<RiverLine>,
    lakes: Vec<Lake>,
    /// Switzerland's area where the border crosses the page; the boards lie
    /// wholly inside, so this is always absent for them.
    land: Option<Vec<Vec<f64>>>,
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct RiverLine {
    wide: bool,
    points: Vec<f64>,
}

#[derive(Deserialize)]
struct Lake {
    id: u16,
    rings: Vec<Vec<f64>>,
    islands: Vec<Vec<f64>>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
    x: f64,
    y: f64,
    lake: bool,
}

/// The hexes of a board on the Waldstatt's map, whose units are its tiles of
/// 50 m: one step between hexes is one unit.
#[derive(Clone, Copy)]
struct Grid {
    /// The Waldstatt point of the board's north-west corner.
    ox: f64,
    oy: f64,
    width: i32,
    height: i32,
}

/// Rows lie √3 / 2 apart; a hex reaches 1 / √3 from its centre to its corners.
const ROW: f64 = 0.866_025_403_784_438_6;
const RADIUS: f64 = 0.577_350_269_189_625_8;

impl Grid {
    /// How far east of `ox` the centres of a row start: odd rows are shifted.
    fn shift(y: i32) -> f64 {
        0.5 + 0.5 * f64::from(y & 1)
    }

    /// A hex's centre on the map, as [`Tile::centre`] puts it on the board.
    fn centre(self, x: i32, y: i32) -> (f64, f64) {
        (
            self.ox + f64::from(x) + Grid::shift(y),
            self.oy + f64::from(y) * ROW + RADIUS,
        )
    }

    /// The rows whose centres lie between `y0` and `y1` on the map.
    fn rows(self, y0: f64, y1: f64) -> std::ops::RangeInclusive<i32> {
        let top = ((y0 - self.oy - RADIUS) / ROW).ceil() as i32;
        let bottom = ((y1 - self.oy - RADIUS) / ROW).floor() as i32;
        top.max(0)..=bottom.min(self.height - 1)
    }

    /// The columns of row `y` whose centres lie between `x0` and `x1`.
    fn columns(self, y: i32, x0: f64, x1: f64) -> std::ops::RangeInclusive<i32> {
        let from = (x0 - self.ox - Grid::shift(y)).ceil() as i32;
        let to = (x1 - self.ox - Grid::shift(y)).floor() as i32;
        from.max(0)..=to.min(self.width - 1)
    }
}

/// Builds a board from its spec and the compiled-in water pages, in hexes
/// (ADR 0008): a hex is a lake where its centre lies inside one, and a river
/// where its centre lies within half a step of a river's line.
pub fn build_board(spec: &BoardSpec) -> Result<Board, String> {
    let (w, h) = (spec.width, spec.height);
    let grid = Grid {
        ox: f64::from(spec.origin.0),
        oy: f64::from(spec.origin.1),
        width: w,
        height: h,
    };
    let mut ground = vec![Ground::Land; (w * h) as usize];
    let mut labels = Vec::new();
    // The map's area the board covers, to find its pages.
    let (east, south) = grid.centre(w, h - 1);
    let (x0, y0) = (spec.origin.0, spec.origin.1);
    let (x1, y1) = (east.ceil() as i32, (south + RADIUS).ceil() as i32);
    for py in y0.div_euclid(PAGE)..=y1.div_euclid(PAGE) {
        for px in x0.div_euclid(PAGE)..=x1.div_euclid(PAGE) {
            let Some((_, source)) = PAGES.iter().find(|(key, _)| *key == (px, py)) else {
                return Err(format!("Wasserseite {px},{py} fehlt"));
            };
            let page: Page = serde_json::from_str(source)
                .map_err(|error| format!("Wasserseite {px},{py}: {error}"))?;
            if page.land.is_some() {
                return Err(format!("Wasserseite {px},{py} reicht über die Grenze"));
            }
            // Each page fills only the hexes whose centres lie on it, as in the
            // Waldstatt each page fills only its own tiles.
            let on_page = |x: i32, y: i32| {
                let (cx, cy) = grid.centre(x, y);
                (
                    cx.div_euclid(f64::from(PAGE)) as i32,
                    cy.div_euclid(f64::from(PAGE)) as i32,
                ) == (px, py)
            };
            let mut set = |x: i32, y: i32, value: Ground| {
                let i = (y * w + x) as usize;
                // A lake keeps its number where a river line runs into it.
                if on_page(x, y) && !matches!(ground[i], Ground::Lake(_)) {
                    ground[i] = value;
                }
            };
            for lake in &page.lakes {
                let rings: Vec<&[f64]> = lake
                    .rings
                    .iter()
                    .chain(&lake.islands)
                    .map(Vec::as_slice)
                    .collect();
                fill_rings(&rings, grid, |x, y| set(x, y, Ground::Lake(lake.id)));
            }
            for line in &page.lines {
                let half = if line.wide { WIDE_RIVER } else { BARRIER };
                stroke_line(&line.points, half, grid, |x, y| set(x, y, Ground::River));
            }
            for label in &page.labels {
                let at = Vec2::new((label.x - grid.ox) as f32, (label.y - grid.oy) as f32);
                let tile = Tile::at(at);
                if (0..w).contains(&tile.x) && (0..h).contains(&tile.y) && on_page(tile.x, tile.y) {
                    labels.push(WaterLabel {
                        name: label.name.clone(),
                        tile,
                        lake: label.lake,
                    });
                }
            }
        }
    }
    Ok(Board::new(spec.name, w, h, ground, labels))
}

/// Fills even-odd rings (flat lists x0, y0, x1, y1, …) into the grid: a hex
/// is inside when its centre is.
fn fill_rings(rings: &[&[f64]], grid: Grid, mut set: impl FnMut(i32, i32)) {
    let ys = || rings.iter().flat_map(|ring| ring.iter().skip(1).step_by(2));
    let Some(min_y) = ys().copied().reduce(f64::min) else {
        return;
    };
    let max_y = ys().copied().fold(min_y, f64::max);
    let mut cuts = Vec::new();
    for ty in grid.rows(min_y, max_y) {
        let (_, cy) = grid.centre(0, ty);
        cuts.clear();
        for ring in rings {
            let n = ring.len() / 2;
            let mut j = n.wrapping_sub(1);
            for i in 0..n {
                let (xi, yi) = (ring[i * 2], ring[i * 2 + 1]);
                let (xj, yj) = (ring[j * 2], ring[j * 2 + 1]);
                j = i;
                if (yi > cy) == (yj > cy) {
                    continue;
                }
                cuts.push(xi + (cy - yi) * (xj - xi) / (yj - yi));
            }
        }
        cuts.sort_by(f64::total_cmp);
        for pair in cuts.chunks_exact(2) {
            for tx in grid.columns(ty, pair[0], pair[1]) {
                set(tx, ty);
            }
        }
    }
}

/// Sets every hex whose centre lies within `half` of the line. Two
/// neighbouring hexes on either side of a line are one step apart, so with
/// `half` at least half a step one of them is always set: a line leaves no
/// gap. Distances are compared squared, so only exactly rounded arithmetic is
/// used.
fn stroke_line(points: &[f64], half: f64, grid: Grid, mut set: impl FnMut(i32, i32)) {
    for segment in points.windows(4).step_by(2) {
        let [ax, ay, bx, by] = [segment[0], segment[1], segment[2], segment[3]];
        let length2 = match (bx - ax).powi(2) + (by - ay).powi(2) {
            0.0 => 1.0,
            l => l,
        };
        for y in grid.rows(ay.min(by) - half, ay.max(by) + half) {
            for x in grid.columns(y, ax.min(bx) - half, ax.max(bx) + half) {
                let (px, py) = grid.centre(x, y);
                let t = (((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / length2).clamp(0.0, 1.0);
                let (dx, dy) = (px - (ax + t * (bx - ax)), py - (ay + t * (by - ay)));
                if dx * dx + dy * dy <= half * half {
                    set(x, y);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_board_builds_from_the_pages() {
        for spec in BOARDS {
            let board = build_board(spec).expect("board builds");
            let water = board.tiles().filter(|&t| board.is_water(t)).count();
            assert!(water > 100, "{} has its rivers", spec.name);
            assert!(water < board.len() / 5, "{} is mostly forest", spec.name);
            let main = board.largest_region();
            assert!(
                board.region_size(main) > board.len() / 3,
                "{} has room to play",
                spec.name
            );
        }
    }

    #[test]
    fn huetten_has_the_hüttnersee() {
        let board = build_board(&BOARDS[2]).expect("board builds");
        let lake = board
            .tiles()
            .filter(|&t| matches!(board.ground(t), Ground::Lake(_)))
            .count();
        assert!(lake > 50, "the Hüttnersee is a lake of {lake} tiles");
        assert!(
            board
                .labels
                .iter()
                .any(|l| l.name == "Hüttnersee" && l.lake)
        );
    }

    fn grid(width: i32, height: i32) -> Grid {
        Grid {
            ox: 0.0,
            oy: 0.0,
            width,
            height,
        }
    }

    #[test]
    fn hex_centres_agree_with_the_board() {
        let grid = grid(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let (cx, cy) = grid.centre(x, y);
                let c = Tile::new(x, y).centre();
                assert!((cx as f32 - c.x).abs() < 1e-5 && (cy as f32 - c.y).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn a_line_leaves_no_gap() {
        // Brooks running every way across the board: no land on one side is
        // joined to land on the other.
        for points in [
            [0.0, 0.0, 10.0, 7.0],
            [0.0, 4.0, 10.5, 4.3],
            [5.2, 0.0, 4.6, 9.0],
            [0.0, 8.0, 10.5, 0.5],
        ] {
            let mut ground = vec![Ground::Land; 100];
            stroke_line(&points, BARRIER, grid(10, 10), |x, y| {
                ground[(y * 10 + x) as usize] = Ground::River;
            });
            let board = Board::new("Bach", 10, 10, ground, Vec::new());
            let side = |t: Tile| {
                let (c, [ax, ay, bx, by]) = (t.centre(), points.map(|v| v as f32));
                (bx - ax) * (c.y - ay) - (by - ay) * (c.x - ax) > 0.0
            };
            for a in board.tiles().filter(|&t| !board.is_water(t)) {
                for b in board.steps(a).filter(|&t| !board.is_water(t)) {
                    assert_eq!(side(a), side(b), "{points:?}: {a:?} and {b:?}");
                }
            }
        }
    }

    #[test]
    fn no_step_on_a_board_crosses_a_river() {
        for spec in BOARDS {
            let board = build_board(spec).expect("board builds");
            let (ox, oy) = (spec.origin.0 as f32, spec.origin.1 as f32);
            let mut lines = Vec::new();
            for (_, source) in PAGES {
                let page: Page = serde_json::from_str(source).expect("page parses");
                for line in page.lines {
                    let p: Vec<Vec2> = line
                        .points
                        .chunks_exact(2)
                        .map(|xy| Vec2::new(xy[0] as f32 - ox, xy[1] as f32 - oy))
                        .collect();
                    lines.extend(p.windows(2).map(|ab| (ab[0], ab[1])));
                }
            }
            let crosses = |a: Vec2, b: Vec2, (c, d): (Vec2, Vec2)| {
                let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
                side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
            };
            for a in board.tiles().filter(|&t| !board.is_water(t)) {
                for b in board.steps(a).filter(|&t| t > a && !board.is_water(t)) {
                    let (ca, cb) = (a.centre(), b.centre());
                    assert!(
                        !lines.iter().any(|&l| crosses(ca, cb, l)),
                        "{}: the step from {a:?} to {b:?} crosses a river",
                        spec.name
                    );
                }
            }
        }
    }

    #[test]
    fn fills_a_lake_by_centres() {
        let mut count = 0;
        // Rows 1 to 3 have their centres between 1.2 and 3.4. Between 2 and 6
        // lie four centres of the even row and five of each odd one, which
        // is shifted half a step east.
        let ring = [2.0, 1.2, 6.0, 1.2, 6.0, 3.4, 2.0, 3.4];
        fill_rings(&[&ring], grid(10, 10), |_, _| count += 1);
        assert_eq!(count, 5 + 4 + 5);
    }
}
