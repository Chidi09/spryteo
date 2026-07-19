use spryteo_core::ir::{Curve, PathElement};

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

fn is_admissible_open(points: &[(f64, f64)], start: usize, end: usize, tolerance: f64) -> bool {
    if end <= start + 1 {
        return true;
    }
    let a = points[start];
    let b = points[end];
    for &p in points.iter().take(end).skip(start + 1) {
        if perpendicular_distance(p, a, b) > tolerance {
            return false;
        }
    }
    true
}

fn greedy_polygon_open(points: &[(f64, f64)], tolerance: f64) -> Vec<usize> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    let mut vertices = vec![0];
    let mut curr = 0;
    while curr < n - 1 {
        let mut next_val = curr + 1;
        for candidate in (curr + 2..n).rev() {
            if is_admissible_open(points, curr, candidate, tolerance) {
                next_val = candidate;
                break;
            }
        }
        vertices.push(next_val);
        curr = next_val;
    }
    vertices
}

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

fn fit_unconstrained_bezier(points: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
    let m = points.len();
    let q0 = points[0];
    let q_last = points[m - 1];

    let dx = q_last.0 - q0.0;
    let dy = q_last.1 - q0.1;
    let chord_len = (dx * dx + dy * dy).sqrt();

    let fallback = || {
        let c1 = (q0.0 + dx / 3.0, q0.1 + dy / 3.0);
        let c2 = (q0.0 + 2.0 * dx / 3.0, q0.1 + 2.0 * dy / 3.0);
        (c1, c2)
    };

    if m <= 2 || chord_len < 1e-9 {
        return fallback();
    }

    let params = chord_length_parameterization(points);

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

fn fit_span_bezier(points: &[(f64, f64)], tolerance: f64, elements: &mut Vec<PathElement>) {
    if points.len() <= 2 {
        let q_last = points.last().unwrap();
        elements.push(PathElement::LineTo(q_last.0, q_last.1));
        return;
    }

    let (c1, c2) = fit_unconstrained_bezier(points);
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

    if max_dev <= tolerance {
        if c1.0.is_finite()
            && c1.1.is_finite()
            && c2.0.is_finite()
            && c2.1.is_finite()
            && q_last.0.is_finite()
            && q_last.1.is_finite()
        {
            elements.push(PathElement::CurveTo(
                c1.0, c1.1, c2.0, c2.1, q_last.0, q_last.1,
            ));
        } else {
            elements.push(PathElement::LineTo(q_last.0, q_last.1));
        }
    } else {
        let mid = points.len() / 2;
        fit_span_bezier(&points[..=mid], tolerance, elements);
        fit_span_bezier(&points[mid..], tolerance, elements);
    }
}

/// Smoothes an open pixel coordinate chain into a Curve with MoveTo and LineTo/CurveTo segments.
pub fn smooth_chain(chain: &[(f64, f64)], tolerance: f32) -> Curve {
    let mut elements = Vec::new();
    if !chain.is_empty() {
        elements.push(PathElement::MoveTo(chain[0].0, chain[0].1));
        let vertices = greedy_polygon_open(chain, tolerance as f64);
        for window in vertices.windows(2) {
            let start = window[0];
            let end = window[1];
            fit_span_bezier(&chain[start..=end], tolerance as f64, &mut elements);
        }
    }
    Curve {
        segments: elements,
        primitive: None,
    }
}
