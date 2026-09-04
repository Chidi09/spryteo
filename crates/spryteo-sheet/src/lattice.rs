use crate::chroma::Mask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub start: u32,
    pub end: u32,
}

impl Band {
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone)]
pub struct Lattice {
    pub cols: Vec<Band>,
    pub rows: Vec<Band>,
    pub confidence: f32,
}

impl Lattice {
    pub fn cell_count(&self) -> usize {
        self.cols.len() * self.rows.len()
    }

    pub fn cell_rect(&self, col: usize, row: usize) -> Option<(u32, u32, u32, u32)> {
        let c = self.cols.get(col)?;
        let r = self.rows.get(row)?;
        Some((c.start, r.start, c.len(), r.len()))
    }
}

#[derive(Debug, Clone)]
pub struct LatticeConfig {
    pub min_profile: u32,
    pub min_run: u32,
    pub min_gap: u32,
    pub min_confidence: f32,
}

impl Default for LatticeConfig {
    fn default() -> Self {
        LatticeConfig {
            min_profile: 2,
            min_run: 10,
            min_gap: 4,
            min_confidence: 0.75,
        }
    }
}

pub fn project(mask: &Mask, axis: Axis) -> Vec<u32> {
    match axis {
        Axis::X => {
            let w = mask.width as usize;
            let h = mask.height as usize;
            let mut profile = vec![0u32; w];
            for y in 0..h {
                for (x, col) in profile.iter_mut().enumerate() {
                    if mask.get(x as u32, y as u32) {
                        *col += 1;
                    }
                }
            }
            profile
        }
        Axis::Y => {
            let w = mask.width as usize;
            let h = mask.height as usize;
            let mut profile = vec![0u32; h];
            for (y, row) in profile.iter_mut().enumerate() {
                for x in 0..w {
                    if mask.get(x as u32, y as u32) {
                        *row += 1;
                    }
                }
            }
            profile
        }
    }
}

pub fn find_bands(profile: &[u32], min_run: u32, min_gap: u32, min_profile: u32) -> Vec<Band> {
    let mut bands: Vec<Band> = Vec::new();
    let mut band_start: Option<u32> = None;
    let mut last_inked: u32 = 0;
    let mut gap: u32 = 0;

    for (i, &val) in profile.iter().enumerate() {
        let inked = val >= min_profile;
        if inked {
            if band_start.is_none() {
                band_start = Some(i as u32);
            }
            last_inked = i as u32;
            gap = 0;
        } else if let Some(start) = band_start {
            gap += 1;
            if gap >= min_gap {
                let end = last_inked + 1;
                if end - start >= min_run {
                    bands.push(Band { start, end });
                }
                band_start = None;
                gap = 0;
            }
        }
    }

    if let Some(start) = band_start {
        let end = last_inked + 1;
        if end - start >= min_run {
            bands.push(Band { start, end });
        }
    }

    bands
}

fn median_lengths(bands: &[Band]) -> u32 {
    if bands.is_empty() {
        return 0;
    }
    let mut lengths: Vec<u32> = bands.iter().map(|b| b.len()).collect();
    lengths.sort_unstable();
    lengths[lengths.len() / 2]
}

pub fn infer_lattice(mask: &Mask, cfg: &LatticeConfig) -> Option<Lattice> {
    let profile_x = project(mask, Axis::X);
    let profile_y = project(mask, Axis::Y);

    let mut cols = find_bands(&profile_x, cfg.min_run, cfg.min_gap, cfg.min_profile);
    let mut rows = find_bands(&profile_y, cfg.min_run, cfg.min_gap, cfg.min_profile);

    if cols.len() >= 2 {
        let med_x = median_lengths(&cols);
        let refined = med_x / 4;
        let refined_min_run = refined.max(cfg.min_run);
        cols = find_bands(&profile_x, refined_min_run, cfg.min_gap, cfg.min_profile);
    }

    if rows.len() >= 2 {
        let med_y = median_lengths(&rows);
        let refined = med_y / 4;
        let refined_min_run = refined.max(cfg.min_run);
        rows = find_bands(&profile_y, refined_min_run, cfg.min_gap, cfg.min_profile);
    }

    if cols.is_empty() || rows.is_empty() {
        return None;
    }

    let mut lattice = Lattice {
        cols,
        rows,
        confidence: 0.0,
    };
    lattice.confidence = score_lattice(mask, &lattice, cfg);
    if lattice.confidence < cfg.min_confidence {
        return None;
    }
    Some(lattice)
}

/// Confidence that `lat` is the real grid, in 0.0..=1.0.
///
/// Combines its signals as a MINIMUM rather than an average: any one of them
/// being bad means the lattice is wrong, and averaging would let a strong
/// signal paper over a fatal one.
///
/// # The intra-band valley test, and why it is not here
///
/// A third signal was intended: re-project ink inside each band and treat an
/// interior low-density valley as evidence that two icons were run together.
/// Measured against the real sheet it scores the CORRECT lattice at 0.0 --
/// column band 9 has a valley well past the threshold. The reason is
/// structural, not a tuning problem. A column band spans the full page height,
/// so its projection sums seven different icons at once, and any x-offset where
/// all seven happen to be hollow reads as a valley. Narrowing the test to a
/// single cell does not rescue it either: icons here are genuinely
/// multi-component (288 components across 105 icons), so an interior gap is
/// normal rather than suspicious.
///
/// Detecting merged icons properly needs a second opinion from an independent
/// segmentation, which is what the reconcile pass does by cross-checking the
/// lattice against connected-component clustering. Leaving a broken signal in
/// here, tuned until the one sheet we have happens to pass, would be worse than
/// leaving it out.
///
/// # What this deliberately does not measure
///
/// It never looks at band SPACING or pitch. The obvious confidence metric --
/// coefficient of variation over the gaps between bands -- scores a correct
/// lattice on the target sheet as garbage. Its seven row bands start at
/// 112, 224, 327, 514, 614, 787, 886, giving gaps of 112, 103, 187, 100, 173,
/// 99: a CV around 0.3, because the sheet is three panel blocks with wide seams
/// between them. That is a property of the page layout, not evidence of a bad
/// grid, and treating it as a defect would reject the correct answer and fall
/// back to the strictly worse clustering path. Band WIDTHS are uniform on the
/// same sheet even though the SPACING is not, which is why width is measured
/// and spacing is not.
pub fn score_lattice(mask: &Mask, lat: &Lattice, cfg: &LatticeConfig) -> f32 {
    if lat.cols.is_empty() || lat.rows.is_empty() {
        return 0.0;
    }

    // A single band on BOTH axes means no separation was found anywhere: this
    // is one blob, not a grid. It has to be special-cased because the three
    // signals below are all relative to the other bands, and with only one band
    // there is nothing to be relative to -- occupancy is trivially 1.0, there
    // are no widths to compare, and a solid blob has no interior valley. They
    // would unanimously rate a total merge as a perfect lattice.
    if lat.cols.len() == 1 && lat.rows.len() == 1 {
        return 0.0;
    }

    // 1. Occupancy: cells that actually contain an icon. A lattice that
    //    hallucinated an extra row or column shows up as a band of empties.
    let occ = occupancy(mask, lat);

    // 2. Band-width uniformity, per axis. Icons in a grid are drawn at one
    //    size, so the bands they produce should be too. A band holding two
    //    merged icons is roughly twice as wide as its neighbours.
    let uni = width_uniformity(&lat.cols).min(width_uniformity(&lat.rows));

    let _ = cfg;
    occ.min(uni).clamp(0.0, 1.0)
}

/// Fraction of grid cells containing a meaningful amount of ink. The floor is
/// derived from the median cell rather than fixed, so it scales with icon size.
fn occupancy(mask: &Mask, lat: &Lattice) -> f32 {
    let mut counts: Vec<u32> = Vec::with_capacity(lat.cell_count());
    for r in 0..lat.rows.len() {
        for c in 0..lat.cols.len() {
            let Some((x, y, w, h)) = lat.cell_rect(c, r) else {
                continue;
            };
            let mut n = 0u32;
            for yy in y..y.saturating_add(h) {
                for xx in x..x.saturating_add(w) {
                    if mask.get(xx, yy) {
                        n += 1;
                    }
                }
            }
            counts.push(n);
        }
    }
    if counts.is_empty() {
        return 0.0;
    }
    let mut sorted = counts.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    let floor = (median / 10).max(1);
    let filled = counts.iter().filter(|&&n| n >= floor).count();
    filled as f32 / counts.len() as f32
}

/// `1 - IQR/median` over band widths, clamped to 0..=1. Measures widths only,
/// never the spacing between them -- see `score_lattice`.
fn width_uniformity(bands: &[Band]) -> f32 {
    if bands.is_empty() {
        return 0.0;
    }
    if bands.len() < 2 {
        // Nothing to compare against; uniformity cannot be disproved, so do
        // not let it veto.
        return 1.0;
    }
    let mut w: Vec<u32> = bands.iter().map(|b| b.len()).collect();
    w.sort_unstable();
    let n = w.len();
    let median = w[n / 2];
    if median == 0 {
        return 0.0;
    }
    // Below four samples the quartile indices collapse onto each other and an
    // IQR is meaningless -- it would report a merged band as perfectly uniform.
    // Full range is the right statistic there. It is too twitchy for larger n
    // (the real sheet's 15 columns span 42..60 around a median of 47, which
    // range would score 0.62 and wrongly reject), so IQR takes over once there
    // are enough samples for outliers to be distinguishable from spread.
    let spread = if n < 4 {
        w[n - 1].saturating_sub(w[0])
    } else {
        w[(3 * n) / 4].saturating_sub(w[n / 4])
    };
    (1.0 - spread as f32 / median as f32).clamp(0.0, 1.0)
}
#[cfg(test)]
#[path = "lattice_tests.rs"]
mod tests;
