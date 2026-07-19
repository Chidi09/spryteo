use std::fmt::Write;

use spryteo_core::ir::{Primitive, Rgb};

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
mod tests {
    use super::*;

    /// Sample points along a circle centred at (cx, cy) with radius r.
    fn sample_circle(cx: f64, cy: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

    /// Sample the perimeter of an axis-aligned rectangle.
    fn sample_rect(x: f64, y: f64, w: f64, h: f64, points_per_side: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x + t * w, y));
        }
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x + w, y + t * h));
        }
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x + (1.0 - t) * w, y + h));
        }
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x, y + (1.0 - t) * h));
        }
        pts
    }

    /// Sample a rounded-rectangle perimeter.
    fn sample_rounded_rect(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        pts_per_side: usize,
        pts_per_corner: usize,
    ) -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        // Top edge (left to right), excluding corner arcs
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let px = x + r + t * (w - 2.0 * r);
            pts.push((px, y));
        }
        // Top-right corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + w - r + r * angle.sin();
            let py = y + r - r * angle.cos();
            pts.push((px, py));
        }
        // Right edge (top to bottom)
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let py = y + r + t * (h - 2.0 * r);
            pts.push((x + w, py));
        }
        // Bottom-right corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + w - r + r * angle.cos();
            let py = y + h - r + r * angle.sin();
            pts.push((px, py));
        }
        // Bottom edge (right to left)
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let px = x + w - r - t * (w - 2.0 * r);
            pts.push((px, y + h));
        }
        // Bottom-left corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + r - r * angle.sin();
            let py = y + h - r + r * angle.cos();
            pts.push((px, py));
        }
        // Left edge (bottom to top)
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let py = y + h - r - t * (h - 2.0 * r);
            pts.push((x, py));
        }
        // Top-left corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + r - r * angle.cos();
            let py = y + r - r * angle.sin();
            pts.push((px, py));
        }
        pts
    }

    #[test]
    fn test_recognize_circle() {
        let pts = sample_circle(50.0, 30.0, 20.0, 64);
        let prim = recognize(&pts, 1.0).expect("should recognise a circle");
        match prim {
            Primitive::Circle { cx, cy, r } => {
                assert!((cx - 50.0).abs() < 0.5, "cx off");
                assert!((cy - 30.0).abs() < 0.5, "cy off");
                assert!((r - 20.0).abs() < 0.5, "r off");
            }
            other => panic!("expected Circle, got {other:?}"),
        }
    }

    #[test]
    fn test_recognize_square() {
        let pts = sample_rect(0.0, 0.0, 10.0, 10.0, 10);
        let prim = recognize(&pts, 0.5).expect("should recognise a rect");
        match prim {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
            } => {
                assert!((x - 0.0).abs() < 0.01);
                assert!((y - 0.0).abs() < 0.01);
                assert!((width - 10.0).abs() < 0.01);
                assert!((height - 10.0).abs() < 0.01);
                assert!(rx.is_none(), "sharp rect should have rx=None");
                assert!(ry.is_none(), "sharp rect should have ry=None");
            }
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    #[test]
    fn test_recognize_rounded_square() {
        let corner_r = 2.0;
        let pts = sample_rounded_rect(0.0, 0.0, 10.0, 10.0, corner_r, 10, 8);
        let prim = recognize(&pts, 0.5).expect("should recognise a rounded rect");
        match prim {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
            } => {
                assert!((x - 0.0).abs() < 0.01);
                assert!((y - 0.0).abs() < 0.01);
                assert!((width - 10.0).abs() < 0.01);
                assert!((height - 10.0).abs() < 0.01);
                let actual_r = rx.expect("rx should be Some for rounded rect");
                let actual_ry = ry.expect("ry should be Some for rounded rect");
                assert!(
                    (actual_r - corner_r).abs() < 1.0,
                    "rx {actual_r} not close to {corner_r}"
                );
                assert!(
                    (actual_ry - corner_r).abs() < 1.0,
                    "ry {actual_ry} not close to {corner_r}"
                );
            }
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    #[test]
    fn test_recognize_scribble_returns_none() {
        // Random-ish points that don't form any recognisable shape
        let pts = vec![
            (0.0, 0.0),
            (3.0, 1.0),
            (5.0, 4.0),
            (2.0, 7.0),
            (1.0, 3.0),
            (6.0, 2.0),
            (8.0, 6.0),
            (4.0, 9.0),
            (0.5, 6.0),
        ];
        assert!(recognize(&pts, 1.0).is_none());
    }

    #[test]
    fn test_stable_id_deterministic() {
        let pts = vec![(1.234, 5.678), (9.012, 3.456)];
        let fill = Some(Rgb {
            r: 255,
            g: 128,
            b: 0,
        });
        let id1 = stable_id(&pts, fill, 42);
        let id2 = stable_id(&pts, fill, 42);
        assert_eq!(id1, id2, "deterministic IDs must match");
    }

    #[test]
    fn test_stable_id_floating_point_stability() {
        // Sets differing only in the 6th decimal place (well below the
        // rounding precision of 3 dp) should produce the SAME id.
        let pts_a = vec![(1.234567, 2.345678)];
        let pts_b = vec![(1.234599, 2.345601)];
        let fill = Some(Rgb { r: 0, g: 0, b: 0 });
        let id_a = stable_id(&pts_a, fill, 0);
        let id_b = stable_id(&pts_b, fill, 0);
        assert_eq!(id_a, id_b, "6th-dp noise should produce same ID");

        // Sets differing in the 2nd decimal place (above rounding precision)
        // should produce DIFFERENT ids.
        let pts_c = vec![(1.23, 2.34)];
        let pts_d = vec![(1.24, 2.35)];
        let id_c = stable_id(&pts_c, fill, 0);
        let id_d = stable_id(&pts_d, fill, 0);
        assert_ne!(id_c, id_d, "2nd-dp difference should produce different IDs");
    }

    #[test]
    fn test_stable_id_without_fill() {
        let pts = vec![(0.0, 0.0)];
        let id = stable_id(&pts, None, 5);
        assert!(id.starts_with("s-"), "ID should start with s-");
        assert_eq!(id.len(), 10, "s- + 8 hex chars = 10"); // s- + 8 hex
    }

    #[test]
    fn test_dedupe_ids() {
        let mut ids = vec![
            "s-aaa".to_string(),
            "s-aaa".to_string(),
            "s-bbb".to_string(),
            "s-aaa".to_string(),
        ];
        dedupe_ids(&mut ids);
        assert_eq!(ids[0], "s-aaa");
        assert_eq!(ids[1], "s-aaa-2");
        assert_eq!(ids[2], "s-bbb");
        assert_eq!(ids[3], "s-aaa-3");
    }

    #[test]
    fn test_dedupe_ids_no_duplicates() {
        let mut ids = vec![
            "s-aaa".to_string(),
            "s-bbb".to_string(),
            "s-ccc".to_string(),
        ];
        let original = ids.clone();
        dedupe_ids(&mut ids);
        assert_eq!(ids, original);
    }

    #[test]
    fn test_dedupe_ids_empty() {
        let mut ids: Vec<String> = vec![];
        dedupe_ids(&mut ids);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_recognize_few_points() {
        // < 3 points should always return None
        assert!(recognize(&[], 1.0).is_none());
        assert!(recognize(&[(0.0, 0.0)], 1.0).is_none());
        assert!(recognize(&[(0.0, 0.0), (1.0, 0.0)], 1.0).is_none());
    }

    #[test]
    fn test_stable_id_prefix_format() {
        let pts = vec![(0.0, 0.0)];
        let fill = Some(Rgb { r: 0, g: 0, b: 0 });
        let id = stable_id(&pts, fill, 0);
        assert_eq!(id.len(), 10, "expected s-xxxxxxxx (10 chars)");
        assert!(id.starts_with("s-"), "expected s- prefix");
        // The hex part should be valid hex
        let hex_part = &id[2..];
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_dedupe_ids_consecutive_duplicates() {
        let mut ids = vec!["s-x".to_string(), "s-x".to_string(), "s-x".to_string()];
        dedupe_ids(&mut ids);
        assert_eq!(ids[0], "s-x");
        assert_eq!(ids[1], "s-x-2");
        assert_eq!(ids[2], "s-x-3");
    }
}
