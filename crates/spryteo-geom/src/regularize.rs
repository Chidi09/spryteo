use spryteo_core::ir::{Curve, PathElement, Primitive};

/// Total-least-squares line fit. Returns the two endpoints of the fitted
/// segment (the extreme projections of the input onto the fitted direction),
/// or None when the points are not collinear within `tol` or there are < 2.
pub fn fit_line(points: &[(f64, f64)], tol: f64) -> Option<((f64, f64), (f64, f64))> {
    if points.len() < 2 {
        return None;
    }

    let n = points.len() as f64;
    let cx: f64 = points.iter().map(|p| p.0).sum::<f64>() / n;
    let cy: f64 = points.iter().map(|p| p.1).sum::<f64>() / n;

    let mut cxx = 0.0;
    let mut cyy = 0.0;
    let mut cxy = 0.0;

    for &(x, y) in points {
        let dx = x - cx;
        let dy = y - cy;
        cxx += dx * dx;
        cyy += dy * dy;
        cxy += dx * dy;
    }

    let trace = cxx + cyy;
    let disc = ((cxx - cyy) * (cxx - cyy) + 4.0 * cxy * cxy)
        .max(0.0)
        .sqrt();
    let lambda1 = (trace + disc) / 2.0;

    let (vx, vy) = if cxy.abs() > 1e-12 {
        (cxy, lambda1 - cxx)
    } else if cxx >= cyy {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };

    let len = (vx * vx + vy * vy).sqrt();
    if len < 1e-12 || !len.is_finite() {
        return None;
    }
    let (ux, uy) = (vx / len, vy / len);

    let mut max_dist = 0.0_f64;
    let mut t_min = f64::INFINITY;
    let mut t_max = f64::NEG_INFINITY;

    for &(x, y) in points {
        let dx = x - cx;
        let dy = y - cy;
        let perp_dist = (-dx * uy + dy * ux).abs();
        if perp_dist > max_dist {
            max_dist = perp_dist;
        }
        let t = dx * ux + dy * uy;
        if t < t_min {
            t_min = t;
        }
        if t > t_max {
            t_max = t;
        }
    }

    if max_dist > tol || !max_dist.is_finite() || !t_min.is_finite() || !t_max.is_finite() {
        return None;
    }

    let p1 = (cx + t_min * ux, cy + t_min * uy);
    let p2 = (cx + t_max * ux, cy + t_max * uy);
    Some((p1, p2))
}

/// Algebraic (Kasa) circle fit. Returns (cx, cy, r), or None when the radial
/// residual exceeds `tol` or there are < 3 points.
pub fn fit_circle(points: &[(f64, f64)], tol: f64) -> Option<(f64, f64, f64)> {
    if points.len() < 3 {
        return None;
    }

    let n = points.len() as f64;

    let s_x: f64 = points.iter().map(|p| p.0).sum();
    let s_y: f64 = points.iter().map(|p| p.1).sum();
    let s_xx: f64 = points.iter().map(|p| p.0 * p.0).sum();
    let s_yy: f64 = points.iter().map(|p| p.1 * p.1).sum();
    let s_xy: f64 = points.iter().map(|p| p.0 * p.1).sum();
    let s_xz: f64 = points.iter().map(|p| p.0 * (p.0 * p.0 + p.1 * p.1)).sum();
    let s_yz: f64 = points.iter().map(|p| p.1 * (p.0 * p.0 + p.1 * p.1)).sum();
    let s_zz: f64 = points.iter().map(|p| p.0 * p.0 + p.1 * p.1).sum();

    let det = n * (s_xx * s_yy - s_xy * s_xy) - s_x * (s_x * s_yy - s_xy * s_y)
        + s_y * (s_x * s_xy - s_xx * s_y);

    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }

    let r0 = -s_zz;
    let r1 = -s_xz;
    let r2 = -s_yz;

    let det_c = r0 * (s_xx * s_yy - s_xy * s_xy) - s_x * (r1 * s_yy - s_xy * r2)
        + s_y * (r1 * s_xy - s_xx * r2);

    let det_a =
        n * (r1 * s_yy - s_xy * r2) - r0 * (s_x * s_yy - s_xy * s_y) + s_y * (s_x * r2 - r1 * s_y);

    let det_b =
        n * (s_xx * r2 - s_xy * r1) - s_x * (s_x * r2 - r1 * s_y) + r0 * (s_x * s_xy - s_xx * s_y);

    let cx = -det_a / (2.0 * det);
    let cy = -det_b / (2.0 * det);
    let r2_val = cx * cx + cy * cy - det_c / det;
    if r2_val <= 0.0 || !r2_val.is_finite() {
        return None;
    }
    let r = r2_val.sqrt();

    let max_resid = points
        .iter()
        .map(|&(x, y)| (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs())
        .fold(0.0_f64, f64::max);

    if max_resid <= tol && max_resid.is_finite() {
        Some((cx, cy, r))
    } else {
        None
    }
}

/// Fits a circular arc: circle fit, then the angular sweep of the points.
/// Returns Primitive::Arc, or None when the circle fit fails or the sweep is
/// so close to a full turn that a Circle is the better description.
pub fn fit_arc(points: &[(f64, f64)], tol: f64) -> Option<Primitive> {
    if points.len() < 3 {
        return None;
    }

    let (cx, cy, r) = fit_circle(points, tol)?;

    let mut unwrapped = Vec::with_capacity(points.len());
    let first_theta = (points[0].1 - cy).atan2(points[0].0 - cx);
    unwrapped.push(first_theta);

    for i in 1..points.len() {
        let theta = (points[i].1 - cy).atan2(points[i].0 - cx);
        let mut diff = theta - unwrapped[i - 1];
        while diff > std::f64::consts::PI {
            diff -= 2.0 * std::f64::consts::PI;
        }
        while diff <= -std::f64::consts::PI {
            diff += 2.0 * std::f64::consts::PI;
        }
        unwrapped.push(unwrapped[i - 1] + diff);
    }

    let min_angle = unwrapped.iter().copied().fold(f64::INFINITY, f64::min);
    let max_angle = unwrapped.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let sweep = max_angle - min_angle;

    if sweep >= 350.0_f64.to_radians() || !sweep.is_finite() {
        return None;
    }

    let start_angle = unwrapped[0];
    let end_angle = unwrapped[unwrapped.len() - 1];

    Some(Primitive::Arc {
        cx,
        cy,
        rx: r,
        ry: r,
        start_angle,
        end_angle,
        rotation: 0.0,
    })
}

/// Rotates the segment a->b onto the nearest angle in `snaps_deg` when it is
/// within `max_dev_deg` of one, pivoting about the segment's MIDPOINT so the
/// segment does not drift. Returns the (possibly unchanged) endpoints.
pub fn snap_angle(
    a: (f64, f64),
    b: (f64, f64),
    snaps_deg: &[f64],
    max_dev_deg: f64,
) -> ((f64, f64), (f64, f64)) {
    if snaps_deg.is_empty() {
        return (a, b);
    }

    let vx = b.0 - a.0;
    let vy = b.1 - a.1;
    let len = vx.hypot(vy);
    if len < 1e-12 || !len.is_finite() {
        return (a, b);
    }

    let theta_deg = vy.atan2(vx).to_degrees();

    let mut best_dev = f64::INFINITY;
    let mut best_target_deg = theta_deg;

    for &s in snaps_deg {
        let mut d = (theta_deg - s) % 180.0;
        while d > 90.0 {
            d -= 180.0;
        }
        while d <= -90.0 {
            d += 180.0;
        }
        let dev = d.abs();
        if dev < best_dev {
            best_dev = dev;
            best_target_deg = theta_deg - d;
        }
    }

    if best_dev <= max_dev_deg && best_dev.is_finite() {
        let target_rad = best_target_deg.to_radians();
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let half_len = len / 2.0;
        let dx = half_len * target_rad.cos();
        let dy = half_len * target_rad.sin();
        ((mid.0 - dx, mid.1 - dy), (mid.0 + dx, mid.1 + dy))
    } else {
        (a, b)
    }
}

/// Snaps a point to the nearest multiple of `pitch` on each axis, but only
/// when the move is within `max_dev`. Returns the point unchanged otherwise.
pub fn snap_to_grid(p: (f64, f64), pitch: f64, max_dev: f64) -> (f64, f64) {
    if pitch <= 0.0 || !pitch.is_finite() || !max_dev.is_finite() {
        return p;
    }

    let sx = (p.0 / pitch).round() * pitch;
    let sy = (p.1 / pitch).round() * pitch;

    let dx = (p.0 - sx).abs();
    let dy = (p.1 - sy).abs();

    if dx <= max_dev && dy <= max_dev {
        (sx, sy)
    } else {
        p
    }
}

/// Endpoints of different curves that are within `eps` of each other should
/// share one exact coordinate. Collects all curve endpoints, groups those
/// within `eps`, and rewrites every member of a group to the group's centroid.
/// Returns how many endpoints were moved.
pub fn weld_endpoints(curves: &mut [Curve], eps: f64) -> usize {
    if curves.is_empty() || eps < 0.0 {
        return 0;
    }

    struct EndpointItem {
        point: (f64, f64),
        curve_idx: usize,
        elem_idx: usize,
        is_start: bool,
    }

    let mut items = Vec::new();

    for (c_idx, curve) in curves.iter().enumerate() {
        if curve.segments.is_empty() {
            continue;
        }

        let start_elem_idx = 0;
        let start_pt = match &curve.segments[start_elem_idx] {
            PathElement::MoveTo(x, y) => Some((*x, *y)),
            PathElement::LineTo(x, y) => Some((*x, *y)),
            PathElement::CurveTo(_, _, _, _, x3, y3) => Some((*x3, *y3)),
            PathElement::ClosePath => None,
        };

        let last_non_close = curve
            .segments
            .iter()
            .enumerate()
            .rposition(|(_, elem)| !matches!(elem, PathElement::ClosePath));

        if let Some(start_p) = start_pt {
            items.push(EndpointItem {
                point: start_p,
                curve_idx: c_idx,
                elem_idx: start_elem_idx,
                is_start: true,
            });
        }

        if let Some(end_elem_idx) = last_non_close {
            if end_elem_idx != start_elem_idx {
                let end_pt = match &curve.segments[end_elem_idx] {
                    PathElement::MoveTo(x, y) => Some((*x, *y)),
                    PathElement::LineTo(x, y) => Some((*x, *y)),
                    PathElement::CurveTo(_, _, _, _, x3, y3) => Some((*x3, *y3)),
                    PathElement::ClosePath => None,
                };

                if let Some(end_p) = end_pt {
                    items.push(EndpointItem {
                        point: end_p,
                        curve_idx: c_idx,
                        elem_idx: end_elem_idx,
                        is_start: false,
                    });
                }
            }
        }
    }

    if items.is_empty() {
        return 0;
    }

    items.sort_by(|a, b| {
        a.point
            .0
            .partial_cmp(&b.point.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.point
                    .1
                    .partial_cmp(&b.point.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.curve_idx.cmp(&b.curve_idx))
            .then_with(|| a.elem_idx.cmp(&b.elem_idx))
            .then_with(|| a.is_start.cmp(&b.is_start))
    });

    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(i: usize, parent: &mut [usize]) -> usize {
        let mut root = i;
        while root != parent[root] {
            root = parent[root];
        }
        let mut curr = i;
        while curr != root {
            let nxt = parent[curr];
            parent[curr] = root;
            curr = nxt;
        }
        root
    }

    fn union(i: usize, j: usize, parent: &mut [usize]) {
        let root_i = find(i, parent);
        let root_j = find(j, parent);
        if root_i != root_j {
            if root_i < root_j {
                parent[root_j] = root_i;
            } else {
                parent[root_i] = root_j;
            }
        }
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if items[j].point.0 - items[i].point.0 > eps {
                break;
            }
            let dx = items[i].point.0 - items[j].point.0;
            let dy = items[i].point.1 - items[j].point.1;
            if dx.hypot(dy) <= eps {
                union(i, j, &mut parent);
            }
        }
    }

    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for i in 0..n {
        let root = find(i, &mut parent);
        groups.entry(root).or_default().push(i);
    }

    let mut moved_count = 0;

    for idxs in groups.values() {
        if idxs.len() <= 1 {
            continue;
        }

        let sum_x: f64 = idxs.iter().map(|&i| items[i].point.0).sum();
        let sum_y: f64 = idxs.iter().map(|&i| items[i].point.1).sum();
        let centroid = (sum_x / idxs.len() as f64, sum_y / idxs.len() as f64);

        for &i in idxs {
            let item = &items[i];
            let orig = item.point;
            if (orig.0 - centroid.0).hypot(orig.1 - centroid.1) > 1e-12 {
                moved_count += 1;
            }

            let elem = &mut curves[item.curve_idx].segments[item.elem_idx];
            match elem {
                PathElement::MoveTo(ref mut x, ref mut y) => {
                    *x = centroid.0;
                    *y = centroid.1;
                }
                PathElement::LineTo(ref mut x, ref mut y) => {
                    *x = centroid.0;
                    *y = centroid.1;
                }
                PathElement::CurveTo(_, _, _, _, ref mut x3, ref mut y3) => {
                    *x3 = centroid.0;
                    *y3 = centroid.1;
                }
                PathElement::ClosePath => {}
            }
        }
    }

    moved_count
}

/// When a curve's first and last point are within `eps` but it does not end in
/// ClosePath, snap the last point onto the first and append ClosePath.
/// Returns true when the curve was changed.
pub fn close_loops(curve: &mut Curve, eps: f64) -> bool {
    if curve.segments.is_empty() {
        return false;
    }

    if matches!(curve.segments.last(), Some(PathElement::ClosePath)) {
        return false;
    }

    let first_pt = match &curve.segments[0] {
        PathElement::MoveTo(x, y) => (*x, *y),
        PathElement::LineTo(x, y) => (*x, *y),
        PathElement::CurveTo(_, _, _, _, x3, y3) => (*x3, *y3),
        PathElement::ClosePath => return false,
    };

    let last_elem_idx = curve.segments.len() - 1;
    let last_pt = match &curve.segments[last_elem_idx] {
        PathElement::MoveTo(x, y) => (*x, *y),
        PathElement::LineTo(x, y) => (*x, *y),
        PathElement::CurveTo(_, _, _, _, x3, y3) => (*x3, *y3),
        PathElement::ClosePath => return false,
    };

    let dist = (first_pt.0 - last_pt.0).hypot(first_pt.1 - last_pt.1);
    if dist <= eps {
        let last_elem = &mut curve.segments[last_elem_idx];
        match last_elem {
            PathElement::MoveTo(ref mut x, ref mut y) => {
                *x = first_pt.0;
                *y = first_pt.1;
            }
            PathElement::LineTo(ref mut x, ref mut y) => {
                *x = first_pt.0;
                *y = first_pt.1;
            }
            PathElement::CurveTo(_, _, _, _, ref mut x3, ref mut y3) => {
                *x3 = first_pt.0;
                *y3 = first_pt.1;
            }
            PathElement::ClosePath => {}
        }
        curve.segments.push(PathElement::ClosePath);
        true
    } else {
        false
    }
}

/// Detects a vertical mirror axis: the x such that reflecting every point
/// across it maps the set onto itself within `tol`. Returns None when no such
/// axis exists.
pub fn detect_mirror_axis(curves: &[Curve], tol: f64) -> Option<f64> {
    let mut points = Vec::new();
    for curve in curves {
        for elem in &curve.segments {
            match elem {
                PathElement::MoveTo(x, y) => points.push((*x, *y)),
                PathElement::LineTo(x, y) => points.push((*x, *y)),
                PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                    points.push((*x1, *y1));
                    points.push((*x2, *y2));
                    points.push((*x3, *y3));
                }
                PathElement::ClosePath => {}
            }
        }
    }

    if points.is_empty() {
        return None;
    }

    let sum_x: f64 = points.iter().map(|p| p.0).sum();
    let axis = sum_x / points.len() as f64;
    if !axis.is_finite() {
        return None;
    }

    for &(px, py) in &points {
        let rx = 2.0 * axis - px;
        let ry = py;

        let has_partner = points
            .iter()
            .any(|&(ox, oy)| (ox - rx).hypot(oy - ry) <= tol);

        if !has_partner {
            return None;
        }
    }

    Some(axis)
}

/// Stroke widths that cluster within `rel_tol` of each other were designed
/// identical. Replaces each cluster with its median, rounded to the nearest
/// `quantum`. Returns true when any width changed.
pub fn unify_widths(widths: &mut [f64], rel_tol: f64, quantum: f64) -> bool {
    if widths.is_empty() {
        return false;
    }

    let mut indexed: Vec<(f64, usize)> = widths
        .iter()
        .copied()
        .enumerate()
        .map(|(i, w)| (w, i))
        .collect();
    indexed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let n = indexed.len();
    let mut clusters: Vec<Vec<(f64, usize)>> = Vec::new();

    let rel_diff = |a: f64, b: f64| -> f64 {
        let max_val = a.max(b);
        if max_val <= 0.0 {
            if a == b {
                0.0
            } else {
                f64::INFINITY
            }
        } else {
            (a - b).abs() / max_val
        }
    };

    let mut current_cluster: Vec<(f64, usize)> = vec![indexed[0]];

    for i in 1..n {
        let prev_val = indexed[i - 1].0;
        let curr_val = indexed[i].0;

        if rel_diff(prev_val, curr_val) <= rel_tol {
            current_cluster.push(indexed[i]);
        } else {
            clusters.push(current_cluster);
            current_cluster = vec![indexed[i]];
        }
    }
    clusters.push(current_cluster);

    let mut changed = false;

    for cluster in clusters {
        let m = cluster.len();
        let median = if m % 2 == 1 {
            cluster[m / 2].0
        } else {
            (cluster[m / 2 - 1].0 + cluster[m / 2].0) / 2.0
        };

        let target_val = if quantum > 0.0 && quantum.is_finite() {
            (median / quantum).round() * quantum
        } else {
            median
        };

        for &(_orig_val, idx) in &cluster {
            if (widths[idx] - target_val).abs() > 1e-12 {
                widths[idx] = target_val;
                changed = true;
            }
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recognize;

    fn sample_circle_pts(cx: f64, cy: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

    fn sample_arc_pts(
        cx: f64,
        cy: f64,
        r: f64,
        start_deg: f64,
        end_deg: f64,
        n: usize,
    ) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        let start_rad = start_deg.to_radians();
        let end_rad = end_deg.to_radians();
        let total_sweep = if end_rad >= start_rad {
            end_rad - start_rad
        } else {
            end_rad + 2.0 * std::f64::consts::PI - start_rad
        };
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            let theta = start_rad + t * total_sweep;
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

    #[test]
    fn test_fit_line_horizontal() {
        let pts = vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)];
        let res = fit_line(&pts, 0.1).expect("fit_line horizontal should succeed");
        assert!((res.0 .0 - 0.0).abs() < 1e-6);
        assert!((res.0 .1 - 5.0).abs() < 1e-6);
        assert!((res.1 .0 - 10.0).abs() < 1e-6);
        assert!((res.1 .1 - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_fit_line_vertical() {
        let pts = vec![(5.0, 0.0), (5.0, 5.0), (5.0, 10.0)];
        let res = fit_line(&pts, 0.1).expect("fit_line vertical should succeed");
        assert!((res.0 .0 - 5.0).abs() < 1e-6);
        assert!((res.0 .1 - 0.0).abs() < 1e-6);
        assert!((res.1 .0 - 5.0).abs() < 1e-6);
        assert!((res.1 .1 - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_fit_line_rejects_non_collinear() {
        let pts = vec![(0.0, 0.0), (5.0, 5.0), (10.0, 0.0)];
        assert!(fit_line(&pts, 0.1).is_none());
    }

    #[test]
    fn test_fit_line_too_few_points() {
        assert!(fit_line(&[], 0.1).is_none());
        assert!(fit_line(&[(0.0, 0.0)], 0.1).is_none());
    }

    #[test]
    fn test_fit_circle_recovers_known_circle() {
        let pts = sample_circle_pts(20.0, 30.0, 15.0, 32);
        let (cx, cy, r) = fit_circle(&pts, 1e-4).expect("fit_circle should succeed");
        assert!((cx - 20.0).abs() < 1e-6);
        assert!((cy - 30.0).abs() < 1e-6);
        assert!((r - 15.0).abs() < 1e-6);
    }

    #[test]
    fn test_fit_circle_rejects_collinear_points() {
        let pts = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        assert!(fit_circle(&pts, 1.0).is_none());
    }

    #[test]
    fn test_fit_circle_too_few_points() {
        assert!(fit_circle(&[], 1.0).is_none());
        assert!(fit_circle(&[(0.0, 0.0), (1.0, 1.0)], 1.0).is_none());
    }

    #[test]
    fn test_fit_arc_quarter_circle() {
        let pts = sample_arc_pts(0.0, 0.0, 10.0, 0.0, 90.0, 16);
        let prim = fit_arc(&pts, 1e-4).expect("fit_arc quarter circle should succeed");
        match prim {
            Primitive::Arc {
                cx,
                cy,
                rx,
                ry,
                start_angle,
                end_angle,
                rotation,
            } => {
                assert!((cx - 0.0).abs() < 1e-4);
                assert!((cy - 0.0).abs() < 1e-4);
                assert!((rx - 10.0).abs() < 1e-4);
                assert!((ry - 10.0).abs() < 1e-4);
                assert!((rotation - 0.0).abs() < 1e-6);
                let sweep = (end_angle - start_angle).abs();
                assert!((sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-3);
            }
            other => panic!("expected Arc, got {other:?}"),
        }
    }

    #[test]
    fn test_fit_arc_across_pi_boundary() {
        let pts = sample_arc_pts(0.0, 0.0, 10.0, 170.0, -170.0, 16);
        let prim = fit_arc(&pts, 1e-4).expect("fit_arc across pi boundary should succeed");
        match prim {
            Primitive::Arc {
                start_angle,
                end_angle,
                ..
            } => {
                let sweep = (end_angle - start_angle).abs();
                let expected_sweep = 20.0_f64.to_radians();
                assert!(
                    (sweep - expected_sweep).abs() < 1e-3,
                    "expected ~20 deg sweep ({expected_sweep}), got {sweep}"
                );
            }
            other => panic!("expected Arc, got {other:?}"),
        }
    }

    #[test]
    fn test_fit_arc_full_circle_returns_none() {
        let pts = sample_circle_pts(0.0, 0.0, 10.0, 64);
        assert!(fit_arc(&pts, 1e-3).is_none());
    }

    #[test]
    fn test_snap_angle_snaps_near_horizontal() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 0.1);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 5.0);
        assert!(
            (na.1 - nb.1).abs() < 1e-6,
            "y-coordinates should match after snapping"
        );
    }

    #[test]
    fn test_snap_angle_leaves_far_angle_alone() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 3.0);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 2.0);
        assert_eq!((na, nb), (a, b));
    }

    #[test]
    fn test_snap_angle_preserves_length() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 0.5);
        let orig_len = (b.0 - a.0).hypot(b.1 - a.1);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 10.0);
        let new_len = (nb.0 - na.0).hypot(nb.1 - na.1);
        assert!((orig_len - new_len).abs() < 1e-6);
    }

    #[test]
    fn test_snap_angle_pivots_about_midpoint() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 0.5);
        let orig_mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 10.0);
        let new_mid = ((na.0 + nb.0) / 2.0, (na.1 + nb.1) / 2.0);
        assert!((orig_mid.0 - new_mid.0).abs() < 1e-6);
        assert!((orig_mid.1 - new_mid.1).abs() < 1e-6);
    }

    #[test]
    fn test_snap_to_grid_snaps_within_tolerance() {
        let p = (10.1, 20.05);
        let snapped = snap_to_grid(p, 1.0, 0.2);
        assert_eq!(snapped, (10.0, 20.0));
    }

    #[test]
    fn test_snap_to_grid_rejects_when_one_axis_too_far() {
        let p = (10.1, 20.5);
        let snapped = snap_to_grid(p, 1.0, 0.2);
        assert_eq!(snapped, p);
    }

    #[test]
    fn test_weld_endpoints_merges_nearby() {
        let mut curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(10.05, 0.0),
                    PathElement::LineTo(20.0, 0.0),
                ],
                primitive: None,
            },
        ];
        let moved = weld_endpoints(&mut curves, 0.1);
        assert_eq!(moved, 2);
        let end1 = match &curves[0].segments[1] {
            PathElement::LineTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        let start2 = match &curves[1].segments[0] {
            PathElement::MoveTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        assert_eq!(end1, start2);
        assert!((end1.0 - 10.025).abs() < 1e-6);
    }

    #[test]
    fn test_weld_endpoints_leaves_distant_alone() {
        let mut curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(10.5, 0.0),
                    PathElement::LineTo(20.0, 0.0),
                ],
                primitive: None,
            },
        ];
        let moved = weld_endpoints(&mut curves, 0.1);
        assert_eq!(moved, 0);
    }

    #[test]
    fn test_weld_endpoints_is_deterministic() {
        let make_curves = || {
            vec![
                Curve {
                    segments: vec![
                        PathElement::MoveTo(0.0, 0.0),
                        PathElement::LineTo(10.0, 0.0),
                    ],
                    primitive: None,
                },
                Curve {
                    segments: vec![
                        PathElement::MoveTo(10.04, 0.0),
                        PathElement::LineTo(20.0, 0.0),
                    ],
                    primitive: None,
                },
            ]
        };

        let mut c1 = make_curves();
        let mut c2 = make_curves();

        let m1 = weld_endpoints(&mut c1, 0.1);
        let m2 = weld_endpoints(&mut c2, 0.1);

        assert_eq!(m1, m2);
        assert_eq!(format!("{c1:?}"), format!("{c2:?}"));
    }

    #[test]
    fn test_close_loops_closes_near_loop() {
        let mut curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(0.05, 0.02),
            ],
            primitive: None,
        };
        let changed = close_loops(&mut curve, 0.1);
        assert!(changed);
        assert_eq!(curve.segments.len(), 4);
        assert!(matches!(
            curve.segments.last(),
            Some(PathElement::ClosePath)
        ));
        let last_line = match &curve.segments[2] {
            PathElement::LineTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        assert_eq!(last_line, (0.0, 0.0));
    }

    #[test]
    fn test_close_loops_ignores_open_curve() {
        let mut curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(5.0, 5.0),
            ],
            primitive: None,
        };
        let changed = close_loops(&mut curve, 0.1);
        assert!(!changed);
        assert_eq!(curve.segments.len(), 3);
    }

    #[test]
    fn test_close_loops_idempotent() {
        let mut curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(0.05, 0.0),
            ],
            primitive: None,
        };
        assert!(close_loops(&mut curve, 0.1));
        assert!(!close_loops(&mut curve, 0.1));
        assert_eq!(curve.segments.len(), 4);
    }

    #[test]
    fn test_detect_mirror_axis_finds_symmetric() {
        let curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(5.0, 0.0),
                    PathElement::LineTo(5.0, 10.0),
                ],
                primitive: None,
            },
        ];
        let axis = detect_mirror_axis(&curves, 0.1).expect("should find mirror axis");
        assert!((axis - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_detect_mirror_axis_none_for_asymmetric() {
        let curves = vec![Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(2.0, 8.0),
            ],
            primitive: None,
        }];
        assert!(detect_mirror_axis(&curves, 0.1).is_none());
    }

    #[test]
    fn test_unify_widths_merges_cluster() {
        let mut widths = vec![2.9, 3.1, 3.0];
        let changed = unify_widths(&mut widths, 0.1, 0.5);
        assert!(changed);
        assert_eq!(widths, vec![3.0, 3.0, 3.0]);
    }

    #[test]
    fn test_unify_widths_keeps_distinct() {
        let mut widths = vec![3.0, 8.0];
        let changed = unify_widths(&mut widths, 0.1, 1.0);
        assert!(!changed);
        assert_eq!(widths, vec![3.0, 8.0]);
    }

    #[test]
    fn test_unify_widths_zero_quantum_does_not_divide_by_zero() {
        let mut widths = vec![2.9, 3.1, 3.0];
        let changed = unify_widths(&mut widths, 0.1, 0.0);
        assert!(changed);
        assert_eq!(widths, vec![3.0, 3.0, 3.0]);
    }

    #[test]
    fn test_recognize_output_is_unchanged() {
        let pts = sample_circle_pts(50.0, 30.0, 20.0, 64);
        let prim = recognize(&pts, 1.0).expect("existing recognize should return circle");
        match prim {
            Primitive::Circle { cx, cy, r } => {
                assert!((cx - 50.0).abs() < 0.5);
                assert!((cy - 30.0).abs() < 0.5);
                assert!((r - 20.0).abs() < 0.5);
            }
            other => panic!("expected Circle, got {other:?}"),
        }
    }
}
