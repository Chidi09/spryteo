    use super::*;
    use crate::lattice::Band;

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

    fn make_lattice(cols: &[(u32, u32)], rows: &[(u32, u32)]) -> Lattice {
        Lattice {
            cols: cols
                .iter()
                .map(|&(s, e)| Band { start: s, end: e })
                .collect(),
            rows: rows
                .iter()
                .map(|&(s, e)| Band { start: s, end: e })
                .collect(),
            confidence: 1.0,
        }
    }

    #[test]
    fn test_ink_bbox_in_finds_tight_box() {
        // Mask 100x100, ink at (20,30) to (29,39)
        let mut ink = Vec::new();
        for y in 30..40 {
            for x in 20..30 {
                ink.push((x, y));
            }
        }
        let mask = make_mask(100, 100, &ink);
        let search_rect = Bbox {
            x1: 10,
            y1: 10,
            x2: 50,
            y2: 50,
        };

        let tight = ink_bbox_in(&mask, &search_rect).expect("should find tight bbox");
        assert_eq!(
            tight,
            Bbox {
                x1: 20,
                y1: 30,
                x2: 30,
                y2: 40,
            }
        );
    }

    #[test]
    fn test_ink_bbox_in_empty_rect_is_none() {
        let mask = make_mask(100, 100, &[(20, 20)]);
        let search_rect = Bbox {
            x1: 50,
            y1: 50,
            x2: 60,
            y2: 60,
        };
        assert!(ink_bbox_in(&mask, &search_rect).is_none());
    }

    #[test]
    fn test_ink_bbox_ignores_ink_outside_rect() {
        let mask = make_mask(100, 100, &[(5, 5), (20, 20), (80, 80)]);
        let search_rect = Bbox {
            x1: 15,
            y1: 15,
            x2: 25,
            y2: 25,
        };
        let tight = ink_bbox_in(&mask, &search_rect).expect("should find tight bbox");
        assert_eq!(
            tight,
            Bbox {
                x1: 20,
                y1: 20,
                x2: 21,
                y2: 21,
            }
        );
    }

    #[test]
    fn test_extract_cells_crop_is_tight_not_cell_rect() {
        // 60px wide cell rect [0, 60), holding 10px wide icon [25, 35) (100 ink pixels)
        let mut ink = Vec::new();
        for y in 25..35 {
            for x in 25..35 {
                ink.push((x, y));
            }
        }
        let mask = make_mask(100, 100, &ink);
        let lat = make_lattice(&[(0, 60)], &[(0, 60)]);
        let cfg = CellConfig {
            pad: 2,
            min_ink: 12,
        };

        let cells = extract_cells(&mask, &lat, &cfg);
        assert_eq!(cells.len(), 1);
        let cell = &cells[0];

        assert_eq!(
            cell.cell_rect,
            Bbox {
                x1: 0,
                y1: 0,
                x2: 60,
                y2: 60
            }
        );
        // Tight is [25, 35) x [25, 35). Pad is 2 -> crop is [23, 37) x [23, 37).
        assert_eq!(cell.crop.width(), 14);
        assert_eq!(cell.crop.height(), 14);
        assert!(cell.crop.width() < cell.cell_rect.width());
    }

    #[test]
    fn test_extract_cells_skips_empty_cells() {
        // 2x2 lattice, only (0,0), (1,0), (0,1) have ink
        let mut ink = Vec::new();
        for (cx, cy) in [(5, 5), (25, 5), (5, 25)] {
            for dy in 0..4 {
                for dx in 0..4 {
                    ink.push((cx + dx, cy + dy)); // 16 pixels each
                }
            }
        }
        let mask = make_mask(40, 40, &ink);
        let lat = make_lattice(&[(0, 20), (20, 40)], &[(0, 20), (20, 40)]);
        let cfg = CellConfig {
            pad: 2,
            min_ink: 12,
        };

        let cells = extract_cells(&mask, &lat, &cfg);
        assert_eq!(cells.len(), 3);
        assert!(!cells.iter().any(|c| c.col == 1 && c.row == 1));
    }

    #[test]
    fn test_extract_cells_padding_clips_at_image_edge() {
        // Icon flush against x=0, y=0. Tight is [0, 10) x [0, 10).
        let mut ink = Vec::new();
        for y in 0..10 {
            for x in 0..10 {
                ink.push((x, y));
            }
        }
        let mask = make_mask(50, 50, &ink);
        let lat = make_lattice(&[(0, 20)], &[(0, 20)]);
        let cfg = CellConfig {
            pad: 2,
            min_ink: 12,
        };

        let cells = extract_cells(&mask, &lat, &cfg);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].crop.x1, 0);
        assert_eq!(cells[0].crop.y1, 0);
        assert_eq!(cells[0].crop.x2, 12);
        assert_eq!(cells[0].crop.y2, 12);
    }

    #[test]
    fn test_extract_cells_row_major_order() {
        // 2x2 lattice with ink in all 4 cells
        let mut ink = Vec::new();
        for (cx, cy) in [(5, 5), (25, 5), (5, 25), (25, 25)] {
            for dy in 0..4 {
                for dx in 0..4 {
                    ink.push((cx + dx, cy + dy));
                }
            }
        }
        let mask = make_mask(40, 40, &ink);
        let lat = make_lattice(&[(0, 20), (20, 40)], &[(0, 20), (20, 40)]);
        let cfg = CellConfig {
            pad: 2,
            min_ink: 12,
        };

        let cells = extract_cells(&mask, &lat, &cfg);
        let coords: Vec<(usize, usize)> = cells.iter().map(|c| (c.col, c.row)).collect();
        assert_eq!(coords, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
    }
