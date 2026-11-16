//! Textured triangles onto a `Pixmap`: the mouth, which the package describes as a mesh.
//!
//! The corners snap to 1/16 of a pixel and the edge tests are exact integers, so two triangles
//! that share an edge cover each pixel on it once between them (a pixel exactly on an edge
//! belongs to the triangle whose edge runs one way): a feathered part drawn at less than full
//! opacity shows no seam and no double-drawn line.

use super::pixmap::Pixmap;

/// Corner coordinates are kept in this many parts of a pixel.
const SUBPIXEL: i64 = 16;
/// A corner farther than this from the target is taken to be on this line: the edge tests
/// multiply two differences of coordinates and must not overflow.
const FAR: f64 = 100_000.0;
/// The opacity of a texel, as a whole, in the 256ths `Pixmap::lay_over` takes.
const WHOLE: f32 = 256.0;

/// One corner: where it lands (pixels of the target), where it reads (0 to 1 across the source) and
/// how opaque it is, 0 to 1.
#[derive(Debug, Clone, Copy)]
pub(super) struct Corner {
    pub(super) at: [f32; 2],
    pub(super) texel: [f32; 2],
    pub(super) alpha: f32,
}

#[derive(Clone, Copy)]
struct Snapped {
    x: i64,
    y: i64,
}

fn snap(corner: &Corner) -> Snapped {
    let place = |v: f32| (f64::from(v).clamp(-FAR, FAR) * SUBPIXEL as f64).round() as i64;
    Snapped {
        x: place(corner.at[0]),
        y: place(corner.at[1]),
    }
}

/// Twice the signed area of (`a`, `b`, `p`): positive when `p` is on the inner side of the edge.
fn edge(a: Snapped, b: Snapped, p: Snapped) -> i64 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Whether a pixel exactly on the edge `a` to `b` is this triangle's. The same edge walked the
/// other way (by the neighbour) answers the opposite.
fn owns_edge(a: Snapped, b: Snapped) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    dy > 0 || (dy == 0 && dx > 0)
}

/// Draws the triangle over `target`, reading `source`, every texel taken at `opacity` (0 to 1)
/// times the interpolated corner opacity.
pub(super) fn fill(target: &mut Pixmap, source: &Pixmap, corners: [Corner; 3], opacity: f32) {
    // A corner that is not a place draws nothing.
    if corners.iter().any(|c| c.at.iter().any(|v| !v.is_finite())) {
        return;
    }
    let mut corners = corners;
    let mut p = corners.map(|c| snap(&c));
    let mut area = edge(p[0], p[1], p[2]);
    if area == 0 {
        return;
    }
    if area < 0 {
        corners.swap(1, 2);
        p.swap(1, 2);
        area = -area;
    }
    // The pixels whose centres can be inside: the box of the corners, in whole pixels.
    let (min_x, max_x) = bounds(p.map(|s| s.x), target.width());
    let (min_y, max_y) = bounds(p.map(|s| s.y), target.height());
    let owns = [
        owns_edge(p[1], p[2]),
        owns_edge(p[2], p[0]),
        owns_edge(p[0], p[1]),
    ];
    // What each corner reads, as a weight of its edge function: the attribute at a pixel is the
    // sum over the three edges of the edge's value there times its corner's attribute, over the
    // area. The edge values change by a constant from one pixel to the next along a row.
    let inverse = 1.0 / area as f32;
    let [w, h] = [source.width() as f32, source.height() as f32];
    let attribute = |f: &dyn Fn(&Corner) -> f32| {
        [
            f(&corners[0]) * inverse,
            f(&corners[1]) * inverse,
            f(&corners[2]) * inverse,
        ]
    };
    let (us, vs, alphas) = (
        attribute(&|c| c.texel[0] * w),
        attribute(&|c| c.texel[1] * h),
        attribute(&|c| c.alpha * opacity * WHOLE),
    );
    let step = [
        -(p[2].y - p[1].y) * SUBPIXEL,
        -(p[0].y - p[2].y) * SUBPIXEL,
        -(p[1].y - p[0].y) * SUBPIXEL,
    ]
    .map(|s| s as f32);
    let dot = |e: &[f32; 3], k: &[f32; 3]| e[0] * k[0] + e[1] * k[1] + e[2] * k[2];
    for y in min_y..=max_y {
        let centre_y = i64::from(y) * SUBPIXEL + SUBPIXEL / 2;
        let Some((first, last)) = row_span(&p, owns, centre_y) else {
            continue;
        };
        let (first, last) = (first.max(i64::from(min_x)), last.min(i64::from(max_x)));
        let start = Snapped {
            x: first * SUBPIXEL + SUBPIXEL / 2,
            y: centre_y,
        };
        let mut e = [
            edge(p[1], p[2], start) as f32,
            edge(p[2], p[0], start) as f32,
            edge(p[0], p[1], start) as f32,
        ];
        for x in first..=last {
            let texel = source.sample(dot(&e, &us), dot(&e, &vs));
            let coverage = dot(&e, &alphas).round().clamp(0.0, WHOLE);
            target.lay_over(x as u32, y, texel, coverage as u32);
            for (value, by) in e.iter_mut().zip(step) {
                *value += by;
            }
        }
    }
}

/// The pixels of the row whose centre is at `centre_y` that are inside the triangle, as the first
/// and the last, or `None` when there are none. Each edge test is a line in the pixel's x, so the
/// span is solved with integers, not searched: a thin triangle across a large box costs by its
/// pixels, not by its box. A pixel exactly on an edge is inside when the triangle owns the edge.
fn row_span(p: &[Snapped; 3], owns: [bool; 3], centre_y: i64) -> Option<(i64, i64)> {
    let (mut first, mut last) = (i64::MIN, i64::MAX);
    for (k, owned) in owns.into_iter().enumerate() {
        let (a, b) = (p[(k + 1) % 3], p[(k + 2) % 3]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        // The edge function at a pixel centre `cx` is `k_edge - dy * cx`; it counts when it is at
        // least `least`.
        let k_edge = dx * (centre_y - a.y) + dy * a.x;
        let least = i64::from(!owned);
        match dy.cmp(&0) {
            std::cmp::Ordering::Equal if k_edge < least => return None,
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Greater => {
                let far = (k_edge - least).div_euclid(dy);
                last = last.min((far - SUBPIXEL / 2).div_euclid(SUBPIXEL));
            }
            std::cmp::Ordering::Less => {
                let near = -(k_edge - least).div_euclid(-dy);
                first = first.max(-(SUBPIXEL / 2 - near).div_euclid(SUBPIXEL));
            }
        }
    }
    (first <= last).then_some((first, last))
}

/// The first and last pixel whose centre can lie within `values` (snapped coordinates), kept
/// inside a target `limit` pixels across. Empty (first after last) when none can.
fn bounds(values: [i64; 3], limit: u32) -> (u32, u32) {
    let low = values.into_iter().min().unwrap_or(0);
    let high = values.into_iter().max().unwrap_or(0);
    // The centre of pixel n is at n + 1/2: a centre inside needs n >= low - 1/2 and n <= high - 1/2.
    let first = (low - SUBPIXEL / 2).div_euclid(SUBPIXEL).max(0);
    let last = (high - SUBPIXEL / 2).div_euclid(SUBPIXEL) + 1;
    let last = last.min(i64::from(limit) - 1);
    if last < first {
        return (1, 0);
    }
    (first as u32, last as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, texel: [u8; 4]) -> Pixmap {
        let bytes: Vec<u8> = (0..width * height).flat_map(|_| texel).collect();
        Pixmap::from_straight_rgba(width, height, &bytes).unwrap()
    }

    fn corner(x: f32, y: f32) -> Corner {
        Corner {
            at: [x, y],
            texel: [0.5, 0.5],
            alpha: 1.0,
        }
    }

    fn quad(target: &mut Pixmap, source: &Pixmap, rect: [f32; 4], opacity: f32) {
        let [x0, y0, x1, y1] = rect;
        let (a, b, c, d) = (
            corner(x0, y0),
            corner(x1, y0),
            corner(x1, y1),
            corner(x0, y1),
        );
        fill(target, source, [a, b, c], opacity);
        fill(target, source, [a, c, d], opacity);
    }

    #[test]
    fn a_quad_covers_the_pixels_whose_centres_it_holds() {
        let mut target = flat(8, 8, [0, 0, 0, 255]);
        let red = flat(1, 1, [255, 0, 0, 255]);
        quad(&mut target, &red, [2.0, 3.0, 5.0, 6.0], 1.0);
        for y in 0..8 {
            for x in 0..8 {
                let inside = (2..5).contains(&x) && (3..6).contains(&y);
                let texel = target.at((y * 8 + x) as usize);
                assert_eq!(
                    texel,
                    if inside {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 0, 255]
                    },
                    "{x},{y}"
                );
            }
        }
    }

    #[test]
    fn two_triangles_sharing_an_edge_draw_each_pixel_once() {
        // At half opacity a pixel drawn twice would be darker than one drawn once, so a seam
        // on the diagonal shows as a different value.
        let mut target = flat(8, 8, [0, 0, 0, 255]);
        let white = flat(1, 1, [255, 255, 255, 255]);
        quad(&mut target, &white, [0.0, 0.0, 8.0, 8.0], 0.5);
        let values: Vec<u8> = (0..64).map(|i| target.at(i)[0]).collect();
        assert!(values.iter().all(|v| *v == values[0]), "{values:?}");
        assert_eq!(values[0], 128);
    }

    #[test]
    fn either_winding_draws_the_same() {
        let white = flat(1, 1, [255, 255, 255, 255]);
        let triangle = [corner(1.0, 1.0), corner(7.0, 2.0), corner(3.0, 7.0)];
        let mut one = flat(8, 8, [0, 0, 0, 255]);
        fill(&mut one, &white, triangle, 1.0);
        let mut other = flat(8, 8, [0, 0, 0, 255]);
        fill(
            &mut other,
            &white,
            [triangle[0], triangle[2], triangle[1]],
            1.0,
        );
        assert_eq!(one, other);
        assert_ne!(one.at(3 * 8 + 3), [0, 0, 0, 255]);
    }

    #[test]
    fn the_texture_follows_the_corners() {
        let source = Pixmap::from_straight_rgba(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        // A strip four pixels wide reading the whole of a two-texel picture, left to right.
        let at = |x: f32, y: f32| Corner {
            at: [x, y],
            texel: [x / 4.0, 0.5],
            alpha: 1.0,
        };
        let mut target = flat(4, 4, [0, 0, 0, 255]);
        let (a, b, c, d) = (at(0.0, 0.0), at(4.0, 0.0), at(4.0, 4.0), at(0.0, 4.0));
        fill(&mut target, &source, [a, b, c], 1.0);
        fill(&mut target, &source, [a, c, d], 1.0);
        let row: Vec<[u8; 4]> = (4..8).map(|i| target.at(i)).collect();
        assert_eq!(row[0], [255, 0, 0, 255]);
        assert_eq!(row[3], [0, 0, 255, 255]);
        // Between them the colour passes through a mix, never a jump.
        assert!(row[1][0] > row[2][0] && row[1][2] < row[2][2]);
    }

    #[test]
    fn a_triangle_outside_the_target_or_with_no_area_draws_nothing() {
        let white = flat(1, 1, [255, 255, 255, 255]);
        let mut target = flat(4, 4, [0, 0, 0, 255]);
        let before = target.clone();
        fill(
            &mut target,
            &white,
            [corner(10.0, 10.0), corner(20.0, 10.0), corner(10.0, 20.0)],
            1.0,
        );
        fill(
            &mut target,
            &white,
            [corner(-9.0, -9.0), corner(-2.0, -9.0), corner(-9.0, -2.0)],
            1.0,
        );
        fill(
            &mut target,
            &white,
            [corner(0.0, 0.0), corner(2.0, 2.0), corner(4.0, 4.0)],
            1.0,
        );
        fill(
            &mut target,
            &white,
            [
                corner(f32::NAN, 0.0),
                corner(1.0, f32::INFINITY),
                corner(2.0, 2.0),
            ],
            1.0,
        );
        assert_eq!(target, before);
    }

    #[test]
    fn the_solved_span_is_the_pixels_the_edge_tests_accept() {
        // A few triangles with corners on and off the pixel grid, thin and wide, both windings.
        let corners = [
            [(3, 5), (60, 11), (20, 90)],
            [(0, 0), (160, 8), (16, 24)],
            [(8, 8), (8, 200), (120, 104)],
            [(1, 1), (255, 3), (255, 5)],
            [(40, 40), (200, 40), (120, 72)],
        ];
        for tri in corners {
            let mut p = tri.map(|(x, y)| Snapped {
                x: x * 3 + 1,
                y: y * 3 - 2,
            });
            if edge(p[0], p[1], p[2]) < 0 {
                p.swap(1, 2);
            }
            let owns = [
                owns_edge(p[1], p[2]),
                owns_edge(p[2], p[0]),
                owns_edge(p[0], p[1]),
            ];
            for y in 0..40_i64 {
                let centre_y = y * SUBPIXEL + SUBPIXEL / 2;
                let brute: Vec<i64> = (0..40)
                    .filter(|x| {
                        let c = Snapped {
                            x: x * SUBPIXEL + SUBPIXEL / 2,
                            y: centre_y,
                        };
                        let e = [
                            edge(p[1], p[2], c),
                            edge(p[2], p[0], c),
                            edge(p[0], p[1], c),
                        ];
                        e.iter().zip(owns).all(|(v, o)| *v > 0 || (*v == 0 && o))
                    })
                    .collect();
                let solved: Vec<i64> = row_span(&p, owns, centre_y)
                    .map(|(a, b)| (a.max(0)..=b.min(39)).collect())
                    .unwrap_or_default();
                assert_eq!(solved, brute, "{tri:?} row {y}");
            }
        }
    }
}
