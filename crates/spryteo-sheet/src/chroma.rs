use spryteo_core::ir::RasterImage;

#[derive(Debug, Clone)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub bits: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct ChromaConfig {
    pub sat_min: u8,
    pub value_max: u8,
    pub alpha_min: u8,
}

impl Default for ChromaConfig {
    fn default() -> Self {
        ChromaConfig {
            sat_min: 90,
            value_max: 250,
            alpha_min: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SheetError {
    ChromaUnusable { ink_fraction: f32 },
}

impl std::fmt::Display for SheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SheetError::ChromaUnusable { ink_fraction } => {
                write!(
                    f,
                    "chroma segmentation unusable (ink fraction: {})",
                    ink_fraction
                )
            }
        }
    }
}

impl std::error::Error for SheetError {}

impl Mask {
    pub fn count(&self) -> usize {
        self.bits.iter().filter(|&&b| b).count()
    }

    pub fn fraction(&self) -> f32 {
        if self.bits.is_empty() {
            return 0.0;
        }
        self.count() as f32 / self.bits.len() as f32
    }

    pub fn get(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = (y * self.width + x) as usize;
        self.bits.get(idx).copied().unwrap_or(false)
    }
}

pub fn saturation(r: u8, g: u8, b: u8) -> u8 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    max - min
}

pub fn chroma_mask(image: &RasterImage, cfg: &ChromaConfig) -> Mask {
    let w = image.width;
    let h = image.height;
    let total = (w as usize) * (h as usize);
    let mut bits = Vec::with_capacity(total);

    for pixel in image.pixels.chunks_exact(4) {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        let a = pixel[3];
        let is_ink = a >= cfg.alpha_min
            && saturation(r, g, b) >= cfg.sat_min
            && r.max(g).max(b) <= cfg.value_max;
        bits.push(is_ink);
    }

    Mask {
        width: w,
        height: h,
        bits,
    }
}

pub fn auto_sat_threshold(image: &RasterImage) -> u8 {
    let mut hist = [0u64; 256];
    let cfg = ChromaConfig::default();

    for pixel in image.pixels.chunks_exact(4) {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        let a = pixel[3];
        if a >= cfg.alpha_min && r.max(g).max(b) <= cfg.value_max {
            let s = saturation(r, g, b);
            hist[s as usize] += 1;
        }
    }

    let total: u64 = hist.iter().sum();
    if total < 2 {
        return 90;
    }

    let non_empty = hist.iter().filter(|&&c| c > 0).count();
    if non_empty < 2 {
        return 90;
    }

    let mut best_var = -1.0_f64;
    let mut best_thresh = 90u8;

    for t in 0u16..=255u16 {
        let t_usize = t as usize;
        let w0: f64 = hist[..=t_usize].iter().sum::<u64>() as f64;
        let w1 = total as f64 - w0;
        if w0 == 0.0 || w1 == 0.0 {
            continue;
        }

        let mu0: f64 = hist[..=t_usize]
            .iter()
            .enumerate()
            .map(|(i, &c)| (i as f64) * (c as f64))
            .sum::<f64>()
            / w0;

        let mu1: f64 = hist[t_usize + 1..]
            .iter()
            .enumerate()
            .map(|(i, &c)| ((t_usize + 1 + i) as f64) * (c as f64))
            .sum::<f64>()
            / w1;

        let var = w0 * w1 * (mu0 - mu1) * (mu0 - mu1);
        if var > best_var {
            best_var = var;
            best_thresh = t as u8;
        }
    }

    best_thresh
}

pub fn ink_coverage(image: &RasterImage, sat_ref: u8, cfg: &ChromaConfig) -> Vec<f32> {
    let total = (image.width as usize) * (image.height as usize);
    let mut coverage = Vec::with_capacity(total);

    for pixel in image.pixels.chunks_exact(4) {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        let a = pixel[3];

        if a < cfg.alpha_min || sat_ref == 0 {
            coverage.push(0.0);
        } else {
            let s = saturation(r, g, b) as f32;
            coverage.push((s / sat_ref as f32).clamp(0.0, 1.0));
        }
    }

    coverage
}

pub fn check_chroma_usable(mask: &Mask) -> Result<f32, SheetError> {
    let frac = mask.fraction();
    if !(0.001..=0.30).contains(&frac) {
        Err(SheetError::ChromaUnusable { ink_fraction: frac })
    } else {
        Ok(frac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spryteo_core::ir::RasterImage;

    fn make_image(width: u32, height: u32, pixels: Vec<u8>) -> RasterImage {
        RasterImage {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn test_saturation_basics() {
        assert_eq!(saturation(128, 128, 128), 0);
        assert_eq!(saturation(255, 0, 0), 255);
        assert_eq!(saturation(255, 255, 255), 0);
        assert_eq!(saturation(0, 0, 0), 0);
    }

    #[test]
    fn test_chroma_mask_separates_colored_ink_from_grey_text() {
        let w: [u8; 4] = [255, 255, 255, 255];
        let c: [u8; 4] = [200, 24, 200, 255];
        let g: [u8; 4] = [90, 70, 46, 255];

        let mut pixels = Vec::new();
        let rows: &[&[[u8; 4]]] = &[&[w, w, w, w], &[w, c, c, w], &[w, g, g, w], &[w, w, w, w]];
        for row in rows {
            for pixel in *row {
                pixels.extend_from_slice(pixel);
            }
        }

        let img = make_image(4, 4, pixels);
        let mask = chroma_mask(&img, &ChromaConfig::default());

        assert_eq!(mask.count(), 2);
        assert!(mask.get(1, 1));
        assert!(mask.get(2, 1));
        assert!(!mask.get(1, 2));
        assert!(!mask.get(2, 2));
        assert!(!mask.get(0, 0));
        assert!(!mask.get(3, 3));
    }

    #[test]
    fn test_chroma_mask_rejects_transparent_and_near_white() {
        let transparent = [200, 24, 200, 0];
        let bright = [252, 252, 250, 255];
        let ink = [200, 24, 200, 255];

        let mut pixels = Vec::new();
        pixels.extend_from_slice(&transparent);
        pixels.extend_from_slice(&bright);
        pixels.extend_from_slice(&ink);

        let img = make_image(3, 1, pixels);
        let mask = chroma_mask(&img, &ChromaConfig::default());

        assert!(!mask.get(0, 0), "transparent pixel must not be ink");
        assert!(!mask.get(1, 0), "near-white pixel must not be ink");
        assert!(mask.get(2, 0), "saturated ink pixel must be ink");
    }

    #[test]
    fn test_ink_coverage_is_soft_and_lightness_independent() {
        let sat_ref: u8 = 100;
        let cfg = ChromaConfig::default();

        // Both of these have saturation 50 -- half of sat_ref -- but wildly
        // different brightness. They must agree at 0.5. Picking a value below
        // sat_ref matters: at or above it the clamp forces equality anyway, so
        // the test would pass even for a lightness-dependent implementation.
        let same_sat_light = [230u8, 180, 230, 255];
        let same_sat_dark = [80u8, 30, 80, 255];
        let at_ref = [200u8, 100, 200, 255];
        let above_ref = [200u8, 0, 200, 255];

        let mut pixels = Vec::new();
        pixels.extend_from_slice(&same_sat_light);
        pixels.extend_from_slice(&same_sat_dark);
        pixels.extend_from_slice(&at_ref);
        pixels.extend_from_slice(&above_ref);

        let img = make_image(4, 1, pixels);
        let cov = ink_coverage(&img, sat_ref, &cfg);

        assert_eq!(cov.len(), 4);
        assert!(
            (cov[0] - 0.5).abs() < 1e-6,
            "half sat_ref -> ~0.5, got {}",
            cov[0]
        );
        assert_eq!(
            cov[0], cov[1],
            "same saturation at different lightness must give identical coverage"
        );
        assert!((cov[2] - 1.0).abs() < 1e-6, "sat == sat_ref -> 1.0");
        assert!((cov[3] - 1.0).abs() < 1e-6, "above sat_ref -> clamp to 1.0");
    }

    #[test]
    fn test_ink_coverage_zero_sat_ref_does_not_divide_by_zero() {
        let pixels = vec![200u8, 24, 200, 255];
        let img = make_image(1, 1, pixels);
        let cfg = ChromaConfig::default();
        let cov = ink_coverage(&img, 0, &cfg);

        assert_eq!(cov, vec![0.0]);
    }

    #[test]
    fn test_auto_sat_threshold_lands_between_two_populations() {
        let low1 = [70u8, 40, 30, 255]; // sat 40
        let low2 = [90u8, 70, 46, 255]; // sat 44
        let low3 = [100u8, 70, 52, 255]; // sat 48
        let high = [200u8, 24, 200, 255]; // sat 176

        let mut pixels = Vec::new();
        pixels.extend_from_slice(&low1);
        pixels.extend_from_slice(&low2);
        pixels.extend_from_slice(&low3);
        for _ in 0..3 {
            pixels.extend_from_slice(&high);
        }

        let img = make_image(6, 1, pixels);
        let t = auto_sat_threshold(&img);

        assert!(
            t > 44,
            "threshold must be above low cluster (~44), got {}",
            t
        );
        assert!(
            t < 176,
            "threshold must be below high cluster (~176), got {}",
            t
        );
    }

    #[test]
    fn test_check_chroma_usable_bounds() {
        let empty = Mask {
            width: 4,
            height: 4,
            bits: vec![false; 16],
        };
        assert!(check_chroma_usable(&empty).is_err());

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
        let frac = check_chroma_usable(&ok).unwrap();
        assert!((frac - 0.024).abs() < 0.001);

        let too_much = Mask {
            width: 4,
            height: 4,
            bits: vec![true; 16],
        };
        let err = check_chroma_usable(&too_much).unwrap_err();
        assert_eq!(err, SheetError::ChromaUnusable { ink_fraction: 1.0 });
    }

    #[test]
    fn test_zero_size_image_does_not_panic() {
        let img = make_image(0, 0, vec![]);
        let cfg = ChromaConfig::default();

        let mask = chroma_mask(&img, &cfg);
        assert_eq!(mask.count(), 0);

        let cov = ink_coverage(&img, 176, &cfg);
        assert!(cov.is_empty());

        let t = auto_sat_threshold(&img);
        assert_eq!(t, 90);
    }
}
