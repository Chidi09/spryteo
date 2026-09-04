//! Input classifier (§3.2). Determines whether an image is pixel-art, icon,
//! line-art, or photo based on configurable heuristics.
//!
//! ## Classification priority
//! 1. **Pixel-art**: dimension ≤ 128 AND unique colours ≤ 64 AND no
//!    anti-aliasing detected.  Also requires the image to have no alpha
//!    channel (all pixels fully opaque) — opacity implies icon-like,
//!    not pixel-art.
//! 2. **Line-art**: after adaptive threshold, ink ratio < 25%.  The
//!    full §3.2 spec also requires a unimodal stroke-width histogram;
//!    that check is **omitted** in this Phase 1 implementation
//!    (this is a documented simplification).
//! 3. **Icon**: unique colours after 1% noise floor ≤ 32, or an alpha
//!    channel whose fills are flat (binary transparency or single
//!    uniform opacity).
//! 4. **Photo**: fallback when nothing else matches.
//!
//! Note: Line-art is checked before Icon so that sparse 2-colour line
//! drawings (low ink ratio) are classified as LineArt rather than Icon.

use spryteo_core::{ClassifiedInput, Mode, RasterImage, Rgb};

use crate::color;

/// Classify an input image, applying `forced_mode` if not `Mode::Auto`.
///
/// Background detection (§3.3) runs regardless of forced/auto mode.
pub fn classify(image: RasterImage, forced_mode: &Mode) -> ClassifiedInput {
    let background_color = detect_background(&image);

    let mode = match forced_mode {
        Mode::Auto => classify_auto(&image),
        other => other.clone(),
    };

    ClassifiedInput {
        image,
        mode,
        background_color,
    }
}

/// Run the automatic classifier heuristics in priority order.
fn classify_auto(image: &RasterImage) -> Mode {
    if is_pixel_art(image) {
        return Mode::PixelArt;
    }
    if is_line_art(image) {
        return Mode::LineArt;
    }
    if is_icon(image) {
        return Mode::Icon;
    }
    Mode::Photo
}

// ── Pixel-art detection ──────────────────────────────────────────────────────

/// Check whether the image is pixel-art:
/// dimension ≤ 128, unique colours 3..=64, no anti-aliasing, no alpha channel.
///
/// The minimum-colour check (≥ 3) distinguishes pixel art from duotone line
/// drawings (black ink on white paper), which would otherwise satisfy the
/// criterion but are conceptually line-art rather than pixel-art.
fn is_pixel_art(image: &RasterImage) -> bool {
    if image.width > 128 || image.height > 128 {
        return false;
    }

    let unique = count_unique_rgb(image);
    if !(3..=64).contains(&unique) {
        return false;
    }

    // Pixel art is fully opaque — alpha implies icon-like transparency
    if has_any_alpha(image) {
        return false;
    }

    !has_anti_aliasing(image)
}

/// Returns `true` if any pixel has alpha ≠ 255.
fn has_any_alpha(image: &RasterImage) -> bool {
    let total = (image.width * image.height) as usize;
    for i in 0..total {
        if image.pixels[i * 4 + 3] != 255 {
            return true;
        }
    }
    false
}

/// Count distinct sRGB triples in the image.
fn count_unique_rgb(image: &RasterImage) -> usize {
    let total = (image.width * image.height) as usize;
    let mut colors: Vec<(u8, u8, u8)> = Vec::new();
    for i in 0..total {
        let idx = i * 4;
        let c = (
            image.pixels[idx],
            image.pixels[idx + 1],
            image.pixels[idx + 2],
        );
        if !colors.contains(&c) {
            colors.push(c);
        }
    }
    colors.len()
}

/// Detect whether the image has anti-aliased edges.
///
/// For each pixel that sits on a colour boundary (has a neighbour of a
/// different colour), we check whether its colour is a *linear blend* of
/// two other colours in its 8-connected neighbourhood.  If such a blended
/// pixel exists, the image uses anti-aliasing.
///
/// Pure hard-edge transitions between two colours never produce
/// intermediate pixel colours, so this correctly identifies AA.
fn has_anti_aliasing(image: &RasterImage) -> bool {
    let pixels = &image.pixels;
    let w = image.width as usize;
    let h = image.height as usize;

    let at = |x: i32, y: i32| -> (u8, u8, u8) {
        let idx = (y as usize * w + x as usize) * 4;
        (pixels[idx], pixels[idx + 1], pixels[idx + 2])
    };

    for y in 0..h {
        for x in 0..w {
            let c = at(x as i32, y as i32);
            let mut neighbours: Vec<(u8, u8, u8)> = Vec::new();
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                        let nc = at(nx, ny);
                        if nc != c && !neighbours.contains(&nc) {
                            neighbours.push(nc);
                        }
                    }
                }
            }
            if neighbours.len() < 2 {
                continue;
            }
            for i in 0..neighbours.len() {
                for j in (i + 1)..neighbours.len() {
                    if is_rgb_blend(c, neighbours[i], neighbours[j]) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check whether `candidate` is a linear blend of two other colours `a` and
/// `b` in sRGB space.  We solve for the blend factor α in each channel and
/// verify that the three channels agree on a common α in the open interval
/// (0, 1).
fn is_rgb_blend(candidate: (u8, u8, u8), a: (u8, u8, u8), b: (u8, u8, u8)) -> bool {
    let tolerance = 15.0;

    let cr = candidate.0 as f64;
    let cg = candidate.1 as f64;
    let cb = candidate.2 as f64;
    let ar = a.0 as f64;
    let ag = a.1 as f64;
    let ab = a.2 as f64;
    let br = b.0 as f64;
    let bg = b.1 as f64;
    let bb = b.2 as f64;

    let alpha_r = if (ar - br).abs() > 1.0 {
        (cr - br) / (ar - br)
    } else {
        -1.0
    };
    let alpha_g = if (ag - bg).abs() > 1.0 {
        (cg - bg) / (ag - bg)
    } else {
        -1.0
    };
    let alpha_b = if (ab - bb).abs() > 1.0 {
        (cb - bb) / (ab - bb)
    } else {
        -1.0
    };

    let alphas: Vec<f64> = [alpha_r, alpha_g, alpha_b]
        .iter()
        .filter(|&&a| (-0.2..=1.2).contains(&a))
        .copied()
        .collect();

    if alphas.len() < 2 {
        return false;
    }

    let mean = alphas.iter().sum::<f64>() / alphas.len() as f64;
    if mean <= 0.01 || mean >= 0.99 {
        return false;
    }

    let pred_r = mean * ar + (1.0 - mean) * br;
    let pred_g = mean * ag + (1.0 - mean) * bg;
    let pred_b = mean * ab + (1.0 - mean) * bb;

    let max_dev = (cr - pred_r)
        .abs()
        .max((cg - pred_g).abs())
        .max((cb - pred_b).abs());

    max_dev < tolerance
}

// ── Icon detection ───────────────────────────────────────────────────────────

/// Check whether the image is an icon:
/// unique colours after 1% noise floor ≤ 32, or alpha channel with flat fills.
fn is_icon(image: &RasterImage) -> bool {
    if has_flat_alpha_fills(image) {
        return true;
    }

    let after_noise = count_rgb_after_noise_floor(image, 0.01);
    after_noise <= 32
}

/// Count unique sRGB triples after filtering out colours that appear in
/// fewer than `noise_floor` fraction of the total pixels.
fn count_rgb_after_noise_floor(image: &RasterImage, noise_floor: f64) -> usize {
    let total = (image.width * image.height) as usize;
    let min_count = (total as f64 * noise_floor).ceil() as usize;

    // Use Vec lookup — no HashMap to keep deterministic output
    // (this function only returns a count, so order doesn't leak,
    //  but we stick with Vec for consistency with crate conventions).
    let mut colors: Vec<(u8, u8, u8)> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();

    for i in 0..total {
        let idx = i * 4;
        let c = (
            image.pixels[idx],
            image.pixels[idx + 1],
            image.pixels[idx + 2],
        );
        if let Some(pos) = colors.iter().position(|&x| x == c) {
            counts[pos] += 1;
        } else {
            colors.push(c);
            counts.push(1);
        }
    }

    colors
        .iter()
        .enumerate()
        .filter(|&(i, _)| counts[i] >= min_count)
        .count()
}

/// Check whether the image has an alpha channel with "flat" (non-gradient)
/// fills.  Returns `true` when the distinct non-zero alpha values number
/// ≤ 2, suggesting binary transparency or a single uniform opacity level.
fn has_flat_alpha_fills(image: &RasterImage) -> bool {
    let total = (image.width * image.height) as usize;
    let mut has_alpha = false;
    let mut alpha_values: Vec<u8> = Vec::new();

    for i in 0..total {
        let a = image.pixels[i * 4 + 3];
        if a != 255 {
            has_alpha = true;
            if a > 0 && !alpha_values.contains(&a) {
                alpha_values.push(a);
            }
        }
    }

    if !has_alpha {
        return false;
    }

    alpha_values.len() <= 2
}

// ── Line-art detection (simplified) ──────────────────────────────────────────

/// Simplified line-art detection: after adaptive threshold, ink ratio < 25%.
///
/// **Note:** The full §3.2 spec also requires a unimodal stroke-width
/// histogram.  That check is omitted here — this is a documented
/// simplification for Phase 1.  Only the ink-ratio portion is implemented.
fn is_line_art(image: &RasterImage) -> bool {
    let block_size = {
        let dim = std::cmp::min(image.width, image.height) as usize;
        let b = std::cmp::max(3, dim / 8);
        if b.is_multiple_of(2) {
            b + 1
        } else {
            b
        }
    };

    let binary = adaptive_threshold(image, block_size, 10.0);

    let total = (image.width * image.height) as usize;
    let ink = binary.iter().filter(|&&b| b == 0).count();
    let ink_ratio = ink as f64 / total as f64;

    ink_ratio < 0.25
}

/// Apply a local-mean adaptive threshold to produce a binary image.
/// Returns a flat `Vec<u8>` where `0` = ink (dark) and `255` = paper (light).
fn adaptive_threshold(image: &RasterImage, block_size: usize, offset: f64) -> Vec<u8> {
    let pixels = &image.pixels;
    let w = image.width as usize;
    let h = image.height as usize;
    let half = block_size / 2;

    // Luminance (simple weighted sum, not full Rec.709)
    let mut luminance: Vec<f64> = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            let l = 0.299 * pixels[idx] as f64
                + 0.587 * pixels[idx + 1] as f64
                + 0.114 * pixels[idx + 2] as f64;
            luminance.push(l);
        }
    }

    // Integral image for O(1) block-mean queries
    let mut integral = vec![0.0; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row_sum = 0.0;
        for x in 0..w {
            row_sum += luminance[y * w + x];
            integral[(y + 1) * (w + 1) + x + 1] = integral[y * (w + 1) + x + 1] + row_sum;
        }
    }

    let mut binary = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let x1 = x.saturating_sub(half);
            let x2 = std::cmp::min(x + half, w - 1);
            let y1 = y.saturating_sub(half);
            let y2 = std::cmp::min(y + half, h - 1);

            let area = ((x2 - x1 + 1) * (y2 - y1 + 1)) as f64;

            let sum = integral[(y2 + 1) * (w + 1) + x2 + 1]
                - integral[(y2 + 1) * (w + 1) + x1]
                - integral[y1 * (w + 1) + x2 + 1]
                + integral[y1 * (w + 1) + x1];

            let mean = sum / area;
            let threshold = mean - offset;

            if luminance[y * w + x] < threshold {
                binary.push(0);
            } else {
                binary.push(255);
            }
        }
    }

    binary
}

// ── Background detection (§3.3) ──────────────────────────────────────────────

/// Detect the background colour by sampling the four image corners.
///
/// The single pixel at each corner is read; if at least 3 of the 4 corner
/// colours agree (within a tolerance of 5 CIELAB distance units), that
/// colour is returned as the detected background.  Otherwise `None`.
#[allow(clippy::needless_range_loop)] // fixed-size (4-corner) pairwise index comparison
pub fn detect_background(image: &RasterImage) -> Option<Rgb> {
    let pixels = &image.pixels;
    let w = image.width;
    let h = image.height;

    // Bail if the image is trivially small
    if w == 0 || h == 0 {
        return None;
    }

    let corners = [
        corner_pixel(pixels, w, h, 0, 0),
        corner_pixel(pixels, w, h, w - 1, 0),
        corner_pixel(pixels, w, h, 0, h - 1),
        corner_pixel(pixels, w, h, w - 1, h - 1),
    ];

    let threshold_sq = 5.0; // Lab distance squared

    for i in 0..4 {
        let ci = corners[i];
        let lab_i = color::srgb_to_lab(&ci);
        let mut agreeing = 1;

        for j in (i + 1)..4 {
            let cj = corners[j];
            let lab_j = color::srgb_to_lab(&cj);
            if color::lab_distance_sq(&lab_i, &lab_j) < threshold_sq {
                agreeing += 1;
            }
        }

        if agreeing >= 3 {
            return Some(ci);
        }
    }

    None
}

/// Return the sRGB colour of the pixel at `(px, py)`.
fn corner_pixel(pixels: &[u8], width: u32, _height: u32, px: u32, py: u32) -> Rgb {
    let idx = ((py * width + px) * 4) as usize;
    Rgb {
        r: pixels[idx],
        g: pixels[idx + 1],
        b: pixels[idx + 2],
    }
}
#[cfg(test)]
#[path = "classifier_tests.rs"]
mod tests;
