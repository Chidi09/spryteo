//! Exact geometry for cubic Bézier paths (§3.12, issue #8).
//!
//! The scene graph needs three things from a path: its bounding box, its
//! area, and its centroid. All three used to be computed by throwing away
//! everything except segment endpoints and treating the result as one
//! straight-edged polygon.
//!
//! That is wrong in four separate ways:
//!
//! - a curve that bulges past its endpoints has a larger bbox than its
//!   endpoint hull, so metadata under-reported the real extent;
//! - a curve's area is not its chord polygon's area, so centroid-based
//!   transform origins sat in the wrong place;
//! - multiple subpaths were joined end-to-end as if they were one ring, so
//!   a shape with a hole reported nonsense;
//! - a curved shape whose endpoints happen to be nearly collinear has
//!   near-zero *polygon* area, so it could be deleted as degenerate even
//!   though it encloses real ink.
//!
//! # Method
//!
//! **Bounding box** is exact. A cubic's extrema in each axis are the roots
//! of its derivative, a quadratic; we solve it, keep roots in `(0, 1)`, and
//! evaluate the curve there. No sampling.
//!
//! **Area and first moments** are exact too, via Green's theorem. For a
//! closed curve,
//!
//! ```text
//!     A  =  ½ ∮ (x·y' − y·x') dt
//!   ∬x dA =  ½ ∮ x²·y' dt
//!   ∬y dA = −½ ∮ y²·x' dt
//! ```
//!
//! On a cubic segment `x(t)` and `y(t)` are degree-3 polynomials, so the
//! area integrand is degree 5 and the moment integrands are degree 8.
//! Gauss–Legendre quadrature with `n` nodes is exact for polynomials of
//! degree `2n − 1`, so five nodes (exact through degree 9) integrate all
//! three exactly — up to floating-point rounding, this is a closed form,
//! not an approximation.
//!
//! **Holes** are resolved by containment parity, matching the `evenodd`
//! fill rule the emitter uses: a subpath nested inside an odd number of
//! others is a hole and its area is subtracted; nested inside an even
//! number, it is a solid island and its area is added. Orientation is not
//! trusted, because the tracer does not guarantee it.

use spryteo_core::ir::{Bbox, PathElement};

/// Segments per cubic when flattening for point-in-polygon containment
/// tests only. Area, centroid and bbox never use this — they are computed
/// on the exact curve — so this figure trades nothing but the precision of
/// "is this subpath inside that one", where the shapes involved differ by
/// far more than a 16th of a curve.
const CONTAINMENT_SEGMENTS: usize = 16;

/// Five-node Gauss–Legendre abscissae and weights on `[0, 1]`.
///
/// Exact for polynomials through degree 9, which covers the degree-8
/// moment integrands with room to spare.
const GAUSS: [(f64, f64); 5] = [
    (0.046910077030668, 0.118463442528095),
    (0.230765344947158, 0.239314335249683),
    (0.500000000000000, 0.284444444444444),
    (0.769234655052842, 0.239314335249683),
    (0.953089922969332, 0.118463442528095),
];

/// One run of segments, starting at a `MoveTo`.
///
/// Whether the run ended in an explicit `ClosePath` is not recorded: SVG
/// fills an open subpath as if it were closed, so both are measured the
/// same way.
#[derive(Debug, Clone, PartialEq)]
struct Subpath {
    start: (f64, f64),
    /// Segments after the opening `MoveTo`, each carrying its own start
    /// point so it can be evaluated independently.
    segments: Vec<Seg>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Seg {
    Line {
        from: (f64, f64),
        to: (f64, f64),
    },
    Cubic {
        p0: (f64, f64),
        p1: (f64, f64),
        p2: (f64, f64),
        p3: (f64, f64),
    },
}

impl Seg {
    fn end(&self) -> (f64, f64) {
        match *self {
            Seg::Line { to, .. } => to,
            Seg::Cubic { p3, .. } => p3,
        }
    }
}

/// Geometry of a single subpath, before holes are resolved.
#[derive(Debug, Clone)]
struct SubpathGeometry {
    /// Signed area: positive or negative depending on winding direction.
    signed_area: f64,
    /// Centroid of this ring alone.
    centroid: (f64, f64),
    /// Coarse polygon, used only for containment tests.
    polygon: Vec<(f64, f64)>,
    bbox: Bbox,
}

/// Exact geometry of a whole path.
#[derive(Debug, Clone, PartialEq)]
pub struct PathGeometry {
    /// Tight bounding box, including curve extrema between endpoints.
    pub bbox: Bbox,
    /// Area-weighted centroid with holes subtracted. For a path that
    /// encloses no area (an open stroke, or a shape whose rings cancel),
    /// this is the centre of the bounding box instead.
    pub centroid: (f64, f64),
    /// Unsigned enclosed area, holes subtracted.
    pub area: f64,
    /// A coarse polygon covering every subpath, for containment tests.
    pub polygon: Vec<(f64, f64)>,
}

impl PathGeometry {
    fn empty() -> Self {
        PathGeometry {
            bbox: Bbox {
                x_min: 0.0,
                x_max: 0.0,
                y_min: 0.0,
                y_max: 0.0,
            },
            centroid: (0.0, 0.0),
            area: 0.0,
            polygon: Vec::new(),
        }
    }
}

// ── Splitting into subpaths ────────────────────────────────────────────

fn split_subpaths(segments: &[PathElement]) -> Vec<Subpath> {
    let mut out: Vec<Subpath> = Vec::new();
    let mut current: Option<Subpath> = None;
    // Where the pen is. A `LineTo` before any `MoveTo` is malformed input;
    // we start it at the origin rather than dropping it silently.
    let mut cursor = (0.0, 0.0);

    for el in segments {
        match *el {
            PathElement::MoveTo(x, y) => {
                if let Some(sp) = current.take() {
                    out.push(sp);
                }
                cursor = (x, y);
                current = Some(Subpath {
                    start: cursor,
                    segments: Vec::new(),
                });
            }
            PathElement::LineTo(x, y) => {
                let sp = current.get_or_insert_with(|| Subpath {
                    start: cursor,
                    segments: Vec::new(),
                });
                sp.segments.push(Seg::Line {
                    from: cursor,
                    to: (x, y),
                });
                cursor = (x, y);
            }
            PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                let sp = current.get_or_insert_with(|| Subpath {
                    start: cursor,
                    segments: Vec::new(),
                });
                sp.segments.push(Seg::Cubic {
                    p0: cursor,
                    p1: (x1, y1),
                    p2: (x2, y2),
                    p3: (x3, y3),
                });
                cursor = (x3, y3);
            }
            PathElement::ClosePath => {
                if let Some(sp) = current.take() {
                    cursor = sp.start;
                    out.push(sp);
                }
            }
        }
    }
    if let Some(sp) = current.take() {
        out.push(sp);
    }
    out.retain(|sp| !sp.segments.is_empty());
    out
}

// ── Cubic evaluation and exact extrema ─────────────────────────────────

fn cubic_at(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let mt = 1.0 - t;
    mt * mt * mt * p0 + 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t * p3
}

fn cubic_derivative_at(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let mt = 1.0 - t;
    3.0 * mt * mt * (p1 - p0) + 6.0 * mt * t * (p2 - p1) + 3.0 * t * t * (p3 - p2)
}

/// Parameters in `(0, 1)` where a cubic's derivative vanishes on one axis.
///
/// With `A = p1 − p0`, `B = p2 − p1`, `C = p3 − p2`, the derivative (scaled
/// by 1/3) is `t²(A − 2B + C) + 2t(B − A) + A`. Solving that quadratic
/// gives the interior extrema exactly.
fn cubic_extrema(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let a_ = p1 - p0;
    let b_ = p2 - p1;
    let c_ = p3 - p2;

    let a = a_ - 2.0 * b_ + c_;
    let b = 2.0 * (b_ - a_);
    let c = a_;

    let mut roots = Vec::new();
    let mut push = |t: f64| {
        if t > 0.0 && t < 1.0 && t.is_finite() {
            roots.push(t);
        }
    };

    if a.abs() < 1e-12 {
        // Degenerate to linear: b·t + c = 0.
        if b.abs() > 1e-12 {
            push(-c / b);
        }
        return roots;
    }

    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return roots;
    }
    let sq = disc.sqrt();
    push((-b + sq) / (2.0 * a));
    push((-b - sq) / (2.0 * a));
    roots
}

// ── Per-segment integrals ──────────────────────────────────────────────

/// Contributions of one segment to `∮(x y' − y x')`, `∮x² y'` and `∮y² x'`.
fn segment_integrals(seg: &Seg) -> (f64, f64, f64) {
    match *seg {
        Seg::Line { from, to } => {
            let (x0, y0) = from;
            let (x1, y1) = to;
            let dx = x1 - x0;
            let dy = y1 - y0;
            // On a straight segment x(t) and y(t) are linear, so all three
            // integrals have elementary closed forms.
            let cross = x0 * y1 - x1 * y0;
            // ∫₀¹ x(t)² y'(t) dt with x linear, y' constant.
            let x2 = dy * (x0 * x0 + x0 * x1 + x1 * x1) / 3.0;
            let y2 = dx * (y0 * y0 + y0 * y1 + y1 * y1) / 3.0;
            (cross, x2, y2)
        }
        Seg::Cubic { p0, p1, p2, p3 } => {
            let mut cross = 0.0;
            let mut x2 = 0.0;
            let mut y2 = 0.0;
            for (t, w) in GAUSS {
                let x = cubic_at(p0.0, p1.0, p2.0, p3.0, t);
                let y = cubic_at(p0.1, p1.1, p2.1, p3.1, t);
                let dx = cubic_derivative_at(p0.0, p1.0, p2.0, p3.0, t);
                let dy = cubic_derivative_at(p0.1, p1.1, p2.1, p3.1, t);
                cross += w * (x * dy - y * dx);
                x2 += w * (x * x * dy);
                y2 += w * (y * y * dx);
            }
            (cross, x2, y2)
        }
    }
}

/// Flatten one segment into points for containment tests.
fn flatten_into(seg: &Seg, out: &mut Vec<(f64, f64)>) {
    match *seg {
        Seg::Line { to, .. } => out.push(to),
        Seg::Cubic { p0, p1, p2, p3 } => {
            for i in 1..=CONTAINMENT_SEGMENTS {
                let t = i as f64 / CONTAINMENT_SEGMENTS as f64;
                out.push((
                    cubic_at(p0.0, p1.0, p2.0, p3.0, t),
                    cubic_at(p0.1, p1.1, p2.1, p3.1, t),
                ));
            }
        }
    }
}

/// Extend a bbox to cover one segment exactly, curve extrema included.
fn grow_bbox(seg: &Seg, x_min: &mut f64, x_max: &mut f64, y_min: &mut f64, y_max: &mut f64) {
    let mut include = |x: f64, y: f64| {
        if x < *x_min {
            *x_min = x;
        }
        if x > *x_max {
            *x_max = x;
        }
        if y < *y_min {
            *y_min = y;
        }
        if y > *y_max {
            *y_max = y;
        }
    };

    match *seg {
        Seg::Line { from, to } => {
            include(from.0, from.1);
            include(to.0, to.1);
        }
        Seg::Cubic { p0, p1, p2, p3 } => {
            include(p0.0, p0.1);
            include(p3.0, p3.1);
            // Control points are deliberately *not* included: the curve
            // does not reach them. Only the true extrema matter.
            for t in cubic_extrema(p0.0, p1.0, p2.0, p3.0) {
                let x = cubic_at(p0.0, p1.0, p2.0, p3.0, t);
                let y = cubic_at(p0.1, p1.1, p2.1, p3.1, t);
                include(x, y);
            }
            for t in cubic_extrema(p0.1, p1.1, p2.1, p3.1) {
                let x = cubic_at(p0.0, p1.0, p2.0, p3.0, t);
                let y = cubic_at(p0.1, p1.1, p2.1, p3.1, t);
                include(x, y);
            }
        }
    }
}

// ── Subpath geometry ───────────────────────────────────────────────────

fn subpath_geometry(sp: &Subpath) -> SubpathGeometry {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    let mut polygon = vec![sp.start];
    let mut cross = 0.0;
    let mut mx = 0.0;
    let mut my = 0.0;

    for seg in &sp.segments {
        grow_bbox(seg, &mut x_min, &mut x_max, &mut y_min, &mut y_max);
        flatten_into(seg, &mut polygon);
        let (c, x2, y2) = segment_integrals(seg);
        cross += c;
        mx += x2;
        my += y2;
    }

    // Green's theorem needs a closed contour, and so does SVG: an open
    // subpath is implicitly closed with a straight segment back to its
    // start when filled. Reporting that enclosed area is therefore the
    // correct answer for fill metadata, and harmless for a stroke, where
    // area is not a meaningful quantity in the first place.
    let last = sp.segments.last().map(|s| s.end()).unwrap_or(sp.start);
    if last != sp.start {
        let closer = Seg::Line {
            from: last,
            to: sp.start,
        };
        let (c, x2, y2) = segment_integrals(&closer);
        cross += c;
        mx += x2;
        my += y2;
        // The closing segment lies within the existing hull for a genuine
        // ring, but grow the box anyway so an open path is covered.
        grow_bbox(&closer, &mut x_min, &mut x_max, &mut y_min, &mut y_max);
    }

    let signed_area = 0.5 * cross;
    let centroid = if signed_area.abs() > 1e-12 {
        // ∬x dA = ½∮x² dy and ∬y dA = −½∮y² dx.
        (0.5 * mx / signed_area, -0.5 * my / signed_area)
    } else {
        // A ring that encloses nothing has no area-weighted centre; fall
        // back to the midpoint of its extent.
        ((x_min + x_max) / 2.0, (y_min + y_max) / 2.0)
    };

    SubpathGeometry {
        signed_area,
        centroid,
        polygon,
        bbox: Bbox {
            x_min,
            x_max,
            y_min,
            y_max,
        },
    }
}

// ── Containment ────────────────────────────────────────────────────────

/// Ray-casting point-in-polygon test.
fn point_in_polygon(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let (x, y) = p;
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) {
            let denom = yj - yi;
            if denom != 0.0 && x < (xj - xi) * (y - yi) / denom + xi {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn bbox_contains(outer: &Bbox, inner: &Bbox) -> bool {
    outer.x_min <= inner.x_min
        && outer.x_max >= inner.x_max
        && outer.y_min <= inner.y_min
        && outer.y_max >= inner.y_max
}

/// How many other subpaths strictly contain this one.
///
/// Containment is judged by a representative point rather than by every
/// vertex: two rings from the same traced region can share boundary
/// points, and an all-vertices test would then report "not contained" for
/// a hole that plainly is.
fn nesting_depth(i: usize, subs: &[SubpathGeometry]) -> usize {
    let inner = &subs[i];
    let Some(&probe) = inner.polygon.first() else {
        return 0;
    };
    // Use the ring's centroid when it genuinely lies inside the ring;
    // otherwise any boundary point still discriminates between rings.
    let probe = if point_in_polygon(inner.centroid, &inner.polygon) {
        inner.centroid
    } else {
        probe
    };

    subs.iter()
        .enumerate()
        .filter(|&(j, outer)| {
            j != i
                && outer.signed_area.abs() > inner.signed_area.abs()
                && bbox_contains(&outer.bbox, &inner.bbox)
                && point_in_polygon(probe, &outer.polygon)
        })
        .count()
}

// ── Public entry point ─────────────────────────────────────────────────

/// Compute exact geometry for a path.
///
/// Each subpath is measured on its own; a subpath nested inside an odd
/// number of others is treated as a hole and subtracted, matching the
/// `evenodd` fill rule the emitter uses. See the module docs for the
/// method and its exactness guarantees.
pub fn path_geometry(segments: &[PathElement]) -> PathGeometry {
    let subpaths = split_subpaths(segments);
    if subpaths.is_empty() {
        return PathGeometry::empty();
    }

    let subs: Vec<SubpathGeometry> = subpaths.iter().map(subpath_geometry).collect();

    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut polygon = Vec::new();
    for s in &subs {
        x_min = x_min.min(s.bbox.x_min);
        x_max = x_max.max(s.bbox.x_max);
        y_min = y_min.min(s.bbox.y_min);
        y_max = y_max.max(s.bbox.y_max);
        polygon.extend_from_slice(&s.polygon);
    }

    // Holes by containment parity, not by winding direction: the tracer
    // does not promise consistent orientation, and `evenodd` does not care
    // about it either.
    let mut net_area = 0.0;
    let mut wx = 0.0;
    let mut wy = 0.0;
    for i in 0..subs.len() {
        let a = subs[i].signed_area.abs();
        let sign = if nesting_depth(i, &subs).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        net_area += sign * a;
        wx += sign * a * subs[i].centroid.0;
        wy += sign * a * subs[i].centroid.1;
    }

    let area = net_area.abs();
    let centroid = if area > 1e-12 {
        (wx / net_area, wy / net_area)
    } else {
        ((x_min + x_max) / 2.0, (y_min + y_max) / 2.0)
    };

    PathGeometry {
        bbox: Bbox {
            x_min,
            x_max,
            y_min,
            y_max,
        },
        centroid,
        area,
        polygon,
    }
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;

/// Arc length of a path, following the outline as drawn.
///
/// Unlike [`path_geometry`], this does not treat an open subpath as closed:
/// a filled shape's area is unaffected by whether the author wrote the
/// closing segment, but its perimeter plainly is, and a centreline stroke is
/// open on purpose. The closing segment counts only where the path data
/// actually asks for it with a `ClosePath`.
///
/// Cubics are measured by Gauss–Legendre quadrature of `|B'(t)|` over each
/// of [`LENGTH_SPANS`] equal spans. The integrand is a square root rather
/// than a polynomial, so no fixed rule is exact; subdividing first keeps the
/// error far below the coordinate precision the SVG is written at.
pub fn path_length(segments: &[PathElement]) -> f64 {
    let mut total = 0.0;
    // Tracked here rather than via `split_subpaths`, which discards the
    // closing segment because area does not need it.
    let mut start = (0.0, 0.0);
    let mut cursor = (0.0, 0.0);
    for el in segments {
        match *el {
            PathElement::MoveTo(x, y) => {
                start = (x, y);
                cursor = start;
            }
            PathElement::LineTo(x, y) => {
                total += segment_length(&Seg::Line {
                    from: cursor,
                    to: (x, y),
                });
                cursor = (x, y);
            }
            PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                total += segment_length(&Seg::Cubic {
                    p0: cursor,
                    p1: (x1, y1),
                    p2: (x2, y2),
                    p3: (x3, y3),
                });
                cursor = (x3, y3);
            }
            PathElement::ClosePath => {
                total += segment_length(&Seg::Line {
                    from: cursor,
                    to: start,
                });
                cursor = start;
            }
        }
    }
    total
}

/// How many spans each cubic is split into before quadrature.
const LENGTH_SPANS: usize = 8;

fn segment_length(seg: &Seg) -> f64 {
    match *seg {
        Seg::Line { from, to } => (to.0 - from.0).hypot(to.1 - from.1),
        Seg::Cubic { p0, p1, p2, p3 } => {
            let span = 1.0 / LENGTH_SPANS as f64;
            let mut total = 0.0;
            for i in 0..LENGTH_SPANS {
                let t0 = i as f64 * span;
                for (t, w) in GAUSS {
                    let t = t0 + t * span;
                    let dx = cubic_derivative_at(p0.0, p1.0, p2.0, p3.0, t);
                    let dy = cubic_derivative_at(p0.1, p1.1, p2.1, p3.1, t);
                    total += w * dx.hypot(dy) * span;
                }
            }
            total
        }
    }
}
