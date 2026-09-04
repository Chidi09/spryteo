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
    LumaUnusable { ink_fraction: f32 },
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
            SheetError::LumaUnusable { ink_fraction } => {
                write!(
                    f,
                    "luma segmentation unusable (ink fraction: {})",
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
#[path = "chroma_tests.rs"]
mod tests;
