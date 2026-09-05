use crate::chroma::{Mask, SheetError};
use spryteo_core::ir::{RasterImage, Rgb};

/// Configuration parameters for luminance-based segmentation.
#[derive(Debug, Clone)]
pub struct LumaConfig {
    /// Minimum luminance difference from background to consider as ink candidate.
    pub d_min: f32,
    /// Minimum alpha value for a pixel to be evaluated.
    pub alpha_min: u8,
}

impl Default for LumaConfig {
    fn default() -> Self {
        LumaConfig {
            d_min: 40.0,
            alpha_min: 10,
        }
    }
}

/// Analysis results of image luminance distribution and polarity.
#[derive(Debug, Clone)]
pub struct LumaInfo {
    /// Estimated background luminance level.
    pub background: f32,
    /// True if the image has dark ink on a light background.
    pub dark_on_light: bool,
    /// 10th-percentile luminance distance among ink candidates, used as reference contrast.
    pub ink_ref: f32,
}

/// Computes Rec. 709 relative luminance for an RGB pixel in 0.0..=255.0.
pub fn luminance(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32)
}

/// Estimates background luminance by taking the median luminance of border pixels.
pub fn background_luminance(image: &RasterImage, cfg: &LumaConfig) -> f32 {
    let w = image.width;
    let h = image.height;
    let mut vals: Vec<f32> = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let is_border = y < 2 || y + 2 >= h || x < 2 || x + 2 >= w;
            if !is_border {
                continue;
            }
            let idx = ((y * w + x) * 4) as usize;
            if idx + 3 >= image.pixels.len() {
                continue;
            }
            let a = image.pixels[idx + 3];
            if a < cfg.alpha_min {
                continue;
            }
            let r = image.pixels[idx];
            let g = image.pixels[idx + 1];
            let b = image.pixels[idx + 2];
            vals.push(luminance(r, g, b));
        }
    }

    if vals.len() < 16 {
        return 255.0;
    }

    vals.sort_unstable_by(|a, b| a.total_cmp(b));
    vals[(vals.len() - 1) / 2]
}

/// Analyzes an image to compute background luminance, dark/light polarity, and reference ink contrast.
pub fn analyze_luma(image: &RasterImage, cfg: &LumaConfig) -> LumaInfo {
    let background = background_luminance(image, cfg);
    let dark_on_light = background >= 128.0;

    let mut d_vals: Vec<f32> = Vec::new();
    for pixel in image.pixels.as_chunks::<4>().0 {
        let a = pixel[3];
        if a < cfg.alpha_min {
            continue;
        }
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        let lum = luminance(r, g, b);
        let d = (lum - background).abs();
        if d > cfg.d_min {
            d_vals.push(d);
        }
    }

    let ink_ref = if d_vals.is_empty() {
        0.0
    } else {
        d_vals.sort_unstable_by(|a, b| a.total_cmp(b));
        let idx = ((d_vals.len() - 1) as f32 * 0.10).round() as usize;
        d_vals[idx]
    };

    LumaInfo {
        background,
        dark_on_light,
        ink_ref,
    }
}

/// Computes a continuous coverage map (0.0..=1.0 per pixel) based on luminance distance to background.
pub fn luma_coverage(image: &RasterImage, info: &LumaInfo, cfg: &LumaConfig) -> Vec<f32> {
    let total = (image.width as usize) * (image.height as usize);
    let mut coverage = Vec::with_capacity(total);

    for pixel in image.pixels.as_chunks::<4>().0 {
        let a = pixel[3];
        if a < cfg.alpha_min || info.ink_ref <= 0.0 {
            coverage.push(0.0);
        } else {
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];
            let lum = luminance(r, g, b);
            let d = (lum - info.background).abs();
            coverage.push((d / info.ink_ref).clamp(0.0, 1.0));
        }
    }

    coverage
}

/// Generates a binary mask and luminance info for an image using luminance analysis.
pub fn luma_mask(image: &RasterImage, cfg: &LumaConfig) -> (Mask, LumaInfo) {
    let info = analyze_luma(image, cfg);
    let cov = luma_coverage(image, &info, cfg);
    let bits: Vec<bool> = cov.iter().map(|&c| c > 0.5).collect();
    let mask = Mask {
        width: image.width,
        height: image.height,
        bits,
    };
    (mask, info)
}

/// Checks whether a luminance mask is usable based on its ink fraction (must be in 0.1%..=30%).
pub fn check_luma_usable(mask: &Mask) -> Result<f32, SheetError> {
    let frac = mask.fraction();
    if !(0.001..=0.30).contains(&frac) {
        Err(SheetError::LumaUnusable { ink_fraction: frac })
    } else {
        Ok(frac)
    }
}

/// Computes the dominant ink color by taking the per-channel median of RGB values
/// over pixels with luminance coverage >= 0.5. Returns None if no such pixels exist.
pub fn dominant_ink_color(image: &RasterImage, info: &LumaInfo, cfg: &LumaConfig) -> Option<Rgb> {
    let cov = luma_coverage(image, info, cfg);
    let mut r_vals = Vec::new();
    let mut g_vals = Vec::new();
    let mut b_vals = Vec::new();

    for (i, &c) in cov.iter().enumerate() {
        if c >= 0.5 {
            let idx = i * 4;
            if idx + 2 < image.pixels.len() {
                r_vals.push(image.pixels[idx]);
                g_vals.push(image.pixels[idx + 1]);
                b_vals.push(image.pixels[idx + 2]);
            }
        }
    }

    if r_vals.is_empty() {
        return None;
    }

    r_vals.sort_unstable();
    g_vals.sort_unstable();
    b_vals.sort_unstable();

    let mid = (r_vals.len() - 1) / 2;
    Some(Rgb {
        r: r_vals[mid],
        g: g_vals[mid],
        b: b_vals[mid],
    })
}
#[cfg(test)]
#[path = "luma_tests.rs"]
mod tests;
