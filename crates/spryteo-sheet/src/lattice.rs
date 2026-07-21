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

    Some(Lattice {
        cols,
        rows,
        confidence: 1.0, // TODO(step 8): real scoring lands in a follow-up
    })
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
    fn test_project_counts_ink_per_axis() {
        let mask = make_mask(4, 3, &[(0, 0), (1, 0), (2, 0), (3, 0), (0, 1), (1, 1)]);
        let x_proj = project(&mask, Axis::X);
        assert_eq!(x_proj, vec![2, 2, 1, 1]);
        let y_proj = project(&mask, Axis::Y);
        assert_eq!(y_proj, vec![4, 2, 0]);
    }

    #[test]
    fn test_find_bands_simple() {
        let profile = [0, 0, 3, 3, 3, 0, 0, 0, 0, 0, 0, 4, 4, 4, 0];
        let bands = find_bands(&profile, 2, 4, 2);
        assert_eq!(bands.len(), 2);
        assert_eq!(bands[0], Band { start: 2, end: 5 });
        assert_eq!(bands[1], Band { start: 11, end: 14 });
    }

    #[test]
    fn test_find_bands_single_stray_pixel_does_not_bridge() {
        let profile = [3, 3, 0, 1, 0, 3, 3];
        let bands_high = find_bands(&profile, 1, 3, 2);
        assert_eq!(bands_high.len(), 2, "min_profile=2 must yield two bands");
        let bands_low = find_bands(&profile, 1, 3, 1);
        assert_eq!(
            bands_low.len(),
            1,
            "min_profile=1 must collapse to one band"
        );
    }

    #[test]
    fn test_find_bands_small_interior_gap_does_not_split() {
        let profile = [3, 3, 3, 0, 0, 3, 3, 3];
        let big_gap = find_bands(&profile, 1, 4, 2);
        assert_eq!(big_gap.len(), 1, "min_gap=4 must absorb the 2-wide hole");
        assert_eq!(big_gap[0], Band { start: 0, end: 8 });

        let small_gap = find_bands(&profile, 1, 2, 2);
        assert_eq!(small_gap.len(), 2, "min_gap=2 must split on the hole");
        assert_eq!(small_gap[0], Band { start: 0, end: 3 });
        assert_eq!(small_gap[1], Band { start: 5, end: 8 });
    }

    #[test]
    fn test_find_bands_discards_short_runs() {
        let profile = [0, 5, 5, 5, 0];
        let bands = find_bands(&profile, 10, 1, 2);
        assert!(bands.is_empty());
    }

    #[test]
    fn test_find_bands_flushes_trailing_band() {
        let profile = [0, 0, 5, 5, 5];
        let bands = find_bands(&profile, 2, 2, 2);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0], Band { start: 2, end: 5 });
    }

    #[test]
    fn test_infer_lattice_on_synthetic_grid() {
        let square = 10u32;
        let gutter = 10u32;
        let cols = 4u32;
        let rows = 3u32;
        let width = cols * square + (cols - 1) * gutter;
        let height = rows * square + (rows - 1) * gutter;

        let mut ink = Vec::new();
        for cy in 0..rows {
            for cx in 0..cols {
                let ox = cx * (square + gutter);
                let oy = cy * (square + gutter);
                for dy in 0..square {
                    for dx in 0..square {
                        ink.push((ox + dx, oy + dy));
                    }
                }
            }
        }
        let mask = make_mask(width, height, &ink);
        let cfg = LatticeConfig::default();
        let lattice = infer_lattice(&mask, &cfg).expect("should infer a lattice");

        assert_eq!(lattice.cols.len(), 4, "must find 4 column bands");
        assert_eq!(lattice.rows.len(), 3, "must find 3 row bands");
        assert_eq!(lattice.cell_count(), 12);

        let (x, y, w, h) = lattice.cell_rect(0, 0).expect("cell_rect(0,0)");
        assert_eq!(x, 0);
        assert_eq!(y, 0);
        assert_eq!(w, square);
        assert_eq!(h, square);

        let (x, y, w, h) = lattice.cell_rect(1, 1).expect("cell_rect(1,1)");
        assert_eq!(x, square + gutter);
        assert_eq!(y, square + gutter);
        assert_eq!(w, square);
        assert_eq!(h, square);
    }

    #[test]
    fn test_infer_lattice_returns_none_on_empty_mask() {
        let mask = Mask {
            width: 10,
            height: 10,
            bits: vec![false; 100],
        };
        let cfg = LatticeConfig::default();
        assert!(infer_lattice(&mask, &cfg).is_none());
    }

    #[test]
    fn test_cell_rect_out_of_range_is_none() {
        let cols = vec![Band { start: 0, end: 10 }];
        let rows = vec![Band { start: 0, end: 10 }];
        let lattice = Lattice {
            cols,
            rows,
            confidence: 1.0,
        };
        assert_eq!(lattice.cell_rect(0, 0), Some((0, 0, 10, 10)));
        assert_eq!(lattice.cell_rect(1, 0), None);
        assert_eq!(lattice.cell_rect(0, 1), None);
        assert_eq!(lattice.cell_rect(1, 1), None);
    }

    #[test]
    fn test_zero_size_mask_does_not_panic() {
        let mask = Mask {
            width: 0,
            height: 0,
            bits: vec![],
        };
        let cfg = LatticeConfig::default();
        let x_proj = project(&mask, Axis::X);
        assert!(x_proj.is_empty());
        let y_proj = project(&mask, Axis::Y);
        assert!(y_proj.is_empty());
        let bands = find_bands(&[], 2, 4, 2);
        assert!(bands.is_empty());
        assert!(infer_lattice(&mask, &cfg).is_none());

        let lattice = Lattice {
            cols: vec![],
            rows: vec![],
            confidence: 1.0,
        };
        assert_eq!(lattice.cell_count(), 0);
        assert!(lattice.cell_rect(0, 0).is_none());
    }
}
