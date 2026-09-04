//! Endpoint extension for open strokes.
//!
//! Distance transform interpretation: `compute_distance_transform` returns `Vec<f64>`.
//! Its values represent the Euclidean distance in pixels from each ink pixel to the nearest
//! non-ink (background) pixel, indexed by `y * width + x`.

/// Extends the free ends of an open polyline chain outward along its local
/// tangent, compensating for the medial axis stopping ~one stroke radius short
/// of a rounded cap.
///
/// `chain` is in pixel coordinates. `dist` is the distance transform, indexed
/// `y * width + x`. Mutates `chain` in place. Returns the number of endpoints
/// actually moved (0, 1, or 2).
#[allow(clippy::ptr_arg)]
pub fn extend_endpoints(
    chain: &mut Vec<(f64, f64)>,
    dist: &[f64],
    ink: &[bool],
    width: u32,
    height: u32,
) -> usize {
    if width == 0 || height == 0 || chain.len() < 5 {
        return 0;
    }

    // 1. Closed chains are skipped entirely (within 1e-9).
    let p_first = chain[0];
    let p_last = chain[chain.len() - 1];
    let dx_close = p_first.0 - p_last.0;
    let dy_close = p_first.1 - p_last.1;
    if (dx_close * dx_close + dy_close * dy_close).sqrt() < 1e-9 {
        return 0;
    }

    // 2. Fit TLS tangents and coarse directions for start and end endpoints independently.
    let n = 5;
    let pts_start = &chain[0..n];
    let coarse_start = (chain[0].0 - chain[n - 1].0, chain[0].1 - chain[n - 1].1);
    let v_start = compute_tls_tangent(pts_start, coarse_start);

    let len = chain.len();
    let pts_end = &chain[len - n..len];
    let coarse_end = (
        chain[len - 1].0 - chain[len - n].0,
        chain[len - 1].1 - chain[len - n].1,
    );
    let v_end = compute_tls_tangent(pts_end, coarse_end);

    // 3. Compute extensions for start and end endpoints.
    let new_start = extend_single_endpoint(p_first, v_start, dist, ink, width, height);
    let new_end = extend_single_endpoint(p_last, v_end, dist, ink, width, height);

    // 4. Apply mutations and count endpoints actually moved.
    let mut moved = 0;
    if let Some(ns) = new_start {
        chain[0] = ns;
        moved += 1;
    }
    if let Some(ne) = new_end {
        chain[len - 1] = ne;
        moved += 1;
    }

    moved
}

fn compute_tls_tangent(pts: &[(f64, f64)], coarse: (f64, f64)) -> Option<(f64, f64)> {
    let n = pts.len() as f64;
    if n < 2.0 {
        return None;
    }
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;

    let mut cxx = 0.0;
    let mut cxy = 0.0;
    let mut cyy = 0.0;
    for p in pts {
        let dx = p.0 - mx;
        let dy = p.1 - my;
        cxx += dx * dx;
        cxy += dx * dy;
        cyy += dy * dy;
    }

    let trace = cxx + cyy;
    let det = cxx * cyy - cxy * cxy;
    let disc = (trace * trace / 4.0 - det).max(0.0);
    let lambda1 = trace / 2.0 + disc.sqrt();

    let (mut vx, mut vy) = if cxy.abs() > 1e-12 {
        (cxy, lambda1 - cxx)
    } else if cxx >= cyy {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };

    let norm = (vx * vx + vy * vy).sqrt();
    if norm < 1e-12 {
        return None;
    }
    vx /= norm;
    vy /= norm;

    // Orient outward: dot product with coarse direction vector must be >= 0
    let dot = vx * coarse.0 + vy * coarse.1;
    if dot < 0.0 {
        vx = -vx;
        vy = -vy;
    }

    Some((vx, vy))
}

fn extend_single_endpoint(
    p0: (f64, f64),
    v: Option<(f64, f64)>,
    dist: &[f64],
    ink: &[bool],
    width: u32,
    height: u32,
) -> Option<(f64, f64)> {
    let dir = v?;
    let px = p0.0.round() as i32;
    let py = p0.1.round() as i32;

    if px < 0 || px >= width as i32 || py < 0 || py >= height as i32 {
        return None;
    }

    let idx = (py * width as i32 + px) as usize;
    if idx >= dist.len() {
        return None;
    }

    let dt_val = dist[idx];
    if dt_val <= 0.5 {
        return None;
    }

    let r = dt_val.min(8.0);
    let max_x = (width - 1) as f64;
    let max_y = (height - 1) as f64;

    let mut t = 0.25;
    let mut best_pos = None;

    while t <= r + 1e-9 {
        let step_t = t.min(r);
        let cand_x = (p0.0 + step_t * dir.0).clamp(0.0, max_x);
        let cand_y = (p0.1 + step_t * dir.1).clamp(0.0, max_y);
        let cx = cand_x.round() as i32;
        let cy = cand_y.round() as i32;

        if cx < 0 || cx >= width as i32 || cy < 0 || cy >= height as i32 {
            break;
        }

        let c_idx = (cy * width as i32 + cx) as usize;
        if c_idx >= ink.len() || !ink[c_idx] {
            break;
        }

        best_pos = Some((cand_x, cand_y));

        if (step_t - r).abs() < 1e-9 {
            break;
        }
        t += 0.25;
    }

    if let Some(pos) = best_pos {
        let dist_moved = (pos.0 - p0.0).hypot(pos.1 - p0.1);
        if dist_moved > 1e-9 {
            return Some(pos);
        }
    }

    None
}
#[cfg(test)]
#[path = "caps_tests.rs"]
mod tests;
