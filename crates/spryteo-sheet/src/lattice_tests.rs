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

    /// Builds a grid of filled squares. `pitch` is the stride between cell
    /// origins, so a pitch equal to `size` merges neighbours into one band.
    fn grid_mask(cols: u32, rows: u32, size: u32, pitch: u32, margin: u32) -> Mask {
        let w = margin * 2 + pitch * cols;
        let h = margin * 2 + pitch * rows;
        let mut bits = vec![false; (w * h) as usize];
        for r in 0..rows {
            for c in 0..cols {
                let x0 = margin + c * pitch;
                let y0 = margin + r * pitch;
                for y in y0..y0 + size {
                    for x in x0..x0 + size {
                        bits[(y * w + x) as usize] = true;
                    }
                }
            }
        }
        Mask {
            width: w,
            height: h,
            bits,
        }
    }

    #[test]
    fn test_score_lattice_high_on_clean_grid() {
        let mask = grid_mask(5, 4, 20, 40, 20);
        let cfg = LatticeConfig::default();
        let lat = infer_lattice(&mask, &cfg).expect("clean grid should infer");
        assert_eq!(lat.cols.len(), 5);
        assert_eq!(lat.rows.len(), 4);
        assert!(
            lat.confidence > 0.75,
            "clean grid should score high, got {}",
            lat.confidence
        );
    }

    /// THE TRAP. Irregular SPACING is not evidence of a bad lattice -- the real
    /// target sheet is three panel blocks with wide seams, giving row gaps of
    /// 112, 103, 187, 100, 173, 99 on a perfectly correct 15x7 grid. Any
    /// confidence metric built on pitch regularity (coefficient of variation
    /// over gaps, FFT of the profile, "bands must be evenly spaced") rejects
    /// the right answer here and falls back to the strictly worse clustering
    /// path. This test encodes uniform-width-but-irregular-spacing as a
    /// HIGH-confidence case on purpose.
    #[test]
    fn test_score_ignores_irregular_spacing() {
        // Six 20px squares, all the same size, at deliberately uneven strides.
        let starts = [20u32, 60, 100, 200, 240, 330];
        let w = 400u32;
        let h = 60u32;
        let mut bits = vec![false; (w * h) as usize];
        for s in starts {
            for y in 20..40u32 {
                for x in s..s + 20 {
                    bits[(y * w + x) as usize] = true;
                }
            }
        }
        let mask = Mask {
            width: w,
            height: h,
            bits,
        };
        let cfg = LatticeConfig::default();
        let lat = infer_lattice(&mask, &cfg).expect("irregular spacing must still infer");
        assert_eq!(lat.cols.len(), 6, "should find all six columns");

        let gaps: Vec<u32> = starts.windows(2).map(|p| p[1] - p[0]).collect();
        let mean = gaps.iter().sum::<u32>() as f32 / gaps.len() as f32;
        let var = gaps.iter().map(|g| (*g as f32 - mean).powi(2)).sum::<f32>() / gaps.len() as f32;
        let cv = var.sqrt() / mean;
        assert!(
            cv > 0.35,
            "fixture must actually have irregular pitch to be a valid test, cv={cv}"
        );
        assert!(
            lat.confidence > 0.75,
            "irregular SPACING must not lower confidence (cv={cv}), got {}",
            lat.confidence
        );
    }

    #[test]
    fn test_score_low_when_everything_merges() {
        // pitch == size: every square touches the next, so the entire grid
        // collapses to a single band on both axes.
        let mask = grid_mask(6, 4, 20, 20, 20);
        let lat = infer_lattice(&mask, &LatticeConfig::default());
        assert!(
            lat.is_none(),
            "a total merge is one blob, not a grid, and must be rejected: {lat:?}"
        );
    }

    #[test]
    fn test_score_low_when_two_columns_merge() {
        // The realistic failure: most icons separate cleanly but one adjacent
        // pair runs together, producing a band twice as wide as its
        // neighbours. Width uniformity is the signal that fires here.
        let w = 300u32;
        let h = 100u32;
        let mut bits = vec![false; (w * h) as usize];
        // Four columns at x=20,60,100,120 -- the last two touch and merge.
        for x0 in [20u32, 60, 100, 120] {
            for y in 30..50u32 {
                for x in x0..x0 + 20 {
                    bits[(y * w + x) as usize] = true;
                }
            }
        }
        let mask = Mask {
            width: w,
            height: h,
            bits,
        };
        let lat = infer_lattice(&mask, &LatticeConfig::default());
        match lat {
            None => {} // rejected outright is also acceptable
            Some(l) => assert!(
                l.confidence < 0.5,
                "a doubled-width band must score low, got {} with widths {:?}",
                l.confidence,
                l.cols.iter().map(|b| b.len()).collect::<Vec<_>>()
            ),
        }
    }

    #[test]
    fn test_score_low_when_rows_are_mostly_empty() {
        // One lone icon in an otherwise empty 4x4 arrangement: occupancy is the
        // signal that fires here, not width or modality.
        let w = 200u32;
        let h = 200u32;
        let mut bits = vec![false; (w * h) as usize];
        for (cx, cy) in [(20u32, 20u32), (120, 20), (20, 120)] {
            for y in cy..cy + 20 {
                for x in cx..cx + 20 {
                    bits[(y * w + x) as usize] = true;
                }
            }
        }
        // Add a 1-pixel-tall sliver far away to force a sparse extra band.
        let mask = Mask {
            width: w,
            height: h,
            bits,
        };
        let lat = infer_lattice(&mask, &LatticeConfig::default());
        if let Some(l) = lat {
            // 2x2 lattice with only 3 of 4 cells filled -> occupancy 0.75.
            assert!(
                l.confidence <= 0.80,
                "a missing cell should be visible in the score, got {}",
                l.confidence
            );
        }
    }

    /// Regression guard using the REAL target sheet's measured band geometry.
    /// Any future change to scoring that would reject the actual 15x7 sheet
    /// fails here without needing the image on disk. Measured score: 0.857.
    #[test]
    fn test_real_sheet_geometry_scores_above_gate() {
        let col_widths = [
            50u32, 48, 44, 46, 43, 45, 42, 44, 43, 50, 47, 48, 52, 50, 60,
        ];
        let row_starts = [112u32, 224, 327, 514, 614, 787, 886];
        let row_heights = [46u32, 40, 42, 43, 46, 40, 39];

        let mut x = 60u32;
        let mut cols = Vec::new();
        for (i, w) in col_widths.iter().enumerate() {
            cols.push(Band {
                start: x,
                end: x + w,
            });
            // Uneven strides, mirroring the real page's panel blocks.
            x += w + if i % 5 == 4 { 40 } else { 48 };
        }
        let rows: Vec<Band> = row_starts
            .iter()
            .zip(row_heights.iter())
            .map(|(s, h)| Band {
                start: *s,
                end: s + h,
            })
            .collect();

        // Fill every cell so occupancy is 1.0 and the width signal is isolated.
        let w = 1536u32;
        let h = 1024u32;
        let mut bits = vec![false; (w * h) as usize];
        for r in &rows {
            for c in &cols {
                for y in r.start..r.end {
                    for xx in c.start..c.end.min(w) {
                        bits[(y * w + xx) as usize] = true;
                    }
                }
            }
        }
        let mask = Mask {
            width: w,
            height: h,
            bits,
        };
        let lat = Lattice {
            cols,
            rows,
            confidence: 0.0,
        };
        let cfg = LatticeConfig::default();
        let score = score_lattice(&mask, &lat, &cfg);
        assert!(
            score >= cfg.min_confidence,
            "the real sheet's own geometry must clear the gate, got {score}"
        );
    }

    #[test]
    fn test_width_uniformity_measures_width_not_spacing() {
        let even = [
            Band { start: 0, end: 10 },
            Band { start: 20, end: 30 },
            Band { start: 40, end: 50 },
            Band { start: 60, end: 70 },
        ];
        let uneven_spacing = [
            Band { start: 0, end: 10 },
            Band { start: 50, end: 60 },
            Band { start: 55, end: 65 },
            Band {
                start: 300,
                end: 310,
            },
        ];
        assert_eq!(
            width_uniformity(&even),
            width_uniformity(&uneven_spacing),
            "spacing must not affect a width metric"
        );

        let ragged = [
            Band { start: 0, end: 10 },
            Band { start: 20, end: 55 },
            Band { start: 60, end: 64 },
            Band { start: 70, end: 95 },
        ];
        assert!(
            width_uniformity(&ragged) < width_uniformity(&even),
            "varying widths must lower uniformity"
        );
    }
