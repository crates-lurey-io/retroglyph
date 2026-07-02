//! Symmetric shadowcasting field of view.
//!
//! A Rust port of Albert Ford's symmetric shadowcasting algorithm
//! (<https://www.albertford.com/shadowcasting/>, CC0). It is symmetric (if A
//! sees B then B sees A), artifact-free, and fast. Slopes are kept as integer
//! rationals so there is no floating-point rounding error.
//!
//! ```ignore
//! fov::compute(origin, radius,
//!     |x, y| is_wall(x, y),        // blocks vision
//!     |x, y| mark_visible(x, y));  // in field of view
//! ```
#![allow(dead_code)]

use retroglyph::Pos;

/// A slope as an integer rational `num / den`, with `den > 0`.
#[derive(Clone, Copy)]
struct Slope {
    num: i64,
    den: i64,
}

impl Slope {
    const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }
}

/// One row of a quadrant, bounded by a start and end slope at a given depth.
#[derive(Clone, Copy)]
struct Row {
    depth: i64,
    start: Slope,
    end: Slope,
}

/// Compute field of view from `origin` out to `radius` (Euclidean).
///
/// `is_blocking(x, y)` returns `true` for tiles that block vision (treat
/// out-of-bounds as blocking). `mark_visible(x, y)` receives every tile in the
/// field of view, including the origin.
pub fn compute<B, V>(origin: Pos, radius: u16, mut is_blocking: B, mut mark_visible: V)
where
    B: FnMut(i32, i32) -> bool,
    V: FnMut(i32, i32),
{
    let ox = i32::from(origin.x);
    let oy = i32::from(origin.y);
    let r = i64::from(radius);
    mark_visible(ox, oy);

    for cardinal in 0..4u8 {
        // Map a quadrant-relative (depth, col) to absolute (x, y).
        let transform = |depth: i64, col: i64| -> (i32, i32) {
            #[allow(clippy::cast_possible_truncation)]
            let (d, c) = (depth as i32, col as i32);
            match cardinal {
                0 => (ox + c, oy - d), // north
                1 => (ox + d, oy + c), // east
                2 => (ox + c, oy + d), // south
                _ => (ox - d, oy + c), // west
            }
        };

        // Depth-first scan via an explicit stack (no recursion).
        let mut stack = vec![Row {
            depth: 1,
            start: Slope::new(-1, 1),
            end: Slope::new(1, 1),
        }];

        while let Some(row) = stack.pop() {
            if row.depth > r {
                continue;
            }
            let mut start = row.start;
            let min_col = round_ties_up(row.depth * start.num, start.den);
            let max_col = round_ties_down(row.depth * row.end.num, row.end.den);

            let mut prev_wall: Option<bool> = None;
            let mut col = min_col;
            while col <= max_col {
                let (x, y) = transform(row.depth, col);
                let wall = is_blocking(x, y);

                // Reveal walls unconditionally and floors that are symmetric.
                let symmetric = col * start.den >= row.depth * start.num
                    && col * row.end.den <= row.depth * row.end.num;
                let dist2 = row.depth * row.depth + col * col;
                if (wall || symmetric) && dist2 <= r * r {
                    mark_visible(x, y);
                }

                if let Some(prev) = prev_wall {
                    if prev && !wall {
                        // wall -> floor: tighten this row's start slope.
                        start = slope(row.depth, col);
                    }
                    if !prev && wall {
                        // floor -> wall: descend with a new end slope.
                        stack.push(Row {
                            depth: row.depth + 1,
                            start,
                            end: slope(row.depth, col),
                        });
                    }
                }
                prev_wall = Some(wall);
                col += 1;
            }

            // Row ended on open floor: continue straight down.
            if prev_wall == Some(false) {
                stack.push(Row {
                    depth: row.depth + 1,
                    start,
                    end: row.end,
                });
            }
        }
    }
}

/// Start/end slope tangent to the left edge of tile `(depth, col)`.
const fn slope(depth: i64, col: i64) -> Slope {
    Slope::new(2 * col - 1, 2 * depth)
}

/// `floor(a / den + 1/2)` for `den > 0`, via integer math.
const fn round_ties_up(a: i64, den: i64) -> i64 {
    div_floor(2 * a + den, 2 * den)
}

/// `ceil(a / den - 1/2)` for `den > 0`, via integer math.
const fn round_ties_down(a: i64, den: i64) -> i64 {
    div_ceil(2 * a - den, 2 * den)
}

/// Floor division for `b > 0`.
const fn div_floor(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b < 0 { q - 1 } else { q }
}

/// Ceiling division for `b > 0`.
const fn div_ceil(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b > 0 { q + 1 } else { q }
}
