use spryteo_core::ir::RasterImage;

/// Converts a `RasterImage` into a binary mask of ink pixels (true = ink, false = background).
///
/// Luminance is computed per pixel as 0.299*R + 0.587*G + 0.114*B.
/// Fully transparent pixels (alpha < 10) are treated as background.
/// Sauvola adaptive thresholding is applied with a window size of 21, k = 0.5, and R = 128.
pub fn binarize(image: &RasterImage) -> Vec<bool> {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut ink_mask = vec![false; w * h];
    if w == 0 || h == 0 {
        return ink_mask;
    }

    // 1. Compute luminance for each pixel
    let mut luminance = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let r = image.pixels[idx] as f64;
            let g = image.pixels[idx + 1] as f64;
            let b = image.pixels[idx + 2] as f64;
            let a = image.pixels[idx + 3];
            if a < 10 {
                // Fully transparent is background. Assign max luminance (255.0) so it won't be ink.
                luminance[y * w + x] = 255.0;
            } else {
                luminance[y * w + x] = 0.299 * r + 0.587 * g + 0.114 * b;
            }
        }
    }

    // 2. Build summed-area tables for O(1) local window mean/stddev
    let mut integral = vec![0.0; (w + 1) * (h + 1)];
    let mut integral_sq = vec![0.0; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            let lum = luminance[y * w + x];
            let idx = (y + 1) * (w + 1) + (x + 1);
            let left = (y + 1) * (w + 1) + x;
            let up = y * (w + 1) + (x + 1);
            let up_left = y * (w + 1) + x;

            integral[idx] = lum + integral[left] + integral[up] - integral[up_left];
            integral_sq[idx] =
                (lum * lum) + integral_sq[left] + integral_sq[up] - integral_sq[up_left];
        }
    }

    let query_sum = |x1: usize, y1: usize, x2: usize, y2: usize| -> f64 {
        let idx_br = (y2 + 1) * (w + 1) + (x2 + 1);
        let idx_tr = y1 * (w + 1) + (x2 + 1);
        let idx_bl = (y2 + 1) * (w + 1) + x1;
        let idx_tl = y1 * (w + 1) + x1;
        integral[idx_br] - integral[idx_tr] - integral[idx_bl] + integral[idx_tl]
    };
    let query_sum_sq = |x1: usize, y1: usize, x2: usize, y2: usize| -> f64 {
        let idx_br = (y2 + 1) * (w + 1) + (x2 + 1);
        let idx_tr = y1 * (w + 1) + (x2 + 1);
        let idx_bl = (y2 + 1) * (w + 1) + x1;
        let idx_tl = y1 * (w + 1) + x1;
        integral_sq[idx_br] - integral_sq[idx_tr] - integral_sq[idx_bl] + integral_sq[idx_tl]
    };

    // 3. Apply Sauvola thresholding
    let window_size = 21;
    let r_rad = window_size / 2;
    let k = 0.5;
    let r_const = 128.0;

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            let a = image.pixels[idx * 4 + 3];
            if a < 10 {
                ink_mask[idx] = false;
                continue;
            }

            let x1 = x.saturating_sub(r_rad);
            let x2 = (x + r_rad).min(w - 1);
            let y1 = y.saturating_sub(r_rad);
            let y2 = (y + r_rad).min(h - 1);

            let count = ((x2 - x1 + 1) * (y2 - y1 + 1)) as f64;
            let sum = query_sum(x1, y1, x2, y2);
            let sum_sq = query_sum_sq(x1, y1, x2, y2);

            let mean = sum / count;
            let variance = (sum_sq - (sum * sum) / count) / count;
            let stddev = variance.max(0.0).sqrt();

            let threshold = mean * (1.0 + k * (stddev / r_const - 1.0));
            if luminance[idx] < threshold {
                ink_mask[idx] = true;
            }
        }
    }

    ink_mask
}
