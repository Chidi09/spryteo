use std::fmt::Write;

use spryteo_core::ir::{Primitive, Rgb};

pub mod regularize;
// Named rather than glob-exported: this module and `recognize` below both deal
// in primitives, and a glob would silently make any future name collision
// ambiguous at the crate root.
pub use regularize::{
    close_loops, detect_mirror_axis, fit_arc, fit_circle, fit_line, snap_angle, snap_to_grid,
    unify_widths, weld_endpoints,
};

/// Number of decimal places used when rounding coordinates for stable ID
/// hashing.  Currently `3` (0.001 unit precision).
pub const HASH_PRECISION: u32 = 3;

// ── Helpers ────────────────────────────────────────────────────────────────

fn round_coord(v: f64) -> f64 {
    let f = 10_f64.powi(HASH_PRECISION as i32);
    let r = (v * f).round() / f;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

fn bbox(points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let x_min = points.iter().map(|p| p.0).reduce(f64::min)?;
    let x_max = points.iter().map(|p| p.0).reduce(f64::max)?;
    let y_min = points.iter().map(|p| p.1).reduce(f64::min)?;
    let y_max = points.iter().map(|p| p.1).reduce(f64::max)?;
    Some((x_min, x_max, y_min, y_max))
}

// ── Primitive recognition ──────────────────────────────────────────────────

/// Attempt to promote a set of flattened 2-D points into a recognised
/// geometric primitive.
///
/// The point set is expected to represent a **closed** path (the caller must
/// flatten any Bézier curves to points before calling).  Primitives are
/// checked in order of specificity:
///
/// 1. Circle (Kasa least-squares fit)
/// 2. Axis-aligned ellipse (centroid + bbox half-extents)
/// 3. Axis-aligned rectangle / rounded-rectangle
/// 4. Plain polygon — returns `None`; the caller falls back to raw path
///    elements.
///
/// ## Arc detection
///
/// Detection of partial arcs *within* a path (consecutive Bézier segments
/// that approximate a circular arc) is explicitly **out of scope** for this
/// function.  Only whole-shape promotion is considered.
///
/// ## Rotated ellipses
///
/// Rotated-ellipse fitting is **out of scope** for the initial dispatch.
/// The ellipse fit always produces `rotation: 0.0` (axis-aligned).
pub fn recognize(points: &[(f64, f64)], tolerance: f64) -> Option<Primitive> {
    if points.len() < 3 {
        return None;
    }

    // 1. Circle
    if let Some(c) = try_circle(points, tolerance) {
        return Some(Primitive::Circle {
            cx: c.0,
            cy: c.1,
            r: c.2,
        });
    }

    // 2. Axis-aligned ellipse
    if let Some(e) = try_ellipse(points, tolerance) {
        return Some(Primitive::Ellipse {
            cx: e.0,
            cy: e.1,
            rx: e.2,
            ry: e.3,
            rotation: 0.0,
        });
    }

    // 3. Rectangle / rounded-rectangle
    if let Some(rect) = try_rect(points, tolerance) {
        return Some(rect);
    }

    None
}

/// Kasa least-squares circle fit.
///
/// Solves the linear system for centre `(cx, cy)` and radius `r` that
/// minimises Σ((x-cx)² + (y-cy)² - r²)², then accepts if the maximum
/// point-to-circle distance is within `tolerance`.
fn try_circle(points: &[(f64, f64)], tolerance: f64) -> Option<(f64, f64, f64)> {
    let n = points.len() as f64;

    let s_x: f64 = points.iter().map(|p| p.0).sum();
    let s_y: f64 = points.iter().map(|p| p.1).sum();
    let s_xx: f64 = points.iter().map(|p| p.0 * p.0).sum();
    let s_yy: f64 = points.iter().map(|p| p.1 * p.1).sum();
    let s_xy: f64 = points.iter().map(|p| p.0 * p.1).sum();
    let s_xz: f64 = points.iter().map(|p| p.0 * (p.0 * p.0 + p.1 * p.1)).sum();
    let s_yz: f64 = points.iter().map(|p| p.1 * (p.0 * p.0 + p.1 * p.1)).sum();
    let s_zz: f64 = points.iter().map(|p| p.0 * p.0 + p.1 * p.1).sum();

    // Solve the 3×3 normal system by Cramer's rule:
    //   [ n   s_x  s_y ] [C]   [-s_zz]
    //   [s_x s_xx s_xy] [A] = [-s_xz]
    //   [s_y s_xy s_yy] [B]   [-s_yz]
    //
    // where  A = -2·cx,  B = -2·cy,  C = cx² + cy² − r².

    let det = n * (s_xx * s_yy - s_xy * s_xy) - s_x * (s_x * s_yy - s_xy * s_y)
        + s_y * (s_x * s_xy - s_xx * s_y);

    if det.abs() < 1e-15 {
        return None;
    }

    let r0 = -s_zz;
    let r1 = -s_xz;
    let r2 = -s_yz;

    // Cramer's rule via Laplace expansion along row 0.
    // Matrix  M * [C, A, B]^T = RHS
    //   M = [[n, s_x, s_y], [s_x, s_xx, s_xy], [s_y, s_xy, s_yy]]
    //   RHS = [r0, r1, r2]
    let det_c = r0 * (s_xx * s_yy - s_xy * s_xy) - s_x * (r1 * s_yy - s_xy * r2)
        + s_y * (r1 * s_xy - s_xx * r2);

    let det_a =
        n * (r1 * s_yy - s_xy * r2) - r0 * (s_x * s_yy - s_xy * s_y) + s_y * (s_x * r2 - r1 * s_y);

    let det_b =
        n * (s_xx * r2 - s_xy * r1) - s_x * (s_x * r2 - r1 * s_y) + r0 * (s_x * s_xy - s_xx * s_y);

    let cx = -det_a / (2.0 * det);
    let cy = -det_b / (2.0 * det);
    let r2 = cx * cx + cy * cy - det_c / det;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();

    let max_resid = points
        .iter()
        .map(|&(x, y)| (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r).abs())
        .fold(0.0_f64, f64::max);

    // Scale the budget with the radius, bounded both ways: at r ≈ 2 an
    // absolute 0.5px budget would let a 3×3 pixel square pass as a circle,
    // while at r ≈ 100 the anti-aliasing wiggle on a genuine circle exceeds
    // a fixed 0.5px and it would wrongly fall through to the rect fit.
    if max_resid <= tolerance.max(0.01 * r).min(0.05 * r) {
        Some((cx, cy, r))
    } else {
        None
    }
}

/// Axis-aligned ellipse fit.
///
/// The centre is the point-set centroid; `rx` / `ry` are the half-extents
/// of the bounding box.  A point is accepted if its distance to the ellipse
/// boundary at the same angle is within `tolerance`.
fn try_ellipse(points: &[(f64, f64)], tolerance: f64) -> Option<(f64, f64, f64, f64)> {
    let n = points.len() as f64;
    let cx: f64 = points.iter().map(|p| p.0).sum::<f64>() / n;
    let cy: f64 = points.iter().map(|p| p.1).sum::<f64>() / n;

    let (x_min, x_max, y_min, y_max) = bbox(points)?;
    let rx = (x_max - x_min) / 2.0;
    let ry = (y_max - y_min) / 2.0;

    if rx < 1e-10 || ry < 1e-10 {
        return None;
    }

    let max_resid = points
        .iter()
        .map(|&(x, y)| {
            let dx = x - cx;
            let dy = y - cy;
            let theta = dy.atan2(dx);
            let sin_t = theta.sin();
            let cos_t = theta.cos();
            let r_theta = rx * ry / ((rx * sin_t).powi(2) + (ry * cos_t).powi(2)).sqrt();
            ((dx * dx + dy * dy).sqrt() - r_theta).abs()
        })
        .fold(0.0_f64, f64::max);

    // Same radius-scaled bound as the circle fit, on the smaller half-axis.
    let r_min = rx.min(ry);
    if max_resid <= tolerance.max(0.01 * r_min).min(0.05 * r_min) {
        Some((cx, cy, rx, ry))
    } else {
        None
    }
}

/// Rectangle / rounded-rectangle detection.
///
/// Verifies that every point lies on the bounding-box perimeter (allowing a
/// generous inset for corner arcs), then estimates corner radii.
fn try_rect(points: &[(f64, f64)], tolerance: f64) -> Option<Primitive> {
    let (x_min, x_max, y_min, y_max) = bbox(points)?;
    let width = x_max - x_min;
    let height = y_max - y_min;

    if width < 1e-10 || height < 1e-10 {
        return None;
    }

    // Decisive rect-vs-curve discriminator: a (rounded) rectangle fills
    // nearly all of its bounding box, while a circle fills only pi/4 = 78.5%
    // and an ellipse or stadium even less. Without this gate a circle whose
    // stricter fits failed on anti-aliasing wiggle would be "promoted" to a
    // rect — i.e. repainted as its bounding square. The 0.93 bound still
    // admits rounded rects with corner radius up to ~0.29*sqrt(w*h).
    let mut poly_area = 0.0;
    for i in 0..points.len() {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % points.len()];
        poly_area += x1 * y2 - x2 * y1;
    }
    if (poly_area / 2.0).abs() < 0.93 * width * height {
        return None;
    }

    // Every point must be within a generous margin of the bounding box
    // perimeter.  The margin accounts for corner arcs on rounded rectangles.
    let margin = tolerance.max(width.min(height) * 0.15);
    for &(x, y) in points {
        let d_left = (x - x_min).abs();
        let d_right = (x - x_max).abs();
        let d_top = (y - y_min).abs();
        let d_bottom = (y - y_max).abs();
        let min_d = d_left.min(d_right).min(d_top).min(d_bottom);
        if min_d > margin {
            return None;
        }
    }

    // Estimate corner radius at each of the 4 corners.
    // Each point is assigned to the nearest corner (by max of dx, dy) so
    // that straight-edge points don't pollute distant corner estimates.
    let interior_tol = tolerance.max(1e-6);
    let mut radii_est: [Vec<f64>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    for &(px, py) in points {
        let corner_deltas = [
            (px - x_min, py - y_min), // TL
            (x_max - px, py - y_min), // TR
            (x_max - px, y_max - py), // BR
            (px - x_min, y_max - py), // BL
        ];
        // Find nearest corner by smallest max(dx, dy)
        let best_ci = corner_deltas
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let ma = a.0.max(a.1);
                let mb = b.0.max(b.1);
                ma.partial_cmp(&mb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);

        let (dx, dy) = corner_deltas[best_ci];
        if dx > interior_tol && dy > interior_tol {
            // Interior corner-arc point — estimate radius from the
            // quarter-circle relation:  (R-dx)^2 + (R-dy)^2 = R^2
            // =>  R = dx + dy + sqrt(2*dx*dy)
            let r_est = dx + dy + (2.0 * dx * dy).sqrt();
            if r_est.is_finite() && r_est > 0.0 {
                radii_est[best_ci].push(r_est);
            }
        }
    }

    // Compute mean radius per corner (skip empty corners).
    let mut corner_means: Vec<f64> = Vec::new();
    for estimates in &radii_est {
        if estimates.is_empty() {
            corner_means.push(0.0);
        } else {
            let mean = estimates.iter().sum::<f64>() / estimates.len() as f64;
            corner_means.push(mean);
        }
    }

    let nonzero: Vec<f64> = corner_means
        .iter()
        .filter(|&&r| r > tolerance)
        .copied()
        .collect();

    if nonzero.is_empty() {
        // Sharp rect
        Some(Primitive::Rect {
            x: x_min,
            y: y_min,
            width,
            height,
            rx: None,
            ry: None,
        })
    } else if nonzero.len() == 4 {
        let mean_r = nonzero.iter().sum::<f64>() / nonzero.len() as f64;
        let max_dev = nonzero
            .iter()
            .map(|r| (r - mean_r).abs())
            .fold(0.0_f64, f64::max);
        if max_dev <= tolerance * 2.0 {
            let cr = Some(mean_r);
            Some(Primitive::Rect {
                x: x_min,
                y: y_min,
                width,
                height,
                rx: cr,
                ry: cr,
            })
        } else {
            // Inconsistent radius estimates are estimator noise on what the
            // area gate above already certified as rectangle-like (skinny
            // rects make the quarter-circle estimator meaningless).
            Some(Primitive::Rect {
                x: x_min,
                y: y_min,
                width,
                height,
                rx: None,
                ry: None,
            })
        }
    } else {
        Some(Primitive::Rect {
            x: x_min,
            y: y_min,
            width,
            height,
            rx: None,
            ry: None,
        })
    }
}

// ── Stable-ID hashing (§3.12) ──────────────────────────────────────────────

/// Generate a stable, deterministic ID for a shape.
///
/// The ID is computed as `blake3(rounded geometry + fill + z-index)`, taking
/// the first 8 hex characters of the digest, prefixed with `s-`.
///
/// Every coordinate is rounded to [`HASH_PRECISION`] (currently 3) decimal
/// places *before* hashing, so two shapes that are geometrically identical up
/// to floating-point noise still produce the same ID.  Coordinates at
/// exactly `-0.0` are normalised to `0.0`.
///
/// ## Deterministic encoding
///
/// Each rounded coordinate is formatted as `"{:.3}"`, fill is formatted as
/// `"rrggbb"` (or `"none"`), and z-index is formatted as a 16-hex-digit
/// fixed-width field.  No `HashMap` or locale-dependent formatting is used —
/// the byte sequence fed to blake3 is fully controlled.
pub fn stable_id(points: &[(f64, f64)], fill: Option<Rgb>, z_index: usize) -> String {
    let mut buf = String::new();
    for &(x, y) in points {
        write!(buf, "{:.3}{:.3}", round_coord(x), round_coord(y)).unwrap();
    }
    match fill {
        Some(rgb) => {
            write!(buf, "{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b).unwrap();
        }
        None => {
            buf.push_str("none");
        }
    }
    write!(buf, "{:016x}", z_index).unwrap();
    let hash = blake3::hash(buf.as_bytes());
    format!("s-{}", &hash.to_hex()[..8])
}

// ── Collision handling ─────────────────────────────────────────────────────

/// Deduplicate a slice of IDs in place by appending `-2`, `-3`, … suffixes
/// to later occurrences of repeated IDs.
///
/// The first occurrence of each ID is left unchanged.  Order is stable (based
/// on input order), and the implementation uses an ordered scan so results
/// are deterministic and platform-independent.
pub fn dedupe_ids(ids: &mut [String]) {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for id in ids.iter_mut() {
        let entry = counts.entry(id.clone()).or_insert(0);
        *entry += 1;
        if *entry > 1 {
            *id = format!("{}-{}", id, *entry);
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
