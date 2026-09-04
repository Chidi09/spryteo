//! Polygon simplification, corner detection, and Bezier curve fitting (§3.6-3.7).
//!
//! This crate implements the curve-fitting pipeline of Spryteo:
//! 1. Greedy optimal polygon simplification (O(N) to O(N log N) approximation of Potrace's shortest path).
//! 2. Corner classification using the deviation angle and a smoothness threshold.
//! 3. Recursive least-squares cubic Bezier fitting with G1 continuity enforced at smooth joins.
//! 4. Primitive recognition on original contour points.

use spryteo_core::ir::{Contour, ContourSet, Curve, CurveSet, PathElement};
use spryteo_core::{CancelToken, SpryteoError};

/// Maximum perpendicular deviation (px) of contour points from a chord for
/// the run to be emitted as a straight `LineTo` instead of a fitted cubic.
/// Matches the polygon stage's 0.5px straightness spirit; straight edges
/// force-fit through Beziers waste bytes and look subtly melted.
const STRAIGHT_TOL: f64 = 0.5;

/// Fits the contours in a `ContourSet` into a flat `CurveSet` of fitted vector paths.
///
/// This is the public entry point for the curve fitting pipeline.
/// It processes layers and their nested contours in a deterministic, depth-first order
/// and returns the flattened set of curves.
pub fn fit_contours(contours: &ContourSet, tolerance: f32, smoothness: f32) -> CurveSet {
    fit_contours_cancellable(contours, tolerance, smoothness, &CancelToken::none())
        .expect("fit_contours with an inert CancelToken cannot be cancelled")
}

/// [`fit_contours`] with cooperative cancellation (ROADMAP §3.1).
///
/// Checkpoints sit at the top-level contour, which is the unit of work
/// rayon schedules and is bounded by that contour's point count. The
/// deterministic depth-first output order is unchanged: cancellation only
/// ever aborts, never reorders.
pub fn fit_contours_cancellable(
    contours: &ContourSet,
    tolerance: f32,
    smoothness: f32,
    cancel: &CancelToken,
) -> Result<CurveSet, SpryteoError> {
    let mut curves = Vec::new();
    for layer in &contours.layers {
        cancel.check()?;

        #[cfg(feature = "parallel")]
        {
            use rayon::prelude::*;
            let layer_curves: Vec<Vec<Curve>> = layer
                .par_iter()
                .map(|contour| {
                    cancel.check()?;
                    let mut sub_curves = Vec::new();
                    flatten_contour(contour, &mut sub_curves, tolerance, smoothness);
                    Ok(sub_curves)
                })
                .collect::<Result<Vec<_>, SpryteoError>>()?;
            for sub_curves in layer_curves {
                curves.extend(sub_curves);
            }
        }

        #[cfg(not(feature = "parallel"))]
        {
            for contour in layer {
                cancel.check()?;
                flatten_contour(contour, &mut curves, tolerance, smoothness);
            }
        }
    }
    Ok(CurveSet { curves })
}

/// Recursively flattens a nested contour and its children, fitting each to a `Curve`.
fn flatten_contour(contour: &Contour, curves: &mut Vec<Curve>, tolerance: f32, smoothness: f32) {
    let mut curve = fit_single_contour(contour, tolerance, smoothness);
    // Holes become additional subpaths of THIS path, cut out by
    // fill-rule="evenodd" — a letter "O" is one path with two subpaths,
    // never a second same-colored shape painted on top (which would cover
    // whatever sits underneath the hole). Islands inside a hole
    // (grandchildren) are genuinely separate shapes and recurse as new
    // top-level curves, painting after their holed container.
    if !contour.children.is_empty() {
        // A path with hole subpaths can no longer be a single primitive.
        curve.primitive = None;
        for hole in &contour.children {
            let hole_curve = fit_single_contour(hole, tolerance, smoothness);
            curve.segments.extend(hole_curve.segments);
        }
    }
    curves.push(curve);
    for hole in &contour.children {
        for island in &hole.children {
            flatten_contour(island, curves, tolerance, smoothness);
        }
    }
}

/// Fits a single closed contour to a `Curve`.
fn fit_single_contour(contour: &Contour, tolerance: f32, smoothness: f32) -> Curve {
    let points = &contour.points;
    let n = points.len();

    // Degenerate case: too few points
    if n < 3 {
        let mut elements = Vec::new();
        if !points.is_empty() {
            elements.push(PathElement::MoveTo(points[0].0, points[0].1));
            for pt in points.iter().skip(1) {
                elements.push(PathElement::LineTo(pt.0, pt.1));
            }
            elements.push(PathElement::ClosePath);
        }
        let primitive = spryteo_geom::recognize(points, tolerance as f64);
        return Curve {
            segments: elements,
            primitive,
        };
    }

    // Stage 1: Greedy optimal polygon simplification
    // We try every starting point and pick the one with the fewest vertices,
    // tie-broken by the least squared deviation of the original points from the polygon edges.
    // Documented simplification: This is a greedy approximation of the paper's
    // globally-optimal shortest-path formulation, not the full DP-over-cycles version.
    let mut best_polygon = None;
    let mut min_len = usize::MAX;
    let mut min_deviation = f64::INFINITY;

    // Shared across every starting-point attempt below (not reset per
    // call) so the *total* admissibility-check work for this whole
    // contour is bounded to O(n), regardless of how many of the n
    // starting points turn out to be expensive -- see greedy_polygon's
    // doc comment for why this budget exists.
    let mut budget: i64 = (n as i64).saturating_mul(200);

    // `compute_squared_deviation` below is itself O(n) per starting point,
    // so trying literally all n starting points is O(n^2) regardless of
    // the budget above (which only bounds greedy_polygon's own internal
    // search). For small/typical contours this is fine and every
    // starting point is tried, matching prior behavior exactly. For very
    // long contours (thousands of points -- e.g. quantization-band
    // boundaries in a smooth-gradient photo), sample a bounded number of
    // evenly-spaced starting points instead: diminishing returns on
    // trying every single rotation for a contour that long, and this is
    // what actually keeps whole-contour cost bounded rather than merely
    // per-step cost.
    const MAX_STARTING_POINTS: usize = 64;
    let stride = (n / MAX_STARTING_POINTS).max(1);

    for i_0 in (0..n).step_by(stride) {
        let poly = greedy_polygon(points, i_0, tolerance as f64, &mut budget);
        let len = poly.len();
        if len >= 3 {
            let deviation = compute_squared_deviation(points, &poly);
            if len < min_len || (len == min_len && deviation < min_deviation) {
                min_len = len;
                min_deviation = deviation;
                best_polygon = Some(poly);
            }
        }
    }

    let vertices = best_polygon.unwrap_or_else(|| (0..n).collect());
    let m = vertices.len();

    // Stage 2: Corner detection
    // A vertex is a CORNER if the angle between its two adjacent segments deviates from straight (180°)
    // by more than a threshold derived from `smoothness`.
    // Formula:
    //   alpha = normalized deviation from collinear in [0, 1]
    //   threshold = smoothness / 1.34
    //   smooth if alpha < threshold, corner (sharp) otherwise.
    let mut is_corner = vec![false; m];
    if m < 3 {
        is_corner.fill(true);
    } else {
        for k in 0..m {
            let v_prev = points[vertices[(k + m - 1) % m]];
            let v_curr = points[vertices[k]];
            let v_next = points[vertices[(k + 1) % m]];

            let ax = v_curr.0 - v_prev.0;
            let ay = v_curr.1 - v_prev.1;
            let bx = v_next.0 - v_curr.0;
            let by = v_next.1 - v_curr.1;

            let len_a = (ax * ax + ay * ay).sqrt();
            let len_b = (bx * bx + by * by).sqrt();

            if len_a < 1e-9 || len_b < 1e-9 {
                is_corner[k] = false;
            } else {
                let dot = ax * bx + ay * by;
                let cos_theta = (dot / (len_a * len_b)).clamp(-1.0, 1.0);
                let theta = cos_theta.acos();
                let alpha = theta / std::f64::consts::PI;
                is_corner[k] = alpha >= (smoothness as f64 / 1.34);
            }
        }
    }

    // Collect corner indices (mapping back to original contour points)
    let corners: Vec<usize> = (0..m)
        .filter(|&k| is_corner[k])
        .map(|k| vertices[k])
        .collect();
    let num_corners = corners.len();

    // Compute tangent vectors at each original point for G1 continuity
    let mut contour_tangents = vec![(0.0, 0.0); n];
    for i in 0..n {
        let prev = points[(i + n - 1) % n];
        let next = points[(i + 1) % n];
        let dx = next.0 - prev.0;
        let dy = next.1 - prev.1;
        let len = (dx * dx + dy * dy).sqrt();
        if len > 1e-9 {
            contour_tangents[i] = (dx / len, dy / len);
        } else {
            contour_tangents[i] = (1.0, 0.0);
        }
    }

    // Stage 3: Bezier fitting per span between corners
    let mut raw_elements = Vec::new();
    let start_idx = if num_corners >= 1 {
        corners[0]
    } else {
        vertices[0]
    };
    let start_pt = points[start_idx];
    raw_elements.push(PathElement::MoveTo(start_pt.0, start_pt.1));

    if num_corners >= 1 {
        // Spans between consecutive corners
        for i in 0..num_corners {
            let s_idx = corners[i];
            let e_idx = corners[(i + 1) % num_corners];

            // Push-then-advance so a single-corner contour (s_idx ==
            // e_idx) walks the full loop instead of producing an empty
            // span that collapses the whole outline to a degenerate
            // LineTo.
            let mut span_indices = Vec::new();
            let mut curr = s_idx;
            loop {
                span_indices.push(curr);
                curr = (curr + 1) % n;
                if curr == e_idx {
                    break;
                }
            }
            span_indices.push(e_idx);

            // Check if the span is straight within tolerance
            let is_straight = if span_indices.len() <= 2 {
                true
            } else {
                let q0 = points[span_indices[0]];
                let q_last = points[*span_indices.last().unwrap()];
                let mut straight = true;
                for &idx in &span_indices[1..span_indices.len() - 1] {
                    let pt = points[idx];
                    if perpendicular_distance(pt, q0, q_last) > STRAIGHT_TOL {
                        straight = false;
                        break;
                    }
                }
                straight
            };

            if is_straight {
                let q_last = points[e_idx];
                raw_elements.push(PathElement::LineTo(q_last.0, q_last.1));
            } else {
                // Fit curved span recursively (endpoints are corners, so not constrained at the root)
                let mut span_elements = Vec::new();
                fit_recursive(
                    points,
                    &contour_tangents,
                    &span_indices,
                    false,
                    false,
                    tolerance as f64,
                    0,
                    span_indices.len() - 1,
                    &mut span_elements,
                );
                let merged = merge_bezier_segments(
                    points,
                    &contour_tangents,
                    &span_indices,
                    false,
                    false,
                    tolerance as f64,
                    &span_elements,
                );
                raw_elements.extend(merged);
            }
        }
    } else {
        // No corners: fit the entire contour as a single closed loop, split at vertices[0]
        let s_idx = vertices[0];
        let mut span_indices = Vec::new();
        for i in 0..n {
            span_indices.push((s_idx + i) % n);
        }
        span_indices.push(s_idx);

        // Fit curved span recursively (endpoints are smooth, so constrained to tangent direction)
        let mut span_elements = Vec::new();
        fit_recursive(
            points,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance as f64,
            0,
            span_indices.len() - 1,
            &mut span_elements,
        );
        let merged = merge_bezier_segments(
            points,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance as f64,
            &span_elements,
        );
        raw_elements.extend(merged);
    }
    raw_elements.push(PathElement::ClosePath);

    // Merge consecutive collinear LineTos: straight-run recovery splits at
    // recursion midpoints, so one physical edge can arrive as several
    // collinear pieces. Greedily extend each merged chord while every
    // dropped vertex stays within STRAIGHT_TOL of it.
    let raw_elements = {
        let mut out: Vec<PathElement> = Vec::with_capacity(raw_elements.len());
        let mut prev_pt = start_pt;
        let mut i = 0;
        while i < raw_elements.len() {
            if matches!(raw_elements[i], PathElement::LineTo(_, _)) {
                let mut run: Vec<(f64, f64)> = Vec::new();
                while i < raw_elements.len() {
                    if let PathElement::LineTo(x, y) = raw_elements[i] {
                        run.push((x, y));
                        i += 1;
                    } else {
                        break;
                    }
                }
                let mut base = prev_pt;
                let mut s = 0;
                while s < run.len() {
                    let mut e = s;
                    'extend: while e + 1 < run.len() {
                        let cand = run[e + 1];
                        for &v in &run[s..=e] {
                            if perpendicular_distance(v, base, cand) > STRAIGHT_TOL {
                                break 'extend;
                            }
                        }
                        e += 1;
                    }
                    out.push(PathElement::LineTo(run[e].0, run[e].1));
                    base = run[e];
                    s = e + 1;
                }
                prev_pt = base;
            } else {
                let elem = raw_elements[i].clone();
                prev_pt = match elem {
                    PathElement::MoveTo(x, y) => (x, y),
                    PathElement::LineTo(x, y) => (x, y),
                    PathElement::CurveTo(_, _, _, _, x, y) => (x, y),
                    PathElement::ClosePath => start_pt,
                };
                out.push(elem);
                i += 1;
            }
        }
        out
    };

    // Stage 4: Numeric hygiene & sanitization
    let mut sanitized_elements = Vec::new();
    let mut last_pt = start_pt;
    for elem in raw_elements {
        let next_pt = match elem {
            PathElement::MoveTo(x, y) => (x, y),
            PathElement::LineTo(x, y) => (x, y),
            PathElement::CurveTo(_, _, _, _, x, y) => (x, y),
            PathElement::ClosePath => start_pt,
        };
        let sanitized = sanitize_element(elem, last_pt);
        sanitized_elements.push(sanitized);
        last_pt = next_pt;
    }

    // Stage 5: Primitive recognition on the ORIGINAL points via spryteo-geom
    let primitive = spryteo_geom::recognize(points, tolerance as f64);

    Curve {
        segments: sanitized_elements,
        primitive,
    }
}

/// Checks if a segment from start to end (indices modulo N) is admissible.
/// Checks admissibility, decrementing `budget` by the number of points
/// examined. If `budget` is already exhausted, returns `false`
/// immediately without examining anything -- this is what bounds
/// `greedy_polygon`'s worst case (see its doc comment).
fn is_admissible(
    points: &[(f64, f64)],
    start: usize,
    end: usize,
    tolerance: f64,
    budget: &mut i64,
) -> bool {
    let n = points.len();
    if end <= start + 1 {
        return true;
    }
    if *budget <= 0 {
        return false;
    }
    let a = points[start % n];
    let b = points[end % n];
    for k in (start + 1)..end {
        *budget -= 1;
        if *budget <= 0 {
            return false;
        }
        let p = points[k % n];
        if perpendicular_distance(p, a, b) > tolerance {
            return false;
        }
    }
    true
}

/// Greedily extends the admissible segment as far as possible from a starting index.
///
/// # Worst-case complexity guard
///
/// For each step, the candidate search below scans backward from the far
/// end of the remaining points, and each candidate check
/// (`is_admissible`) is itself O(distance). In the worst case (long,
/// nearly-straight contours -- e.g. quantization-band boundaries in a
/// smooth-gradient photo) this makes a single `greedy_polygon` call
/// O(n^3), and since `fit_single_contour` calls it once per starting
/// point, the whole-contour cost is O(n^4). Confirmed in practice: a
/// single 1024x1024 photo-mode conversion took ~560s in this stage
/// alone, hanging the browser tab (WASM runs synchronously on the main
/// thread) for real, non-synthetic uploads.
///
/// `budget` bounds total admissibility-check work per call. Once
/// exhausted, the candidate search falls back to the smallest safe step
/// (`curr + 1`) instead of the expensive backward scan -- always
/// correct (just less optimally simplified for the remainder of that
/// one contour), and guarantees termination instead of an unbounded
/// hang. Budget is sized per-call in `fit_single_contour` so small/
/// typical contours never come close to it.
fn greedy_polygon(
    points: &[(f64, f64)],
    i_0: usize,
    tolerance: f64,
    budget: &mut i64,
) -> Vec<usize> {
    let n = points.len();
    let mut vertices = vec![i_0];
    let mut curr = i_0;
    while curr < i_0 + n {
        if curr != i_0 && is_admissible(points, curr, i_0 + n, tolerance, budget) {
            vertices.push(i_0 + n);
            break;
        }
        let mut next_val = curr + 1;
        if *budget > 0 {
            for candidate in (curr + 2..=i_0 + n).rev() {
                if is_admissible(points, curr, candidate, tolerance, budget) {
                    next_val = candidate;
                    break;
                }
                if *budget <= 0 {
                    break;
                }
            }
        }
        vertices.push(next_val);
        curr = next_val;
    }
    let mapped: Vec<usize> = vertices
        .iter()
        .take(vertices.len() - 1)
        .map(|&v| v % n)
        .collect();
    mapped
}

/// Computes the sum of squared perpendicular distances of the original points from the simplified polygon edges.
fn compute_squared_deviation(points: &[(f64, f64)], vertices: &[usize]) -> f64 {
    let n = points.len();
    let m = vertices.len();
    if m == 0 {
        return 0.0;
    }
    let mut sum_sq = 0.0;
    for i in 0..m {
        let v_curr = vertices[i];
        let v_next = vertices[(i + 1) % m];
        let a = points[v_curr];
        let b = points[v_next];
        let start = v_curr;
        let mut end = v_next;
        if end <= start {
            end += n;
        }
        for k in (start + 1)..end {
            let p = points[k % n];
            let d = perpendicular_distance(p, a, b);
            sum_sq += d * d;
        }
    }
    sum_sq
}

/// Calculates the perpendicular distance from point p to the line passing through a and b.
fn perpendicular_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-18 {
        let px = p.0 - a.0;
        let py = p.1 - a.1;
        (px * px + py * py).sqrt()
    } else {
        let num = ((dx * (a.1 - p.1)) - ((a.0 - p.0) * dy)).abs();
        num / len_sq.sqrt()
    }
}

/// Computes the maximum distance from the points to the fitted bezier segment.
fn evaluate_bezier_error(points: &[(f64, f64)], c1: (f64, f64), c2: (f64, f64)) -> f64 {
    let q0 = points[0];
    let q_last = *points.last().unwrap();
    let params = chord_length_parameterization(points);
    let mut max_dev = 0.0;
    for (i, &pt) in points.iter().enumerate() {
        let t = params[i];
        let fit_pt = evaluate_bezier(q0, c1, c2, q_last, t);
        let dx = pt.0 - fit_pt.0;
        let dy = pt.1 - fit_pt.1;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist > max_dev {
            max_dev = dist;
        }
    }
    max_dev
}

/// Fits a cubic Bezier curve to a sub-span of indices recursively.
/// Enforces G1 continuity at smooth joins by constraining control points along tangent directions.
#[allow(clippy::too_many_arguments)] // internal recursion carrying span-position bookkeeping for the merge pass
fn fit_recursive(
    contour_points: &[(f64, f64)],
    contour_tangents: &[(f64, f64)],
    indices: &[usize],
    constrain_start: bool,
    constrain_end: bool,
    tolerance: f64,
    start_pos: usize,
    end_pos: usize,
    elements: &mut Vec<(PathElement, usize, usize)>,
) {
    let points: Vec<(f64, f64)> = indices.iter().map(|&idx| contour_points[idx]).collect();

    // Straight-run recovery: if every point sits within STRAIGHT_TOL of the
    // chord, a LineTo represents the run exactly — no cubic needed. The
    // chord test needs distinct endpoints (a closed loop entering here has
    // a degenerate chord and must recurse instead).
    let q_first = points[0];
    let q_last = *points.last().unwrap();
    let chord_len_sq = (q_last.0 - q_first.0).powi(2) + (q_last.1 - q_first.1).powi(2);
    if points.len() >= 2 && chord_len_sq > 1e-12 {
        let is_straight = points[1..points.len() - 1]
            .iter()
            .all(|&p| perpendicular_distance(p, q_first, q_last) <= STRAIGHT_TOL);
        if is_straight {
            elements.push((PathElement::LineTo(q_last.0, q_last.1), start_pos, end_pos));
            return;
        }
    }

    let start_tangent = if constrain_start {
        Some(contour_tangents[indices[0]])
    } else {
        None
    };
    let end_tangent = if constrain_end {
        Some(contour_tangents[*indices.last().unwrap()])
    } else {
        None
    };

    let (c1, c2) = fit_bezier_segment(&points, start_tangent, end_tangent);

    let max_dev = evaluate_bezier_error(&points, c1, c2);

    // If the fit exceeds tolerance, we split at the midpoint.
    if max_dev <= tolerance || indices.len() <= 2 {
        elements.push((
            PathElement::CurveTo(c1.0, c1.1, c2.0, c2.1, q_last.0, q_last.1),
            start_pos,
            end_pos,
        ));
    } else {
        let mid_idx = indices.len() / 2;
        fit_recursive(
            contour_points,
            contour_tangents,
            &indices[..=mid_idx],
            constrain_start,
            true,
            tolerance,
            start_pos,
            start_pos + mid_idx,
            elements,
        );
        fit_recursive(
            contour_points,
            contour_tangents,
            &indices[mid_idx..],
            true,
            constrain_end,
            tolerance,
            start_pos + mid_idx,
            end_pos,
            elements,
        );
    }
}

/// Greedily merges consecutive Bezier segments inside a single curved span.
pub(crate) fn merge_bezier_segments(
    contour_points: &[(f64, f64)],
    contour_tangents: &[(f64, f64)],
    span_indices: &[usize],
    constrain_start: bool,
    constrain_end: bool,
    tolerance: f64,
    elements: &[(PathElement, usize, usize)],
) -> Vec<PathElement> {
    if elements.is_empty() {
        return Vec::new();
    }

    let mut merged = Vec::new();
    let mut i = 0;

    while i < elements.len() {
        let mut current_elem = elements[i].clone();
        let mut j = i;

        // Straight runs stay straight: merging re-fits a cubic across the
        // combined range, which would melt a recovered LineTo back into a
        // Bezier — never merge from or across one.
        while j + 1 < elements.len()
            && !matches!(current_elem.0, PathElement::LineTo(_, _))
            && !matches!(elements[j + 1].0, PathElement::LineTo(_, _))
        {
            let next_end = elements[j + 1].2;
            let start_pos = current_elem.1;
            let end_pos = next_end;

            let run_points: Vec<(f64, f64)> = span_indices[start_pos..=end_pos]
                .iter()
                .map(|&idx| contour_points[idx])
                .collect();

            let c_start = if start_pos == 0 {
                constrain_start
            } else {
                true
            };
            let start_tangent = if c_start {
                Some(contour_tangents[span_indices[start_pos]])
            } else {
                None
            };

            let c_end = if end_pos == span_indices.len() - 1 {
                constrain_end
            } else {
                true
            };
            let end_tangent = if c_end {
                Some(contour_tangents[span_indices[end_pos]])
            } else {
                None
            };

            let (c1, c2) = fit_bezier_segment(&run_points, start_tangent, end_tangent);
            let max_dev = evaluate_bezier_error(&run_points, c1, c2);

            if max_dev <= tolerance {
                let q_last = *run_points.last().unwrap();
                current_elem = (
                    PathElement::CurveTo(c1.0, c1.1, c2.0, c2.1, q_last.0, q_last.1),
                    start_pos,
                    end_pos,
                );
                j += 1;
            } else {
                break;
            }
        }

        merged.push(current_elem.0);
        i = j + 1;
    }

    merged
}

/// Evaluates a cubic Bezier curve at parameter t in [0, 1].
fn evaluate_bezier(
    c0: (f64, f64),
    c1: (f64, f64),
    c2: (f64, f64),
    c3: (f64, f64),
    t: f64,
) -> (f64, f64) {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let mt3 = mt2 * mt;
    let t2 = t * t;
    let t3 = t2 * t;

    let x = mt3 * c0.0 + 3.0 * mt2 * t * c1.0 + 3.0 * mt * t2 * c2.0 + t3 * c3.0;
    let y = mt3 * c0.1 + 3.0 * mt2 * t * c1.1 + 3.0 * mt * t2 * c2.1 + t3 * c3.1;
    (x, y)
}

/// Computes the chord-length parameterization of the given points.
fn chord_length_parameterization(points: &[(f64, f64)]) -> Vec<f64> {
    let m = points.len();
    if m == 0 {
        return Vec::new();
    }
    let mut d = vec![0.0; m];
    for i in 1..m {
        let dx = points[i].0 - points[i - 1].0;
        let dy = points[i].1 - points[i - 1].1;
        let dist = (dx * dx + dy * dy).sqrt();
        d[i] = d[i - 1] + dist;
    }
    let total_len = d[m - 1];
    let mut t = vec![0.0; m];
    if total_len > 1e-9 {
        for (i, val) in t.iter_mut().enumerate() {
            *val = d[i] / total_len;
        }
    } else {
        for (i, val) in t.iter_mut().enumerate() {
            *val = i as f64 / (m - 1) as f64;
        }
    }
    t
}

/// Fits a cubic Bezier segment to the points.
/// Optionally constrains the start and/or end tangent directions (G1 continuity).
fn fit_bezier_segment(
    points: &[(f64, f64)],
    start_tangent: Option<(f64, f64)>,
    end_tangent: Option<(f64, f64)>,
) -> ((f64, f64), (f64, f64)) {
    let m = points.len();
    let q0 = points[0];
    let q_last = points[m - 1];

    let dx = q_last.0 - q0.0;
    let dy = q_last.1 - q0.1;
    let chord_len = (dx * dx + dy * dy).sqrt();

    // Default chord-based control points (fallback)
    let fallback = || {
        let c1 = (q0.0 + dx / 3.0, q0.1 + dy / 3.0);
        let c2 = (q0.0 + 2.0 * dx / 3.0, q0.1 + 2.0 * dy / 3.0);
        (c1, c2)
    };

    if m <= 2 || chord_len < 1e-9 {
        return fallback();
    }

    let params = chord_length_parameterization(points);

    // Compute coefficients for least-squares fitting
    let mut a11 = 0.0;
    let mut a12 = 0.0;
    let mut a22 = 0.0;
    for &t in &params {
        let f1 = 3.0 * (1.0 - t) * (1.0 - t) * t;
        let f2 = 3.0 * (1.0 - t) * t * t;
        a11 += f1 * f1;
        a12 += f1 * f2;
        a22 += f2 * f2;
    }

    match (start_tangent, end_tangent) {
        (None, None) => {
            // Case 1: Unconstrained least squares
            let mut b1x = 0.0;
            let mut b2x = 0.0;
            let mut b1y = 0.0;
            let mut b2y = 0.0;

            for (i, &pt) in points.iter().enumerate() {
                let t = params[i];
                let f1 = 3.0 * (1.0 - t) * (1.0 - t) * t;
                let f2 = 3.0 * (1.0 - t) * t * t;
                let mt = 1.0 - t;
                let mt3 = mt * mt * mt;
                let t3 = t * t * t;
                let yx = pt.0 - mt3 * q0.0 - t3 * q_last.0;
                let yy = pt.1 - mt3 * q0.1 - t3 * q_last.1;

                b1x += f1 * yx;
                b2x += f2 * yx;
                b1y += f1 * yy;
                b2y += f2 * yy;
            }

            let det = a11 * a22 - a12 * a12;
            if det.abs() < 1e-9 {
                return fallback();
            }

            let c1x = (b1x * a22 - b2x * a12) / det;
            let c2x = (a11 * b2x - a12 * b1x) / det;
            let c1y = (b1y * a22 - b2y * a12) / det;
            let c2y = (a11 * b2y - a12 * b1y) / det;

            ((c1x, c1y), (c2x, c2y))
        }
        (Some(t0), None) => {
            // Case 2: Start constrained (t0), end unconstrained
            let mut r0 = 0.0;
            let mut r1 = 0.0;
            let mut r2 = 0.0;

            for (i, &pt) in points.iter().enumerate() {
                let t = params[i];
                let f1 = 3.0 * (1.0 - t) * (1.0 - t) * t;
                let f2 = 3.0 * (1.0 - t) * t * t;
                let mt = 1.0 - t;
                let mt3 = mt * mt * mt;
                let t3 = t * t * t;

                let yx = pt.0 - mt3 * q0.0 - t3 * q_last.0 - f1 * q0.0;
                let yy = pt.1 - mt3 * q0.1 - t3 * q_last.1 - f1 * q0.1;

                r0 += t0.0 * f1 * yx + t0.1 * f1 * yy;
                r1 += f2 * yx;
                r2 += f2 * yy;
            }

            let det = a11 * a22 - a12 * a12;
            if det.abs() < 1e-9 {
                return fallback();
            }

            let alpha = (a22 * r0 - a12 * (t0.0 * r1 + t0.1 * r2)) / det;
            if alpha <= 0.0 {
                return fallback();
            }

            let c1 = (q0.0 + alpha * t0.0, q0.1 + alpha * t0.1);
            let c2x = (r1 - a12 * t0.0 * alpha) / a22;
            let c2y = (r2 - a12 * t0.1 * alpha) / a22;

            (c1, (c2x, c2y))
        }
        (None, Some(tm)) => {
            // Case 3: Start unconstrained, end constrained (tm)
            let mut r0 = 0.0;
            let mut r1 = 0.0;
            let mut r2 = 0.0;

            for (i, &pt) in points.iter().enumerate() {
                let t = params[i];
                let f1 = 3.0 * (1.0 - t) * (1.0 - t) * t;
                let f2 = 3.0 * (1.0 - t) * t * t;
                let mt = 1.0 - t;
                let mt3 = mt * mt * mt;
                let t3 = t * t * t;

                let yx = pt.0 - mt3 * q0.0 - t3 * q_last.0 - f2 * q_last.0;
                let yy = pt.1 - mt3 * q0.1 - t3 * q_last.1 - f2 * q_last.1;

                r0 += f1 * yx;
                r1 += f1 * yy;
                r2 += -tm.0 * f2 * yx - tm.1 * f2 * yy;
            }

            let det = a11 * a22 - a12 * a12;
            if det.abs() < 1e-9 {
                return fallback();
            }

            let beta = (a11 * r2 + a12 * (tm.0 * r0 + tm.1 * r1)) / det;
            if beta <= 0.0 {
                return fallback();
            }

            let c2 = (q_last.0 - beta * tm.0, q_last.1 - beta * tm.1);
            let c1x = (r0 + a12 * tm.0 * beta) / a11;
            let c1y = (r1 + a12 * tm.1 * beta) / a11;

            ((c1x, c1y), c2)
        }
        (Some(t0), Some(tm)) => {
            // Case 4: Both constrained
            let mut r1 = 0.0;
            let mut r2 = 0.0;

            let m11 = a11;
            let m12 = -a12 * (t0.0 * tm.0 + t0.1 * tm.1);
            let m22 = a22;

            for (i, &pt) in points.iter().enumerate() {
                let t = params[i];
                let f1 = 3.0 * (1.0 - t) * (1.0 - t) * t;
                let f2 = 3.0 * (1.0 - t) * t * t;
                let mt = 1.0 - t;

                // B0 evaluation
                let b0x = mt * mt * (1.0 + 2.0 * t) * q0.0 + t * t * (3.0 - 2.0 * t) * q_last.0;
                let b0y = mt * mt * (1.0 + 2.0 * t) * q0.1 + t * t * (3.0 - 2.0 * t) * q_last.1;

                let ex = pt.0 - b0x;
                let ey = pt.1 - b0y;

                r1 += f1 * (t0.0 * ex + t0.1 * ey);
                r2 += -f2 * (tm.0 * ex + tm.1 * ey);
            }

            let det = m11 * m22 - m12 * m12;
            if det.abs() < 1e-9 {
                return fallback();
            }

            let alpha = (r1 * m22 - r2 * m12) / det;
            let beta = (m11 * r2 - m12 * r1) / det;

            if alpha <= 0.0 || beta <= 0.0 {
                return fallback();
            }

            let c1 = (q0.0 + alpha * t0.0, q0.1 + alpha * t0.1);
            let c2 = (q_last.0 - beta * tm.0, q_last.1 - beta * tm.1);

            (c1, c2)
        }
    }
}

/// Sanitizes a path element, ensuring all its coordinates are finite.
/// If any coordinate is non-finite, falls back to a straight `LineTo` to the `fallback_end` point.
fn sanitize_element(elem: PathElement, fallback_end: (f64, f64)) -> PathElement {
    match elem {
        PathElement::MoveTo(x, y) => {
            if x.is_finite() && y.is_finite() {
                PathElement::MoveTo(x, y)
            } else {
                PathElement::MoveTo(fallback_end.0, fallback_end.1)
            }
        }
        PathElement::LineTo(x, y) => {
            if x.is_finite() && y.is_finite() {
                PathElement::LineTo(x, y)
            } else {
                PathElement::LineTo(fallback_end.0, fallback_end.1)
            }
        }
        PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
            if x1.is_finite()
                && y1.is_finite()
                && x2.is_finite()
                && y2.is_finite()
                && x3.is_finite()
                && y3.is_finite()
            {
                PathElement::CurveTo(x1, y1, x2, y2, x3, y3)
            } else {
                PathElement::LineTo(fallback_end.0, fallback_end.1)
            }
        }
        PathElement::ClosePath => PathElement::ClosePath,
    }
}
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
