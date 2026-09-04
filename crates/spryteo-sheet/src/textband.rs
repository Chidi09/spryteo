use crate::chroma::Mask;
use crate::lattice::{self, Axis, Band};

/// Configuration parameters for text row classification.
#[derive(Debug, Clone)]
pub struct TextBandConfig {
    /// Minimum height ratio between tall (icon) and short (text/header) rows.
    pub height_ratio_min: f32,
    /// Maximum fraction of band ink allowed outside column bands before a short row is text.
    pub outside_frac_max: f32,
    /// Minimum height (in pixels) for a Y projection run to form a band.
    pub min_run: u32,
    /// Minimum gap (in pixels) between Y projection runs.
    pub min_gap: u32,
    /// Minimum projection value for a row/column to count as inked. Matches
    /// `LatticeConfig`'s floor of 2 so a single stray anti-aliased pixel
    /// cannot bridge two bands.
    pub min_profile: u32,
}

impl Default for TextBandConfig {
    fn default() -> Self {
        TextBandConfig {
            height_ratio_min: 1.8,
            outside_frac_max: 0.05,
            min_run: 3,
            min_gap: 3,
            min_profile: 2,
        }
    }
}

/// Classified row bands of a contact sheet.
#[derive(Debug, Clone)]
pub struct TextBands {
    /// Icon row bands (tall rows plus short rows that aligned with columns), sorted by `start`.
    pub icon: Vec<Band>,
    /// Text row bands (short rows whose ink fell outside column bounds).
    pub text: Vec<Band>,
    /// Short row bands whose ink was column-aligned and were retained as icons.
    pub kept_short: Vec<Band>,
}

/// Classifies row bands in a monochrome mask as icons or text.
pub fn classify_rows(mask: &Mask, cfg: &TextBandConfig) -> TextBands {
    let y_profile = lattice::project(mask, Axis::Y);
    let bands = lattice::find_bands(&y_profile, cfg.min_run, cfg.min_gap, cfg.min_profile);

    if bands.len() < 3 {
        return TextBands {
            icon: bands,
            text: Vec::new(),
            kept_short: Vec::new(),
        };
    }

    let n = bands.len();
    let mut sorted_heights: Vec<u32> = bands.iter().map(|b| b.len()).collect();
    sorted_heights.sort_unstable();

    let mut best_ratio = -1.0_f32;
    let mut best_k = 1;

    for k in 1..n {
        let short_sum: u32 = sorted_heights[0..k].iter().sum();
        let tall_sum: u32 = sorted_heights[k..n].iter().sum();
        let mean_short = short_sum as f32 / k as f32;
        let mean_tall = tall_sum as f32 / (n - k) as f32;
        let ratio = mean_tall / mean_short;

        if ratio.total_cmp(&best_ratio).is_gt() {
            best_ratio = ratio;
            best_k = k;
        }
    }

    if best_ratio < cfg.height_ratio_min {
        return TextBands {
            icon: bands,
            text: Vec::new(),
            kept_short: Vec::new(),
        };
    }

    let max_short_height = sorted_heights[best_k - 1];

    let mut short_bands = Vec::new();
    let mut tall_bands = Vec::new();

    for band in &bands {
        if band.len() <= max_short_height {
            short_bands.push(*band);
        } else {
            tall_bands.push(*band);
        }
    }

    let mut mask_tall = mask.clone();
    for y in 0..mask_tall.height {
        let in_tall = tall_bands.iter().any(|b| y >= b.start && y < b.end);
        if !in_tall {
            for x in 0..mask_tall.width {
                let idx = (y * mask_tall.width + x) as usize;
                mask_tall.bits[idx] = false;
            }
        }
    }

    let x_profile = lattice::project(&mask_tall, Axis::X);
    let col_bands = lattice::find_bands(&x_profile, cfg.min_run, cfg.min_gap, cfg.min_profile);

    if col_bands.is_empty() {
        return TextBands {
            icon: bands,
            text: Vec::new(),
            kept_short: Vec::new(),
        };
    }

    let mut icon = tall_bands;
    let mut text = Vec::new();
    let mut kept_short = Vec::new();

    for band in short_bands {
        let mut total_ink = 0u32;
        let mut outside_ink = 0u32;

        for y in band.start..band.end {
            for x in 0..mask.width {
                if mask.get(x, y) {
                    total_ink += 1;
                    let inside_col = col_bands.iter().any(|cb| x >= cb.start && x < cb.end);
                    if !inside_col {
                        outside_ink += 1;
                    }
                }
            }
        }

        let frac = if total_ink > 0 {
            outside_ink as f32 / total_ink as f32
        } else {
            0.0
        };

        if frac > cfg.outside_frac_max {
            text.push(band);
        } else {
            kept_short.push(band);
            icon.push(band);
        }
    }

    icon.sort_by_key(|b| b.start);

    TextBands {
        icon,
        text,
        kept_short,
    }
}

/// Returns a copy of `mask` with all text rows zeroed out.
pub fn strip_text_rows(mask: &Mask, tb: &TextBands) -> Mask {
    let mut cleaned = mask.clone();
    for band in &tb.text {
        for y in band.start..band.end {
            for x in 0..cleaned.width {
                let idx = (y * cleaned.width + x) as usize;
                if idx < cleaned.bits.len() {
                    cleaned.bits[idx] = false;
                }
            }
        }
    }
    cleaned
}

/// Finds the nearest text band starting after `icon_band.end`, within 1.5x the icon band's height.
pub fn label_band_below(tb: &TextBands, icon_band: &Band) -> Option<Band> {
    let max_dist = 1.5 * icon_band.len() as f32;
    let mut nearest: Option<(Band, u32)> = None;

    for text_band in &tb.text {
        if text_band.start >= icon_band.end {
            let dist = text_band.start - icon_band.end;
            if dist as f32 <= max_dist {
                match nearest {
                    None => nearest = Some((*text_band, dist)),
                    Some((_, best_dist)) => {
                        if dist < best_dist {
                            nearest = Some((*text_band, dist));
                        }
                    }
                }
            }
        }
    }

    nearest.map(|(b, _)| b)
}
#[cfg(test)]
#[path = "textband_tests.rs"]
mod tests;
