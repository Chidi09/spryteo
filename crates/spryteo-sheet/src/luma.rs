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
    for pixel in image.pixels.chunks_exact(4) {
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

    for pixel in image.pixels.chunks_exact(4) {
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
mod tests {
    use super::*;

    fn make_image(width: u32, height: u32, pixels: Vec<u8>) -> RasterImage {
        RasterImage {
            width,
            height,
            pixels,
        }
    }

    fn pixel(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        [r, g, b, a]
    }

    fn lum_from_rgb(r: u8, g: u8, b: u8) -> f32 {
        luminance(r, g, b)
    }

    #[test]
    fn test_luminance_basic() {
        assert_eq!(luminance(0, 0, 0), 0.0);
        assert_eq!(luminance(255, 255, 255), 255.0);
        let v = luminance(128, 128, 128);
        assert!((v - 128.0).abs() < 0.1);
    }

    #[test]
    fn test_dark_strokes_on_white_and_inverse_produce_identical_mask() {
        let w: u32 = 10;
        let h: u32 = 12;
        let white = pixel(255, 255, 255, 255);
        let stroke = pixel(60, 60, 60, 255);

        let inv_bg = pixel(0, 0, 0, 255);
        let inv_stroke = pixel(195, 195, 195, 255);

        let stroke_rows = 6u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if y < stroke_rows && (2..8).contains(&x) {
                    pixels.extend_from_slice(&stroke);
                } else {
                    pixels.extend_from_slice(&white);
                }
            }
        }
        let img = make_image(w, h, pixels);

        let mut inv_pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if y < stroke_rows && (2..8).contains(&x) {
                    inv_pixels.extend_from_slice(&inv_stroke);
                } else {
                    inv_pixels.extend_from_slice(&inv_bg);
                }
            }
        }
        let inv_img = make_image(w, h, inv_pixels);

        let cfg = LumaConfig::default();
        let (mask, info) = luma_mask(&img, &cfg);
        let (inv_mask, inv_info) = luma_mask(&inv_img, &cfg);

        assert_eq!(
            mask.bits, inv_mask.bits,
            "mask must be identical for original and inverse"
        );
        assert!(info.dark_on_light, "white background must be dark_on_light");
        assert!(
            !inv_info.dark_on_light,
            "black background must not be dark_on_light"
        );
        assert!(
            (info.background - 255.0).abs() < 5.0,
            "background must be ~255 for original"
        );
        assert!(
            (inv_info.background - 0.0).abs() < 5.0,
            "background must be ~0 for inverse"
        );
    }

    #[test]
    fn test_lightness_independence_with_discrimination_power() {
        let white = pixel(255, 255, 255, 255);
        let dark_stroke = pixel(60, 60, 60, 255);
        let light_stroke = pixel(190, 190, 190, 255);

        assert!(
            (lum_from_rgb(60, 60, 60) - lum_from_rgb(190, 190, 190)).abs() > 10.0,
            "two stroke luminances must be distinct"
        );

        let mut pixels = Vec::new();
        pixels.extend_from_slice(&dark_stroke);
        pixels.extend_from_slice(&light_stroke);
        pixels.extend_from_slice(&white);
        let img = make_image(3, 1, pixels);

        let cfg = LumaConfig::default();
        let (mask, _info) = luma_mask(&img, &cfg);

        assert!(mask.get(0, 0), "dark stroke (lum ~60) must be in mask");
        assert!(mask.get(1, 0), "light stroke (lum ~190) must be in mask");
        assert!(!mask.get(2, 0), "white background must not be in mask");
    }

    #[test]
    fn test_transparent_pixels_never_ink() {
        let transparent_black = pixel(0, 0, 0, 0);
        let ink = pixel(60, 60, 60, 255);
        let white = pixel(255, 255, 255, 255);

        let mut pixels = Vec::new();
        pixels.extend_from_slice(&transparent_black);
        pixels.extend_from_slice(&ink);
        pixels.extend_from_slice(&white);
        let img = make_image(3, 1, pixels);

        let cfg = LumaConfig::default();
        let (mask, _info) = luma_mask(&img, &cfg);

        assert!(!mask.get(0, 0), "transparent black must not be ink");
        assert!(mask.get(1, 0), "ink must be in mask");
        assert!(!mask.get(2, 0), "white must not be in mask");
    }

    #[test]
    fn test_uniform_image_returns_luma_unusable() {
        let white = pixel(255, 255, 255, 255);
        let mut pixels = Vec::new();
        for _ in 0..16 {
            pixels.extend_from_slice(&white);
        }
        let img = make_image(4, 4, pixels);
        let cfg = LumaConfig::default();
        let (mask, _info) = luma_mask(&img, &cfg);
        let err = check_luma_usable(&mask).unwrap_err();
        assert_eq!(err, SheetError::LumaUnusable { ink_fraction: 0.0 });
    }

    #[test]
    fn test_background_luminance_uses_only_border_ring() {
        let w: u32 = 10;
        let h: u32 = 10;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let is_border = y < 2 || y + 2 >= h || x < 2 || x + 2 >= w;
                if is_border {
                    pixels.extend_from_slice(&[255, 255, 255, 255]);
                } else {
                    pixels.extend_from_slice(&[50, 50, 50, 255]);
                }
            }
        }
        let img = make_image(w, h, pixels);
        let cfg = LumaConfig::default();
        let bg = background_luminance(&img, &cfg);
        assert!(
            (bg - 255.0).abs() < 1.0,
            "background must be ~255 from border, got {}",
            bg
        );
    }

    #[test]
    fn test_background_luminance_fewer_than_16_pixels_returns_255() {
        let img = make_image(1, 1, vec![100, 100, 100, 255]);
        let cfg = LumaConfig::default();
        let bg = background_luminance(&img, &cfg);
        assert_eq!(
            bg, 255.0,
            "must return 255 when fewer than 16 border pixels"
        );
    }

    #[test]
    fn test_check_luma_usable_bounds() {
        let empty = Mask {
            width: 4,
            height: 4,
            bits: vec![false; 16],
        };
        assert!(check_luma_usable(&empty).is_err());

        let ok = Mask {
            width: 100,
            height: 100,
            bits: {
                let mut b = vec![false; 10000];
                for b in b.iter_mut().take(240) {
                    *b = true;
                }
                b
            },
        };
        let frac = check_luma_usable(&ok).unwrap();
        assert!((frac - 0.024).abs() < 0.001);

        let too_much = Mask {
            width: 4,
            height: 4,
            bits: vec![true; 16],
        };
        let err = check_luma_usable(&too_much).unwrap_err();
        assert_eq!(err, SheetError::LumaUnusable { ink_fraction: 1.0 });
    }

    #[test]
    fn test_luma_coverage_soft_and_bounded() {
        let white = pixel(255, 255, 255, 255);
        let ink = pixel(60, 60, 60, 255);
        let pixels = [ink, white].concat().to_vec();
        let img = make_image(2, 1, pixels);
        let cfg = LumaConfig::default();
        let (mask, info) = luma_mask(&img, &cfg);
        let cov = luma_coverage(&img, &info, &cfg);

        assert_eq!(cov.len(), 2);
        assert!(
            cov[0] > 0.5,
            "ink pixel must have high coverage, got {}",
            cov[0]
        );
        assert!((cov[1]).abs() < 1e-6, "background must have zero coverage");

        assert!(mask.get(0, 0), "ink pixel must be in mask");
        assert!(!mask.get(1, 0), "background pixel must not be in mask");
    }

    #[test]
    fn test_zero_size_image_does_not_panic() {
        let img = make_image(0, 0, vec![]);
        let cfg = LumaConfig::default();
        let (mask, info) = luma_mask(&img, &cfg);
        assert_eq!(mask.count(), 0);
        assert!((info.background - 255.0).abs() < 0.1);
        assert!(info.dark_on_light);
        assert!((info.ink_ref - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_dominant_ink_color() {
        let white = pixel(255, 255, 255, 255);
        let ink1 = pixel(50, 60, 70, 255);
        let ink2 = pixel(40, 50, 60, 255);
        let ink3 = pixel(60, 70, 80, 255);
        let pixels = [white, ink1, ink2, ink3].concat();
        let img = make_image(4, 1, pixels);
        let cfg = LumaConfig::default();
        let info = analyze_luma(&img, &cfg);
        let dom = dominant_ink_color(&img, &info, &cfg);
        assert_eq!(
            dom,
            Some(Rgb {
                r: 50,
                g: 60,
                b: 70
            })
        );
    }
}
