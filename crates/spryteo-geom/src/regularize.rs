use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegularizeConfig {
    /// Grid pitch in the curves' coordinate space. Caller-supplied, NEVER
    /// inferred -- at 45px source resolution a 24-unit grid is
    /// indistinguishable from a 25-unit one (4%, below the anti-aliasing
    /// noise floor), so guessing it would be false precision.
    pub grid_pitch: f64,
    pub max_grid_dev: f64,      // default 1.5
    pub max_angle_dev_deg: f64, // default 6.0
    pub weld_eps: f64,          // default 1.0
    pub close_eps: f64,         // default 1.2
    pub fit_tol: f64,           // default 0.8
    pub width_rel_tol: f64,     // default 0.15
    pub width_quantum: f64,     // default 0.25
    pub snaps_deg: Vec<f64>,
    /// Minimum segment length eligible for angle snapping. Shorter segments
    /// are curve samples, not design lines -- see the angle pass.
    pub min_snap_segment: f64, // default [0, 45, 90, 135]
}

impl Default for RegularizeConfig {
    fn default() -> Self {
        Self {
            grid_pitch: 1.0,
            max_grid_dev: 1.5,
            max_angle_dev_deg: 6.0,
            weld_eps: 1.0,
            close_eps: 1.2,
            fit_tol: 0.8,
            width_rel_tol: 0.15,
            width_quantum: 0.25,
            snaps_deg: vec![0.0, 45.0, 90.0, 135.0],
            // 3.0 in a 24-unit canvas: an eighth of the icon. Traced curve
            // segments are well under this; real strokes are well over.
            min_snap_segment: 3.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegularizeReport {
    pub welded: usize,
    pub loops_closed: usize,
    pub lines_fitted: usize,
    pub arcs_fitted: usize,
    pub angles_snapped: usize,
    pub points_gridded: usize,
    pub widths_unified: bool,
    pub mirror_axis: Option<f64>,
    /// Largest distance any single point moved.
    pub max_displacement: f64,
    /// 1.0 - max_displacement/max_grid_dev, clamped to 0..=1.
    pub confidence: f32,
}

fn extract_curve_points(curve: &Curve) -> Vec<(f64, f64)> {
    let mut pts = Vec::new();
    for seg in &curve.segments {
        match seg {
            PathElement::MoveTo(x, y) | PathElement::LineTo(x, y) => pts.push((*x, *y)),
            PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                pts.push((*x1, *y1));
                pts.push((*x2, *y2));
                pts.push((*x3, *y3));
            }
            PathElement::ClosePath => {}
        }
    }
    pts
}

/// Regularizes in place. Returns the report.
pub fn regularize(
    curves: &mut [Curve],
    widths: &mut [f64],
    cfg: &RegularizeConfig,
) -> RegularizeReport {
    let mut max_displacement = 0.0_f64;
    let initial_pts: Vec<Vec<(f64, f64)>> = curves.iter().map(extract_curve_points).collect();

    // Pass 1: Weld endpoints FIRST so that greedy path traversal junctions sharing near-identical coordinates are merged before any geometry modification or snapping.
    let welded = weld_endpoints(curves, cfg.weld_eps);

    // Pass 2: Close loops on open curves whose end points are within close_eps of start points, securing closed topology before primitive recognition.
    let mut loops_closed = 0;
    for curve in curves.iter_mut() {
        if close_loops(curve, cfg.close_eps) {
            loops_closed += 1;
        }
    }

    // Pass 3: Fit primitives BEFORE snapping so that whole shape parameters (e.g. circle center and radius) are recognized from clean unsnapped points, avoiding deforming primitives into lumpy polygons.
    let mut lines_fitted = 0;
    let mut arcs_fitted = 0;
    for curve in curves.iter_mut() {
        if curve.primitive.is_none() {
            let pts = extract_curve_points(curve);
            if pts.len() >= 2 {
                if let Some((p1, p2)) = fit_line(&pts, cfg.fit_tol) {
                    let vx = p2.0 - p1.0;
                    let vy = p2.1 - p1.1;
                    let len_sq = vx * vx + vy * vy;
                    if len_sq > 1e-12 {
                        for &(px, py) in &pts {
                            let t =
                                (((px - p1.0) * vx + (py - p1.1) * vy) / len_sq).clamp(0.0, 1.0);
                            let proj = (p1.0 + t * vx, p1.1 + t * vy);
                            let dist = (px - proj.0).hypot(py - proj.1);
                            if dist > max_displacement {
                                max_displacement = dist;
                            }
                        }
                    }
                    let is_closed = matches!(curve.segments.last(), Some(PathElement::ClosePath));
                    curve.segments = vec![
                        PathElement::MoveTo(p1.0, p1.1),
                        PathElement::LineTo(p2.0, p2.1),
                    ];
                    if is_closed {
                        curve.segments.push(PathElement::ClosePath);
                    }
                    lines_fitted += 1;
                } else if let Some(prim) = fit_arc(&pts, cfg.fit_tol) {
                    if let Primitive::Arc { cx, cy, rx, .. } = prim {
                        for &(px, py) in &pts {
                            let dist = (((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - rx).abs();
                            if dist > max_displacement {
                                max_displacement = dist;
                            }
                        }
                    }
                    curve.primitive = Some(prim);
                    arcs_fitted += 1;
                } else if let Some((cx, cy, r)) = fit_circle(&pts, cfg.fit_tol) {
                    for &(px, py) in &pts {
                        let dist = (((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r).abs();
                        if dist > max_displacement {
                            max_displacement = dist;
                        }
                    }
                    curve.primitive = Some(Primitive::Circle { cx, cy, r });
                    arcs_fitted += 1;
                }
            }
        }
    }

    // Pass 4: Snap segment angles to target angles pivoting about segment midpoints.
    let mut angles_snapped = 0;
    for curve in curves.iter_mut() {
        let n_segs = curve.segments.len();
        if n_segs < 2 {
            continue;
        }

        // Never angle-snap a CLOSED curve. A closed loop is a shape -- a
        // circle, a rounded rect -- and its edges are not independent design
        // lines; they are a boundary being sampled. Rotating each edge onto
        // the nearest of 0/45/90/135 squares the shape off. Measured on the
        // real sheet, this is what turned the small circular heads in the group
        // icons octagonal while leaving the large open body curves untouched:
        // a small loop's few edges are each long enough to clear any absolute
        // length gate, so only the closed/open distinction separates them.
        // Shapes get regularised through primitive recognition instead.
        let is_closed = curve
            .segments
            .iter()
            .any(|e| matches!(e, PathElement::ClosePath))
            || match (curve.segments.first(), curve.segments.last()) {
                (
                    Some(PathElement::MoveTo(x0, y0)),
                    Some(PathElement::LineTo(x1, y1) | PathElement::CurveTo(_, _, _, _, x1, y1)),
                ) => (x1 - x0).hypot(y1 - y0) < 1e-6,
                _ => false,
            };
        if is_closed {
            continue;
        }

        for i in 1..n_segs {
            let (p_start, p_end) = match (&curve.segments[i - 1], &curve.segments[i]) {
                (
                    PathElement::MoveTo(x0, y0)
                    | PathElement::LineTo(x0, y0)
                    | PathElement::CurveTo(_, _, _, _, x0, y0),
                    PathElement::LineTo(x1, y1),
                ) => ((*x0, *y0), (*x1, *y1)),
                _ => continue,
            };

            // Only segments long enough to be deliberate lines may be rotated
            // onto a snap angle. A traced curve is a chain of SHORT segments
            // whose directions sample a smooth turn; forcing each of those onto
            // the nearest of 0/45/90/135 quantises the turn into facets.
            // Measured on the real sheet, this is what turned the small
            // circular heads in the group icons into visible hexagons -- the
            // grid pass was innocent, this was the culprit. A real design line
            // spans a meaningful fraction of the icon, so require that.
            if (p_end.0 - p_start.0).hypot(p_end.1 - p_start.1) < cfg.min_snap_segment {
                continue;
            }

            let (na, nb) = snap_angle(p_start, p_end, &cfg.snaps_deg, cfg.max_angle_dev_deg);
            if (na.0 - p_start.0).hypot(na.1 - p_start.1) > 1e-9
                || (nb.0 - p_end.0).hypot(nb.1 - p_end.1) > 1e-9
            {
                angles_snapped += 1;
                let d_a = (na.0 - p_start.0).hypot(na.1 - p_start.1);
                let d_b = (nb.0 - p_end.0).hypot(nb.1 - p_end.1);
                if d_a > max_displacement {
                    max_displacement = d_a;
                }
                if d_b > max_displacement {
                    max_displacement = d_b;
                }

                match &mut curve.segments[i - 1] {
                    PathElement::MoveTo(x, y)
                    | PathElement::LineTo(x, y)
                    | PathElement::CurveTo(_, _, _, _, x, y) => {
                        *x = na.0;
                        *y = na.1;
                    }
                    PathElement::ClosePath => {}
                }
                if let PathElement::LineTo(x, y) = &mut curve.segments[i] {
                    *x = nb.0;
                    *y = nb.1;
                }
            }
        }
    }

    // Pass 5: Snap points and recognized primitives to grid pitch.
    let mut points_gridded = 0;
    if cfg.grid_pitch > 0.0 {
        for curve in curves.iter_mut() {
            if let Some(ref mut prim) = curve.primitive {
                match prim {
                    Primitive::Circle {
                        ref mut cx,
                        ref mut cy,
                        ref mut r,
                    } => {
                        let (nx, ny) = snap_to_grid((*cx, *cy), cfg.grid_pitch, cfg.max_grid_dev);
                        if (nx - *cx).hypot(ny - *cy) > 1e-9 {
                            let d = (nx - *cx).hypot(ny - *cy);
                            if d > max_displacement {
                                max_displacement = d;
                            }
                            *cx = nx;
                            *cy = ny;
                            points_gridded += 1;
                        }
                        let nr = (*r / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (nr - *r).abs() <= cfg.max_grid_dev && (nr - *r).abs() > 1e-9 {
                            let d = (nr - *r).abs();
                            if d > max_displacement {
                                max_displacement = d;
                            }
                            *r = nr;
                        }
                    }
                    Primitive::Ellipse {
                        ref mut cx,
                        ref mut cy,
                        ref mut rx,
                        ref mut ry,
                        ..
                    } => {
                        let (nx, ny) = snap_to_grid((*cx, *cy), cfg.grid_pitch, cfg.max_grid_dev);
                        if (nx - *cx).hypot(ny - *cy) > 1e-9 {
                            let d = (nx - *cx).hypot(ny - *cy);
                            if d > max_displacement {
                                max_displacement = d;
                            }
                            *cx = nx;
                            *cy = ny;
                            points_gridded += 1;
                        }
                        let nrx = (*rx / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (nrx - *rx).abs() <= cfg.max_grid_dev && (nrx - *rx).abs() > 1e-9 {
                            *rx = nrx;
                        }
                        let nry = (*ry / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (nry - *ry).abs() <= cfg.max_grid_dev && (nry - *ry).abs() > 1e-9 {
                            *ry = nry;
                        }
                    }
                    Primitive::Rect {
                        ref mut x,
                        ref mut y,
                        ref mut width,
                        ref mut height,
                        ..
                    } => {
                        let (nx, ny) = snap_to_grid((*x, *y), cfg.grid_pitch, cfg.max_grid_dev);
                        if (nx - *x).hypot(ny - *y) > 1e-9 {
                            let d = (nx - *x).hypot(ny - *y);
                            if d > max_displacement {
                                max_displacement = d;
                            }
                            *x = nx;
                            *y = ny;
                            points_gridded += 1;
                        }
                        let nx2 = ((*x + *width) / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (nx2 - (*x + *width)).abs() <= cfg.max_grid_dev
                            && (nx2 - (*x + *width)).abs() > 1e-9
                        {
                            *width = nx2 - *x;
                            points_gridded += 1;
                        }
                        let ny2 = ((*y + *height) / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (ny2 - (*y + *height)).abs() <= cfg.max_grid_dev
                            && (ny2 - (*y + *height)).abs() > 1e-9
                        {
                            *height = ny2 - *y;
                            points_gridded += 1;
                        }
                    }
                    Primitive::Arc {
                        ref mut cx,
                        ref mut cy,
                        ref mut rx,
                        ref mut ry,
                        ..
                    } => {
                        let (nx, ny) = snap_to_grid((*cx, *cy), cfg.grid_pitch, cfg.max_grid_dev);
                        if (nx - *cx).hypot(ny - *cy) > 1e-9 {
                            let d = (nx - *cx).hypot(ny - *cy);
                            if d > max_displacement {
                                max_displacement = d;
                            }
                            *cx = nx;
                            *cy = ny;
                            points_gridded += 1;
                        }
                        let nrx = (*rx / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (nrx - *rx).abs() <= cfg.max_grid_dev && (nrx - *rx).abs() > 1e-9 {
                            *rx = nrx;
                        }
                        let nry = (*ry / cfg.grid_pitch).round() * cfg.grid_pitch;
                        if (nry - *ry).abs() <= cfg.max_grid_dev && (nry - *ry).abs() > 1e-9 {
                            *ry = nry;
                        }
                    }
                }
            }

            // Only geometry whose points ARE design-grid positions may be
            // snapped. A recognised primitive qualifies (its centre/corners
            // were placed on the grid by whoever drew it) and so does a pure
            // polyline (its vertices are real corners). A free-form traced
            // curve does NOT: its Bezier control points encode CURVATURE, not
            // position, and rounding them to the grid pulls smooth arcs into
            // visible lumps. Measured on the real sheet, gridding control
            // points turned every circular icon -- the magnifier, the bell,
            // the speech bubble -- visibly scalloped, which is strictly worse
            // than leaving them alone.
            // Raw path segments are NEVER gridded -- not their anchors, not
            // their Bezier control points. Both are SAMPLES OF A CURVE rather
            // than positions anyone placed on a design grid, so rounding them
            // quantises the path's shape into facets.
            //
            // Recognised primitives are still gridded, but through their
            // PARAMETERS above (a circle's centre and radius, a rect's
            // corners) -- which is the whole reason primitive recognition runs
            // before this pass. Note that recognition SETS curve.primitive, so
            // gating this loop on `primitive.is_none()` does not work: the
            // curves most likely to be deformed are exactly the ones that just
            // acquired a primitive. Measured on the real sheet at pitch 1.0,
            // gridding segments scalloped every circular icon and squared off
            // the small heads in the group icons, while leaving the large open
            // body curves untouched.
        }
    }

    // Measure overall point displacement from initial positions
    for (i, curve) in curves.iter().enumerate() {
        if i < initial_pts.len() {
            let final_pts = extract_curve_points(curve);
            for (p_init, p_final) in initial_pts[i].iter().zip(final_pts.iter()) {
                let d = (p_final.0 - p_init.0).hypot(p_final.1 - p_init.1);
                if d > max_displacement {
                    max_displacement = d;
                }
            }
        }
    }

    // Pass 6: Detect vertical mirror axis (record only, do not mutate geometry).
    let mirror_axis = detect_mirror_axis(curves, cfg.fit_tol);

    // Pass 7: Unify stroke widths across curves that cluster within relative tolerance.
    let widths_unified = unify_widths(widths, cfg.width_rel_tol, cfg.width_quantum);

    let confidence = if cfg.max_grid_dev <= 0.0 {
        0.0
    } else {
        (1.0 - max_displacement / cfg.max_grid_dev).clamp(0.0, 1.0) as f32
    };

    RegularizeReport {
        welded,
        loops_closed,
        lines_fitted,
        arcs_fitted,
        angles_snapped,
        points_gridded,
        widths_unified,
        mirror_axis,
        max_displacement,
        confidence,
    }
}
#[cfg(test)]
#[path = "regularize_tests.rs"]
mod tests;
