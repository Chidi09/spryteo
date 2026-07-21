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
mod tests {
    use super::*;

    fn make_mask(width: u32, height: u32, ink: &[(u32, u32)]) -> Mask {
        let mut bits = vec![false; (width * height) as usize];
        for &(x, y) in ink {
            let idx = (y * width + x) as usize;
            bits[idx] = true;
        }
        Mask {
            width,
            height,
            bits,
        }
    }

    #[test]
    fn test_short_bands_offset_from_columns_classified_as_text() {
        // Two tall icon rows (aligned into columns) + short bands offset from column grid
        let width = 100u32;
        let height = 120u32;
        let mut ink = Vec::new();

        // Icon columns: x=10..20, x=40..50, x=70..80
        // Tall row 1: y=10..50 (height 40)
        // Tall row 2: y=60..100 (height 40)
        for y in 10..50 {
            for x in (10..20).chain(40..50).chain(70..80) {
                ink.push((x, y));
            }
        }
        for y in 60..100 {
            for x in (10..20).chain(40..50).chain(70..80) {
                ink.push((x, y));
            }
        }

        // Short text row: y=105..117 (height 12), offset ink at x=25..35
        for y in 105..117 {
            for x in 25..35 {
                ink.push((x, y));
            }
        }

        let mask = make_mask(width, height, &ink);
        let cfg = TextBandConfig::default();
        let tb = classify_rows(&mask, &cfg);

        assert_eq!(tb.text.len(), 1);
        assert_eq!(
            tb.text[0],
            Band {
                start: 105,
                end: 117
            }
        );
        assert_eq!(tb.icon.len(), 2);
        assert_eq!(tb.kept_short.len(), 0);

        let cleaned = strip_text_rows(&mask, &tb);
        for y in 0..height {
            for x in 0..width {
                if (105..117).contains(&y) {
                    assert!(!cleaned.get(x, y), "text row must be zeroed");
                } else {
                    assert_eq!(
                        cleaned.get(x, y),
                        mask.get(x, y),
                        "non-text rows must be identical"
                    );
                }
            }
        }
    }

    #[test]
    fn test_short_band_column_aligned_retained_in_icon() {
        // A short band whose ink is perfectly column-aligned (synthetic dots row)
        let width = 100u32;
        let height = 120u32;
        let mut ink = Vec::new();

        for y in 10..50 {
            for x in (10..20).chain(40..50).chain(70..80) {
                ink.push((x, y));
            }
        }
        for y in 60..100 {
            for x in (10..20).chain(40..50).chain(70..80) {
                ink.push((x, y));
            }
        }

        // Short dots row: y=105..117 (height 12), aligned ink at x=12..18 and x=42..48
        for y in 105..117 {
            for x in (12..18).chain(42..48) {
                ink.push((x, y));
            }
        }

        let mask = make_mask(width, height, &ink);
        let cfg = TextBandConfig::default();
        let tb = classify_rows(&mask, &cfg);

        assert!(tb.text.is_empty());
        assert_eq!(tb.kept_short.len(), 1);
        assert_eq!(
            tb.kept_short[0],
            Band {
                start: 105,
                end: 117
            }
        );
        assert_eq!(tb.icon.len(), 3);
        assert_eq!(
            tb.icon[2],
            Band {
                start: 105,
                end: 117
            }
        );

        let cleaned = strip_text_rows(&mask, &tb);
        assert_eq!(cleaned.bits, mask.bits);
    }

    #[test]
    fn test_all_bands_similar_height_no_text() {
        let width = 100u32;
        let height = 160u32;
        let mut ink = Vec::new();

        for y in (10..50).chain(60..100).chain(110..150) {
            for x in 10..20 {
                ink.push((x, y));
            }
        }

        let mask = make_mask(width, height, &ink);
        let cfg = TextBandConfig::default();
        let tb = classify_rows(&mask, &cfg);

        assert!(tb.text.is_empty());
        assert!(tb.kept_short.is_empty());
        assert_eq!(tb.icon.len(), 3);

        let cleaned = strip_text_rows(&mask, &tb);
        assert_eq!(cleaned.bits, mask.bits);
    }

    #[test]
    fn test_height_ratio_below_threshold_no_text() {
        let width = 100u32;
        let height = 120u32;
        let mut ink = Vec::new();

        // Band heights: 20 (y=10..30), 35 (y=40..75), 35 (y=80..115)
        // Ratio = 35 / 20 = 1.75 < 1.8
        for y in 10..30 {
            for x in 10..20 {
                ink.push((x, y));
            }
        }
        for y in 40..75 {
            for x in 10..20 {
                ink.push((x, y));
            }
        }
        for y in 80..115 {
            for x in 10..20 {
                ink.push((x, y));
            }
        }

        let mask = make_mask(width, height, &ink);
        let cfg = TextBandConfig::default();
        let tb = classify_rows(&mask, &cfg);

        assert!(tb.text.is_empty());
        assert_eq!(tb.icon.len(), 3);
    }

    #[test]
    fn test_short_header_above_tall_band_classified_as_text() {
        // A short band ABOVE the first tall band with misaligned ink (a header)
        let width = 100u32;
        let height = 120u32;
        let mut ink = Vec::new();

        // Header: y=0..12 (height 12), offset ink at x=25..35
        for y in 0..12 {
            for x in 25..35 {
                ink.push((x, y));
            }
        }

        // Tall icon rows: y=20..60 (height 40), y=70..110 (height 40)
        for y in (20..60).chain(70..110) {
            for x in (10..20).chain(40..50) {
                ink.push((x, y));
            }
        }

        let mask = make_mask(width, height, &ink);
        let cfg = TextBandConfig::default();
        let tb = classify_rows(&mask, &cfg);

        assert_eq!(tb.text.len(), 1);
        assert_eq!(tb.text[0], Band { start: 0, end: 12 });
        assert_eq!(tb.icon.len(), 2);
    }

    #[test]
    fn test_label_band_below() {
        let icon1 = Band { start: 10, end: 50 }; // height 40 -> 1.5x = 60
        let text1 = Band { start: 55, end: 67 }; // dist 5 <= 60
        let text2 = Band {
            start: 120,
            end: 132,
        }; // dist 70 > 60

        let tb = TextBands {
            icon: vec![icon1],
            text: vec![text1, text2],
            kept_short: vec![],
        };

        let found = label_band_below(&tb, &icon1);
        assert_eq!(found, Some(text1));

        let icon2 = Band {
            start: 200,
            end: 240,
        };
        let not_found = label_band_below(&tb, &icon2);
        assert_eq!(not_found, None);
    }
}
