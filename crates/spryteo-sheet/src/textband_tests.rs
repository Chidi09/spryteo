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
