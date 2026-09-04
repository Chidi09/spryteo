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
