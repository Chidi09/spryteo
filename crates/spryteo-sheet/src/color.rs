use crate::chroma::{saturation, Mask};
use crate::cluster::Bbox;
use spryteo_core::ir::{RasterImage, Rgb};

#[derive(Debug, Clone, PartialEq)]
pub struct GradientFit {
    /// Gradient axis start point, in the same pixel coordinates as `rect`.
    pub x1: f64,
    pub y1: f64,
    /// Gradient axis end point.
    pub x2: f64,
    pub y2: f64,
    /// Colour at (x1,y1).
    pub start: Rgb,
    /// Colour at (x2,y2).
    pub end: Rgb,
    /// RMS residual of the fitted model against the sampled pixels, in 0..255
    /// colour units. Lower is a better fit.
    pub residual: f64,
    /// How many pixels the fit was computed from.
    pub sample_count: usize,
}

#[derive(Debug, Clone)]
pub struct ColorConfig {
    /// Only pixels at or above this saturation are sampled. Rejects
    /// anti-aliased edge pixels, which are blends with the white background
    /// and would drag both endpoint colours toward white.
    pub core_sat_min: u8, // default 150
    /// Below this many sampled pixels, return None -- too little data.
    pub min_samples: usize, // default 30
    /// Above this RMS residual the linear model does not describe the data;
    /// return None and let the caller fall back to a flat colour.
    ///
    /// Calibrated against the real target sheet, not guessed. Fitting all 105
    /// icons there gives residuals of p10 20.8 / median 26.7 / p90 32.1 /
    /// max 37.5, so the original estimate of 18.0 rejected 103 of 105 icons
    /// whose gradients were in fact recovered correctly (median axis angle 51
    /// degrees, median start-to-end colour distance 74 -- a real ramp, not
    /// noise). The residual floor is set by anti-aliasing: stroke edge pixels
    /// are blends with the white page, and even at `core_sat_min` some survive
    /// and sit off the fitted plane. That inflates RMS without saying anything
    /// about whether the axis is right. 36.0 admits every real icon while still
    /// firing on data a plane genuinely cannot describe -- randomised colours
    /// score well above 100.
    pub max_residual: f64, // default 36.0
}

impl Default for ColorConfig {
    fn default() -> Self {
        ColorConfig {
            core_sat_min: 150,
            min_samples: 30,
            max_residual: 36.0,
        }
    }
}

/// Solves M * x = v for a 3x3 matrix M via Gaussian elimination with partial pivoting.
/// Returns None if the matrix is singular or badly conditioned.
#[allow(clippy::needless_range_loop)]
fn solve_3x3(mut m: [[f64; 3]; 3], mut v: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let mut pivot_row = col;
        let mut max_val = m[col][col].abs();
        for r in (col + 1)..3 {
            let val = m[r][col].abs();
            if val > max_val {
                max_val = val;
                pivot_row = r;
            }
        }

        if max_val < 1e-12 {
            return None;
        }

        if pivot_row != col {
            m.swap(col, pivot_row);
            v.swap(col, pivot_row);
        }

        let pivot = m[col][col];
        for r in (col + 1)..3 {
            let factor = m[r][col] / pivot;
            m[r][col] = 0.0;
            for c in (col + 1)..3 {
                m[r][c] -= factor * m[col][c];
            }
            v[r] -= factor * v[col];
        }
    }

    let mut x = [0.0; 3];
    for i in (0..3).rev() {
        let mut sum = v[i];
        for j in (i + 1)..3 {
            sum -= m[i][j] * x[j];
        }
        if m[i][i].abs() < 1e-12 {
            return None;
        }
        x[i] = sum / m[i][i];
    }

    Some(x)
}

/// Computes the p-th percentile (0.0..=1.0) of a sorted slice using linear interpolation.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = (sorted.len() - 1) as f64 * p;
    let idx = pos.floor() as usize;
    let frac = pos - idx as f64;
    if idx + 1 < sorted.len() {
        sorted[idx] * (1.0 - frac) + sorted[idx + 1] * frac
    } else {
        sorted[idx]
    }
}

/// Fits a linear (planar) colour ramp over the core pixels of one icon.
/// Returns None when there is too little data, the fit is too poor, or the
/// colour genuinely does not vary (a flat-coloured icon).
pub fn fit_linear_gradient(
    image: &RasterImage,
    mask: &Mask,
    rect: &Bbox,
    cfg: &ColorConfig,
) -> Option<GradientFit> {
    // STEP 1 -- SAMPLE
    let mut samples = Vec::new();
    for y in rect.y1..rect.y2 {
        for x in rect.x1..rect.x2 {
            if mask.get(x, y) {
                let idx = (y as usize * image.width as usize + x as usize) * 4;
                if idx + 3 < image.pixels.len() {
                    let r = image.pixels[idx];
                    let g = image.pixels[idx + 1];
                    let b = image.pixels[idx + 2];
                    if saturation(r, g, b) >= cfg.core_sat_min {
                        samples.push((x as f64, y as f64, r as f64, g as f64, b as f64));
                    }
                }
            }
        }
    }

    if samples.len() < cfg.min_samples {
        return None;
    }

    // STEP 2 -- PER-CHANNEL PLANE FIT
    let sample_count = samples.len();
    let n = sample_count as f64;
    let mean_x = samples.iter().map(|s| s.0).sum::<f64>() / n;
    let mean_y = samples.iter().map(|s| s.1).sum::<f64>() / n;

    let mut s_x = 0.0;
    let mut s_y = 0.0;
    let mut s_xx = 0.0;
    let mut s_xy = 0.0;
    let mut s_yy = 0.0;

    let mut s_r = 0.0;
    let mut s_xr = 0.0;
    let mut s_yr = 0.0;

    let mut s_g = 0.0;
    let mut s_xg = 0.0;
    let mut s_yg = 0.0;

    let mut s_b = 0.0;
    let mut s_xb = 0.0;
    let mut s_yb = 0.0;

    for &(x, y, r, g, b) in &samples {
        let dx = x - mean_x;
        let dy = y - mean_y;

        s_x += dx;
        s_y += dy;
        s_xx += dx * dx;
        s_xy += dx * dy;
        s_yy += dy * dy;

        s_r += r;
        s_xr += dx * r;
        s_yr += dy * r;

        s_g += g;
        s_xg += dx * g;
        s_yg += dy * g;

        s_b += b;
        s_xb += dx * b;
        s_yb += dy * b;
    }

    let m = [[n, s_x, s_y], [s_x, s_xx, s_xy], [s_y, s_xy, s_yy]];

    let sol_r = solve_3x3(m, [s_r, s_xr, s_yr])?;
    let sol_g = solve_3x3(m, [s_g, s_xg, s_yg])?;
    let sol_b = solve_3x3(m, [s_b, s_xb, s_yb])?;

    // STEP 3 -- DIRECTION
    let bx_r = sol_r[1];
    let by_r = sol_r[2];
    let bx_g = sol_g[1];
    let by_g = sol_g[2];
    let bx_b = sol_b[1];
    let by_b = sol_b[2];

    let a00 = bx_r * bx_r + bx_g * bx_g + bx_b * bx_b;
    let a01 = bx_r * by_r + bx_g * by_g + bx_b * by_b;
    let a11 = by_r * by_r + by_g * by_g + by_b * by_b;

    let trace = a00 + a11;
    let det = a00 * a11 - a01 * a01;
    let disc = (trace * trace / 4.0 - det).max(0.0);
    let lambda1 = trace / 2.0 + disc.sqrt();

    if lambda1 < 1e-9 {
        return None;
    }

    let (mut dir_x, mut dir_y) = if a01.abs() < 1e-12 {
        if a00 >= a11 {
            (1.0, 0.0)
        } else {
            (0.0, 1.0)
        }
    } else {
        (a01, lambda1 - a00)
    };

    let len = (dir_x * dir_x + dir_y * dir_y).sqrt();
    if len < 1e-12 {
        return None;
    }
    dir_x /= len;
    dir_y /= len;

    if dir_x + dir_y < 0.0 {
        dir_x = -dir_x;
        dir_y = -dir_y;
    }

    // STEP 4 -- EXTENT
    let mut t_values: Vec<f64> = samples
        .iter()
        .map(|&(x, y, _, _, _)| (x - mean_x) * dir_x + (y - mean_y) * dir_y)
        .collect();
    t_values.sort_by(|a, b| a.total_cmp(b));

    let t_start = percentile(&t_values, 0.05);
    let t_end = percentile(&t_values, 0.95);

    if (t_end - t_start).abs() < 1e-6 {
        return None;
    }

    // STEP 5 -- ENDPOINTS
    let x1 = mean_x + dir_x * t_start;
    let y1 = mean_y + dir_y * t_start;
    let x2 = mean_x + dir_x * t_end;
    let y2 = mean_y + dir_y * t_end;

    let dx1 = dir_x * t_start;
    let dy1 = dir_y * t_start;
    let start_r = (sol_r[0] + sol_r[1] * dx1 + sol_r[2] * dy1)
        .clamp(0.0, 255.0)
        .round() as u8;
    let start_g = (sol_g[0] + sol_g[1] * dx1 + sol_g[2] * dy1)
        .clamp(0.0, 255.0)
        .round() as u8;
    let start_b = (sol_b[0] + sol_b[1] * dx1 + sol_b[2] * dy1)
        .clamp(0.0, 255.0)
        .round() as u8;

    let dx2 = dir_x * t_end;
    let dy2 = dir_y * t_end;
    let end_r = (sol_r[0] + sol_r[1] * dx2 + sol_r[2] * dy2)
        .clamp(0.0, 255.0)
        .round() as u8;
    let end_g = (sol_g[0] + sol_g[1] * dx2 + sol_g[2] * dy2)
        .clamp(0.0, 255.0)
        .round() as u8;
    let end_b = (sol_b[0] + sol_b[1] * dx2 + sol_b[2] * dy2)
        .clamp(0.0, 255.0)
        .round() as u8;

    // STEP 6 -- RESIDUAL
    let mut sum_sq_err = 0.0;
    for &(x, y, r, g, b) in &samples {
        let dx = x - mean_x;
        let dy = y - mean_y;

        let pred_r = sol_r[0] + sol_r[1] * dx + sol_r[2] * dy;
        let pred_g = sol_g[0] + sol_g[1] * dx + sol_g[2] * dy;
        let pred_b = sol_b[0] + sol_b[1] * dx + sol_b[2] * dy;

        let er = pred_r - r;
        let eg = pred_g - g;
        let eb = pred_b - b;

        sum_sq_err += er * er + eg * eg + eb * eb;
    }

    let residual = (sum_sq_err / n).sqrt();
    if residual > cfg.max_residual {
        return None;
    }

    Some(GradientFit {
        x1,
        y1,
        x2,
        y2,
        start: Rgb {
            r: start_r,
            g: start_g,
            b: start_b,
        },
        end: Rgb {
            r: end_r,
            g: end_g,
            b: end_b,
        },
        residual,
        sample_count,
    })
}

/// Mean colour of the core pixels in `rect`. The fallback when the gradient
/// fit is rejected. None when no core pixels exist.
pub fn dominant_color(
    image: &RasterImage,
    mask: &Mask,
    rect: &Bbox,
    core_sat_min: u8,
) -> Option<Rgb> {
    let mut count = 0u64;
    let mut sum_r = 0.0f64;
    let mut sum_g = 0.0f64;
    let mut sum_b = 0.0f64;

    for y in rect.y1..rect.y2 {
        for x in rect.x1..rect.x2 {
            if mask.get(x, y) {
                let idx = (y as usize * image.width as usize + x as usize) * 4;
                if idx + 3 < image.pixels.len() {
                    let r = image.pixels[idx];
                    let g = image.pixels[idx + 1];
                    let b = image.pixels[idx + 2];
                    if saturation(r, g, b) >= core_sat_min {
                        count += 1;
                        sum_r += r as f64;
                        sum_g += g as f64;
                        sum_b += b as f64;
                    }
                }
            }
        }
    }

    if count == 0 {
        return None;
    }

    let n = count as f64;
    let r = (sum_r / n).clamp(0.0, 255.0).round() as u8;
    let g = (sum_g / n).clamp(0.0, 255.0).round() as u8;
    let b = (sum_b / n).clamp(0.0, 255.0).round() as u8;

    Some(Rgb { r, g, b })
}
#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
