use crate::color::{self, Lab};
use spryteo_core::{Fill, GradientStop, Layer, RasterImage};

/// Fit a linear or radial gradient to the color distribution of the pixels in the image
/// covered by the layer mask. Returns Some(Fill) if a gradient fit is better than a flat fill
/// and its residual is below the tolerance.
pub fn detect_gradient(image: &RasterImage, layer: &Layer, tolerance: f64) -> Option<Fill> {
    let width = image.width;
    let height = image.height;

    // Safety check: layer mask must match image dimensions
    if layer.mask.len() != (width * height) as usize {
        return None;
    }

    // 1. Collect all non-zero covered pixels
    struct PixelCoord {
        x: f64,
        y: f64,
        lab: Lab,
    }

    // Least-squares gradient fitting is statistical: a bounded,
    // evenly-strided sample of the covered pixels gives the same fit
    // (deterministically) as using every pixel, while converting all
    // covered pixels to Lab and making several full passes over them
    // cost ~2s across the layers of a 1-megapixel photo. The stride is
    // in *covered-pixel* order, so it stays evenly spread across the
    // region regardless of mask shape.
    const GRADIENT_SAMPLE_MAX: usize = 16_384;
    let covered = layer.mask.iter().filter(|&&m| m != 0).count();
    let stride = covered.div_ceil(GRADIENT_SAMPLE_MAX).max(1);

    let mut coords = Vec::new();
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;

    let mut covered_seen = 0usize;
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if layer.mask[idx] != 0 {
                if covered_seen.is_multiple_of(stride) {
                    let rgb = color::get_rgb(&image.pixels, width, x, y);
                    let lab = color::srgb_to_lab(&rgb);
                    let x_f = x as f64;
                    let y_f = y as f64;
                    sum_x += x_f;
                    sum_y += y_f;
                    coords.push(PixelCoord {
                        x: x_f,
                        y: y_f,
                        lab,
                    });
                }
                covered_seen += 1;
            }
        }
    }

    let n = coords.len();
    if n < 3 {
        return None;
    }

    let n_f = n as f64;
    let cx = sum_x / n_f;
    let cy = sum_y / n_f;

    // 2. Check variance/flatness
    // Compute mean Lab color
    let mut sum_l = 0.0;
    let mut sum_a = 0.0;
    let mut sum_b = 0.0;
    for p in &coords {
        sum_l += p.lab.l;
        sum_a += p.lab.a;
        sum_b += p.lab.b;
    }
    let mean_lab = Lab {
        l: sum_l / n_f,
        a: sum_a / n_f,
        b: sum_b / n_f,
    };

    let mut sum_dist_sq = 0.0;
    for p in &coords {
        sum_dist_sq += color::lab_distance_sq(&p.lab, &mean_lab);
    }
    let std_dev = (sum_dist_sq / n_f).sqrt();

    // If standard deviation in Lab is less than 1.0 (Just Noticeable Difference),
    // then the region is perceptually flat and we do not promote to a gradient.
    if std_dev < 1.0 {
        return None;
    }

    // Initialize best fit variables
    let mut best_fit: Option<Fill> = None;
    let mut best_residual = f64::MAX;

    // ─── LINEAR FIT ───
    // Fit channel = c0 + c1*x + c2*y
    // Set up normal equations: M * c = V for L, a, b
    let mut s_x = 0.0;
    let mut s_y = 0.0;
    let mut s_xx = 0.0;
    let mut s_xy = 0.0;
    let mut s_yy = 0.0;

    let mut v_l = [0.0; 3];
    let mut v_a = [0.0; 3];
    let mut v_b = [0.0; 3];

    for p in &coords {
        s_x += p.x;
        s_y += p.y;
        s_xx += p.x * p.x;
        s_xy += p.x * p.y;
        s_yy += p.y * p.y;

        v_l[0] += p.lab.l;
        v_l[1] += p.x * p.lab.l;
        v_l[2] += p.y * p.lab.l;

        v_a[0] += p.lab.a;
        v_a[1] += p.x * p.lab.a;
        v_a[2] += p.y * p.lab.a;

        v_b[0] += p.lab.b;
        v_b[1] += p.x * p.lab.b;
        v_b[2] += p.y * p.lab.b;
    }

    let m = [[n_f, s_x, s_y], [s_x, s_xx, s_xy], [s_y, s_xy, s_yy]];

    if let (Some(c_l), Some(c_a), Some(c_b)) =
        (solve_3x3(m, v_l), solve_3x3(m, v_a), solve_3x3(m, v_b))
    {
        // Evaluate linear fit residual
        let mut lin_sum_sq = 0.0;
        for p in &coords {
            let pred_l = c_l[0] + c_l[1] * p.x + c_l[2] * p.y;
            let pred_a = c_a[0] + c_a[1] * p.x + c_a[2] * p.y;
            let pred_b = c_b[0] + c_b[1] * p.x + c_b[2] * p.y;
            let pred_lab = Lab {
                l: pred_l,
                a: pred_a,
                b: pred_b,
            };
            lin_sum_sq += color::lab_distance_sq(&p.lab, &pred_lab);
        }
        let lin_residual = (lin_sum_sq / n_f).sqrt();

        // Derive direction of steepest color change (eigenvector of J^T J)
        let e = c_l[1] * c_l[1] + c_a[1] * c_a[1] + c_b[1] * c_b[1];
        let g_prime = c_l[2] * c_l[2] + c_a[2] * c_a[2] + c_b[2] * c_b[2];
        let f = c_l[1] * c_l[2] + c_a[1] * c_a[2] + c_b[1] * c_b[2];

        let (dir_x, dir_y) = if f.abs() < 1e-9 {
            if e >= g_prime {
                (1.0, 0.0)
            } else {
                (0.0, 1.0)
            }
        } else {
            let trace = e + g_prime;
            let desc = ((e - g_prime) * (e - g_prime) + 4.0 * f * f).sqrt();
            let lambda = (trace + desc) / 2.0;
            let vx = f;
            let vy = lambda - e;
            let len = (vx * vx + vy * vy).sqrt();
            if len < 1e-9 {
                (1.0, 0.0)
            } else {
                (vx / len, vy / len)
            }
        };

        // Project onto gradient line passing through centroid
        let mut t_min = f64::MAX;
        let mut t_max = f64::MIN;
        for p in &coords {
            let t = (p.x - cx) * dir_x + (p.y - cy) * dir_y;
            if t < t_min {
                t_min = t;
            }
            if t > t_max {
                t_max = t;
            }
        }

        // Handle case where t_max and t_min are identical (e.g. single line of pixels perpendicular to dir)
        let (t_min, t_max) = if (t_max - t_min).abs() < 1e-5 {
            (t_min - 0.5, t_max + 0.5)
        } else {
            (t_min, t_max)
        };

        let x1 = cx + t_min * dir_x;
        let y1 = cy + t_min * dir_y;
        let x2 = cx + t_max * dir_x;
        let y2 = cy + t_max * dir_y;

        // Predict color at endpoints
        let lab1 = Lab {
            l: c_l[0] + c_l[1] * x1 + c_l[2] * y1,
            a: c_a[0] + c_a[1] * x1 + c_a[2] * y1,
            b: c_b[0] + c_b[1] * x1 + c_b[2] * y1,
        };
        let lab2 = Lab {
            l: c_l[0] + c_l[1] * x2 + c_l[2] * y2,
            a: c_a[0] + c_a[1] * x2 + c_a[2] * y2,
            b: c_b[0] + c_b[1] * x2 + c_b[2] * y2,
        };

        let color1 = color::lab_to_srgb(&lab1);
        let color2 = color::lab_to_srgb(&lab2);

        let stops = vec![
            GradientStop {
                offset: 0.0,
                color: color1,
            },
            GradientStop {
                offset: 1.0,
                color: color2,
            },
        ];

        let fill = Fill::LinearGradient {
            x1,
            y1,
            x2,
            y2,
            stops,
        };
        if lin_residual < best_residual {
            best_residual = lin_residual;
            best_fit = Some(fill);
        }
    }

    // ─── RADIAL FIT ───
    // Fit channel = c0 + c1*dist
    let mut s_d = 0.0;
    let mut s_dd = 0.0;
    let mut v_l_r = [0.0; 2];
    let mut v_a_r = [0.0; 2];
    let mut v_b_r = [0.0; 2];

    let mut max_dist = 0.0;
    let mut dists = Vec::with_capacity(n);

    for p in &coords {
        let dx = p.x - cx;
        let dy = p.y - cy;
        let d = (dx * dx + dy * dy).sqrt();
        dists.push(d);
        if d > max_dist {
            max_dist = d;
        }

        s_d += d;
        s_dd += d * d;

        v_l_r[0] += p.lab.l;
        v_l_r[1] += d * p.lab.l;

        v_a_r[0] += p.lab.a;
        v_a_r[1] += d * p.lab.a;

        v_b_r[0] += p.lab.b;
        v_b_r[1] += d * p.lab.b;
    }

    let det_r = n_f * s_dd - s_d * s_d;
    if det_r.abs() > 1e-7 {
        let c_l_r0 = (s_dd * v_l_r[0] - s_d * v_l_r[1]) / det_r;
        let c_l_r1 = (n_f * v_l_r[1] - s_d * v_l_r[0]) / det_r;

        let c_a_r0 = (s_dd * v_a_r[0] - s_d * v_a_r[1]) / det_r;
        let c_a_r1 = (n_f * v_a_r[1] - s_d * v_a_r[0]) / det_r;

        let c_b_r0 = (s_dd * v_b_r[0] - s_d * v_b_r[1]) / det_r;
        let c_b_r1 = (n_f * v_b_r[1] - s_d * v_b_r[0]) / det_r;

        // Evaluate radial fit residual
        let mut rad_sum_sq = 0.0;
        for (i, p) in coords.iter().enumerate() {
            let d = dists[i];
            let pred_l = c_l_r0 + c_l_r1 * d;
            let pred_a = c_a_r0 + c_a_r1 * d;
            let pred_b = c_b_r0 + c_b_r1 * d;
            let pred_lab = Lab {
                l: pred_l,
                a: pred_a,
                b: pred_b,
            };
            rad_sum_sq += color::lab_distance_sq(&p.lab, &pred_lab);
        }
        let rad_residual = (rad_sum_sq / n_f).sqrt();

        let r_val = if max_dist < 1e-5 { 1.0 } else { max_dist };

        // Predict color at center (dist = 0) and edge (dist = r)
        let lab_center = Lab {
            l: c_l_r0,
            a: c_a_r0,
            b: c_b_r0,
        };
        let lab_edge = Lab {
            l: c_l_r0 + c_l_r1 * r_val,
            a: c_a_r0 + c_a_r1 * r_val,
            b: c_b_r0 + c_b_r1 * r_val,
        };

        let color_center = color::lab_to_srgb(&lab_center);
        let color_edge = color::lab_to_srgb(&lab_edge);

        let stops = vec![
            GradientStop {
                offset: 0.0,
                color: color_center,
            },
            GradientStop {
                offset: 1.0,
                color: color_edge,
            },
        ];

        let fill = Fill::RadialGradient {
            cx,
            cy,
            r: r_val,
            stops,
        };

        if rad_residual < best_residual {
            best_residual = rad_residual;
            best_fit = Some(fill);
        }
    }

    // Only promote if the better fit is under tolerance
    if best_residual < tolerance {
        best_fit
    } else {
        None
    }
}

// Solver for 3x3 linear system using Cramer's rule
fn solve_3x3(m: [[f64; 3]; 3], v: [f64; 3]) -> Option<[f64; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-7 {
        return None;
    }

    let det0 = v[0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (v[1] * m[2][2] - m[1][2] * v[2])
        + m[0][2] * (v[1] * m[2][1] - m[1][1] * v[2]);

    let det1 = m[0][0] * (v[1] * m[2][2] - m[1][2] * v[2])
        - v[0] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * v[2] - v[1] * m[2][0]);

    let det2 = m[0][0] * (m[1][1] * v[2] - v[1] * m[2][1])
        - m[0][1] * (m[1][0] * v[2] - v[1] * m[2][0])
        + v[0] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    Some([det0 / det, det1 / det, det2 / det])
}
#[cfg(test)]
#[path = "gradient_tests.rs"]
mod tests;
