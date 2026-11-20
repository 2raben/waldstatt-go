//! The boards, cut from the Waldstatt's water pages (ADR 0004).
//!
//! The pages in `data/waters/` are copied unchanged from the Waldstatt
//! (`2raben/waldstatt`), whose `tools/waters.mjs` generates them from
//! swisstopo's swissTLM3D and BAFU's stream orders. A page covers 200 × 200
//! tiles of 50 m; tile (0, 0) is the Klosterplatz Einsiedeln and north is −y.
//! Each page is rasterized into tiles as the Waldstatt's `world.ts` does:
//! lakes are filled even-odd, and every tile whose centre lies within half a
//! tile of a river's line (0.6 for wide rivers) is water, so even a brook is an
//! unbroken line that straight steps cannot slip through.

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
        height: 100,
    },
    BoardSpec {
        name: "Biberbrugg",
        description: "Wo die Alp in die Sihl fliesst",
        origin: (-95, -140),
        width: 100,
        height: 100,
    },
    BoardSpec {
        name: "Hütten",
        description: "Die Sihl und der Hüttnersee",
        origin: (-170, -170),
        width: 100,
        height: 100,
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

/// A rectangle of Waldstatt tiles to rasterize into.
#[derive(Clone, Copy)]
struct Clip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Clip {
    fn is_empty(self) -> bool {
        self.x0 > self.x1 || self.y0 > self.y1
    }
}

/// Builds a board from its spec and the compiled-in water pages.
pub fn build_board(spec: &BoardSpec) -> Result<Board, String> {
    let (ox, oy) = spec.origin;
    let (w, h) = (spec.width, spec.height);
    let mut ground = vec![Ground::Land; (w * h) as usize];
    let mut labels = Vec::new();
    let board_clip = Clip {
        x0: ox,
        y0: oy,
        x1: ox + w - 1,
        y1: oy + h - 1,
    };
    for py in oy.div_euclid(PAGE)..=(oy + h - 1).div_euclid(PAGE) {
        for px in ox.div_euclid(PAGE)..=(ox + w - 1).div_euclid(PAGE) {
            let Some((_, source)) = PAGES.iter().find(|(key, _)| *key == (px, py)) else {
                return Err(format!("Wasserseite {px},{py} fehlt"));
            };
            let page: Page = serde_json::from_str(source)
                .map_err(|error| format!("Wasserseite {px},{py}: {error}"))?;
            if page.land.is_some() {
                return Err(format!("Wasserseite {px},{py} reicht über die Grenze"));
            }
            // Each page fills only its own tiles, as in the Waldstatt.
            let clip = Clip {
                x0: board_clip.x0.max(px * PAGE),
                y0: board_clip.y0.max(py * PAGE),
                x1: board_clip.x1.min(px * PAGE + PAGE - 1),
                y1: board_clip.y1.min(py * PAGE + PAGE - 1),
            };
            if clip.is_empty() {
                continue;
            }
            let mut set = |x: i32, y: i32, value: Ground| {
                let i = ((y - oy) * w + (x - ox)) as usize;
                // A lake keeps its number where a river line runs into it.
                if !matches!(ground[i], Ground::Lake(_)) {
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
                fill_rings(&rings, clip, |x, y| set(x, y, Ground::Lake(lake.id)));
            }
            for line in &page.lines {
                let half = if line.wide { WIDE_RIVER } else { BARRIER };
                stroke_line(&line.points, half, clip, |x, y| set(x, y, Ground::River));
            }
            for label in &page.labels {
                let tile = (label.x.floor() as i32, label.y.floor() as i32);
                if (clip.x0..=clip.x1).contains(&tile.0) && (clip.y0..=clip.y1).contains(&tile.1) {
                    labels.push(WaterLabel {
                        name: label.name.clone(),
                        tile: Tile::new(tile.0 - ox, tile.1 - oy),
                        lake: label.lake,
                    });
                }
            }
        }
    }
    Ok(Board::new(spec.name, w, h, ground, labels))
}

/// Fills even-odd rings (flat lists x0, y0, x1, y1, …) into the clip: a tile
/// is inside when its centre is.
fn fill_rings(rings: &[&[f64]], clip: Clip, mut set: impl FnMut(i32, i32)) {
    let ys = || rings.iter().flat_map(|ring| ring.iter().skip(1).step_by(2));
    let Some(min_y) = ys().copied().reduce(f64::min) else {
        return;
    };
    let max_y = ys().copied().fold(min_y, f64::max);
    let top = clip.y0.max(min_y.floor() as i32);
    let bottom = clip.y1.min(max_y.ceil() as i32);
    let mut cuts = Vec::new();
    for ty in top..=bottom {
        let cy = f64::from(ty) + 0.5;
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
            let from = clip.x0.max((pair[0] - 0.5).ceil() as i32);
            let to = clip.x1.min((pair[1] - 0.5).floor() as i32);
            for tx in from..=to {
                set(tx, ty);
            }
        }
    }
}

/// Sets every tile of the clip whose centre lies within `half` of the line.
/// Distances are compared squared, so only exactly rounded arithmetic is used.
fn stroke_line(points: &[f64], half: f64, clip: Clip, mut set: impl FnMut(i32, i32)) {
    for segment in points.windows(4).step_by(2) {
        let [ax, ay, bx, by] = [segment[0], segment[1], segment[2], segment[3]];
        let length2 = match (bx - ax).powi(2) + (by - ay).powi(2) {
            0.0 => 1.0,
            l => l,
        };
        let fx = clip.x0.max((ax.min(bx) - half - 1.0).floor() as i32);
        let tx = clip.x1.min((ax.max(bx) + half).ceil() as i32);
        let fy = clip.y0.max((ay.min(by) - half - 1.0).floor() as i32);
        let ty = clip.y1.min((ay.max(by) + half).ceil() as i32);
        for y in fy..=ty {
            for x in fx..=tx {
                let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let t = (((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / length2).clamp(0.0, 1.0);
                let (dx, dy) = (px - (ax + t * (bx - ax)), py - (ay + t * (by - ay)));
                if dx * dx + dy * dy < half * half {
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

    #[test]
    fn a_line_leaves_no_gap_for_straight_steps() {
        // A brook running diagonally: every straight step across it meets water.
        let clip = Clip {
            x0: 0,
            y0: 0,
            x1: 9,
            y1: 9,
        };
        let mut ground = vec![Ground::Land; 100];
        stroke_line(&[0.0, 0.0, 10.0, 7.0], BARRIER, clip, |x, y| {
            ground[(y * 10 + x) as usize] = Ground::River;
        });
        let board = Board::new("Bach", 10, 10, ground, Vec::new());
        assert_ne!(
            board.region(Tile::new(9, 0)),
            board.region(Tile::new(0, 9)),
            "land on either side of the brook stays apart"
        );
    }

    #[test]
    fn fills_a_square_lake() {
        let clip = Clip {
            x0: 0,
            y0: 0,
            x1: 9,
            y1: 9,
        };
        let mut count = 0;
        let ring = [2.0, 2.0, 6.0, 2.0, 6.0, 5.0, 2.0, 5.0];
        fill_rings(&[&ring], clip, |_, _| count += 1);
        assert_eq!(count, 4 * 3);
    }
}
