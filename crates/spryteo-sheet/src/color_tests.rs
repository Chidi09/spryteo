    use super::*;

    fn make_test_image_and_mask(
        width: u32,
        height: u32,
        mut pixel_fn: impl FnMut(u32, u32) -> (u8, u8, u8, u8, bool),
    ) -> (RasterImage, Mask, Bbox) {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        let mut bits = Vec::with_capacity((width * height) as usize);

        for y in 0..height {
            for x in 0..width {
                let (r, g, b, a, mask_bit) = pixel_fn(x, y);
                pixels.push(r);
                pixels.push(g);
                pixels.push(b);
                pixels.push(a);
                bits.push(mask_bit);
            }
        }

        let image = RasterImage {
            width,
            height,
            pixels,
        };
        let mask = Mask {
            width,
            height,
            bits,
        };
        let rect = Bbox {
            x1: 0,
            y1: 0,
            x2: width,
            y2: height,
        };

        (image, mask, rect)
    }

    #[test]
    fn test_fit_recovers_horizontal_ramp() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |x, _| {
            let r = (x as f64 / 39.0 * 255.0).round() as u8;
            (r, 0, 255, 255, true)
        });
        let cfg = ColorConfig::default();
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg).expect("fit should succeed");

        let dir_x = fit.x2 - fit.x1;
        let dir_y = fit.y2 - fit.y1;
        let angle_deg = dir_y.atan2(dir_x).to_degrees();
        assert!(
            angle_deg.abs() < 5.0,
            "horizontal ramp angle should be within 5 deg of 0, got {angle_deg}"
        );

        // Check start and end colours are close to rgb(0,0,255) and rgb(255,0,255)
        let t_start_frac: f64 = 0.05;
        let t_end_frac: f64 = 0.95;
        let expected_start_r = (t_start_frac * 255.0).round() as i16;
        let expected_end_r = (t_end_frac * 255.0).round() as i16;

        assert!(
            (fit.start.r as i16 - expected_start_r).abs() <= 3,
            "start.r expected ~{expected_start_r}, got {}",
            fit.start.r
        );
        assert_eq!(fit.start.g, 0);
        assert_eq!(fit.start.b, 255);

        assert!(
            (fit.end.r as i16 - expected_end_r).abs() <= 3,
            "end.r expected ~{expected_end_r}, got {}",
            fit.end.r
        );
        assert_eq!(fit.end.g, 0);
        assert_eq!(fit.end.b, 255);
    }

    #[test]
    fn test_fit_recovers_diagonal_ramp() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |x, y| {
            let t = (x as f64 + y as f64) / 78.0;
            let r = (t * 255.0).round() as u8;
            (r, 0, 255, 255, true)
        });
        let cfg = ColorConfig::default();
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg).expect("fit should succeed");

        let dir_x = fit.x2 - fit.x1;
        let dir_y = fit.y2 - fit.y1;
        let angle_deg = dir_y.atan2(dir_x).to_degrees();
        assert!(
            (angle_deg - 45.0).abs() < 5.0,
            "diagonal ramp angle should be within 5 deg of 45, got {angle_deg}"
        );
    }

    #[test]
    fn test_fit_recovers_opposing_channel_ramp() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |x, _| {
            let t = x as f64 / 39.0;
            let r = (20.0 + t * 200.0).round() as u8;
            let b = (220.0 - t * 200.0).round() as u8;
            (r, 0, b, 255, true)
        });
        let cfg = ColorConfig::default();
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg).expect("fit should succeed");

        let dir_x = fit.x2 - fit.x1;
        let dir_y = fit.y2 - fit.y1;
        let angle_deg = dir_y.atan2(dir_x).to_degrees();

        // Print recovered angle as required
        println!("Opposing channel ramp recovered angle: {angle_deg} degrees");

        assert!(
            angle_deg.abs() < 5.0,
            "opposing channel ramp angle should be within 5 deg of 0, got {angle_deg}"
        );
    }

    #[test]
    fn test_fit_direction_sign_is_stable() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |x, _| {
            let r = (x as f64 / 39.0 * 255.0).round() as u8;
            (r, 0, 255, 255, true)
        });
        let cfg = ColorConfig::default();

        let fit1 = fit_linear_gradient(&image, &mask, &rect, &cfg).unwrap();
        let fit2 = fit_linear_gradient(&image, &mask, &rect, &cfg).unwrap();

        assert_eq!(fit1, fit2, "fit output must be byte-identical");
        assert!(fit1.x1 <= fit2.x2, "x1 <= x2 must hold for +x ramp");
    }

    #[test]
    fn test_fit_rejects_flat_color() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |_, _| (200, 0, 255, 255, true));
        let cfg = ColorConfig::default();
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg);
        assert!(fit.is_none(), "flat color should return None");
    }

    #[test]
    fn test_fit_rejects_too_few_samples() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |x, y| {
            let mask_bit = x < 5 && y == 0;
            (255, 0, 255, 255, mask_bit)
        });
        let cfg = ColorConfig::default(); // min_samples = 30
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg);
        assert!(fit.is_none(), "fewer than min_samples should return None");
    }

    #[test]
    fn test_fit_rejects_high_residual() {
        let mut seed = 123456789u64;
        let mut rand_u8 = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 32) as u8
        };

        let (image, mask, rect) = make_test_image_and_mask(40, 40, |_, _| {
            let r = rand_u8();
            let g = 0;
            let b = 255; // ensures high saturation
            (r, g, b, 255, true)
        });
        let cfg = ColorConfig::default();
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg);
        assert!(
            fit.is_none(),
            "random colors with high residual should return None"
        );
    }

    #[test]
    fn test_fit_ignores_low_saturation_pixels() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |x, y| {
            if (5..35).contains(&x) && (5..35).contains(&y) {
                let r = ((x - 5) as f64 / 29.0 * 255.0).round() as u8;
                (r, 0, 255, 255, true)
            } else {
                // Near white border, low saturation
                (250, 250, 250, 255, true)
            }
        });
        let cfg = ColorConfig::default(); // core_sat_min = 150
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg).expect("fit should succeed");

        // Compute fit on ramp only (inner 5..35)
        let inner_rect = Bbox {
            x1: 5,
            y1: 5,
            x2: 35,
            y2: 35,
        };
        let inner_fit = fit_linear_gradient(&image, &mask, &inner_rect, &cfg)
            .expect("inner fit should succeed");

        assert_eq!(
            fit.start, inner_fit.start,
            "border must be excluded, start color should match inner fit"
        );
        assert_eq!(
            fit.end, inner_fit.end,
            "border must be excluded, end color should match inner fit"
        );
    }

    #[test]
    fn test_fit_percentile_extent_resists_outlier() {
        let (image, mask, rect) = make_test_image_and_mask(110, 40, |x, y| {
            if x < 40 {
                let r = (x as f64 / 39.0 * 255.0).round() as u8;
                (r, 0, 255, 255, true)
            } else if x == 100 && y == 20 {
                // Stray core pixel far away
                (255, 0, 255, 255, true)
            } else {
                (0, 0, 0, 0, false)
            }
        });
        let cfg = ColorConfig::default();
        let fit = fit_linear_gradient(&image, &mask, &rect, &cfg).expect("fit should succeed");

        assert!(
            fit.x2 < 50.0,
            "x2 should be near main blob edge (< 50), not at outlier (100), got {}",
            fit.x2
        );
    }

    #[test]
    fn test_dominant_color_averages_core_pixels() {
        let (image, mask, rect) = make_test_image_and_mask(40, 40, |_, _| (200, 50, 50, 255, true));
        let dom = dominant_color(&image, &mask, &rect, 150).expect("should find dominant color");
        assert_eq!(
            dom,
            Rgb {
                r: 200,
                g: 50,
                b: 50
            }
        );
    }

    #[test]
    fn test_dominant_color_none_when_no_core_pixels() {
        let (image, mask, rect) =
            make_test_image_and_mask(40, 40, |_, _| (200, 200, 200, 255, true));
        let dom = dominant_color(&image, &mask, &rect, 150);
        assert!(dom.is_none(), "low sat pixels should yield None");
    }

    #[test]
    fn test_zero_size_and_empty_rect_do_not_panic() {
        let empty_img = RasterImage {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let empty_mask = Mask {
            width: 0,
            height: 0,
            bits: vec![],
        };
        let rect = Bbox {
            x1: 0,
            y1: 0,
            x2: 0,
            y2: 0,
        };
        let cfg = ColorConfig::default();

        assert!(fit_linear_gradient(&empty_img, &empty_mask, &rect, &cfg).is_none());
        assert!(dominant_color(&empty_img, &empty_mask, &rect, 150).is_none());

        let rect_out_of_bounds = Bbox {
            x1: 10,
            y1: 10,
            x2: 10,
            y2: 10,
        };
        assert!(fit_linear_gradient(&empty_img, &empty_mask, &rect_out_of_bounds, &cfg).is_none());
        assert!(dominant_color(&empty_img, &empty_mask, &rect_out_of_bounds, 150).is_none());
    }
