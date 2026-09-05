    use super::*;

    // Deterministic pseudo-random number generator (LCG)
    struct SimpleRng {
        state: u32,
    }

    impl SimpleRng {
        fn new(seed: u32) -> Self {
            Self { state: seed }
        }

        fn next_i32(&mut self, min: i32, max: i32) -> i32 {
            self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (self.state / 65536) % 32768;
            let range = max - min + 1;
            min + (val as i32 % range)
        }
    }

    fn compute_variance(image: &RasterImage) -> f64 {
        let n = (image.width * image.height) as f64;
        if n == 0.0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for i in 0..(image.width * image.height) as usize {
            let idx = i * 4;
            let val = (image.pixels[idx] as f64
                + image.pixels[idx + 1] as f64
                + image.pixels[idx + 2] as f64)
                / 3.0;
            sum += val;
        }
        let mean = sum / n;

        let mut sum_sq_diff = 0.0;
        for i in 0..(image.width * image.height) as usize {
            let idx = i * 4;
            let val = (image.pixels[idx] as f64
                + image.pixels[idx + 1] as f64
                + image.pixels[idx + 2] as f64)
                / 3.0;
            sum_sq_diff += (val - mean).powi(2);
        }
        sum_sq_diff / n
    }

    #[test]
    fn test_bilateral_filter_reduces_noise_variance() {
        let width = 32;
        let height = 32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut rng = SimpleRng::new(42);

        // Fill with solid color + noise
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let noise = rng.next_i32(-15, 15);
                let r = (128 + noise).clamp(0, 255) as u8;
                let g = (128 + noise).clamp(0, 255) as u8;
                let b = (128 + noise).clamp(0, 255) as u8;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 255;
            }
        }

        let input_img = RasterImage {
            width,
            height,
            pixels,
        };
        let var_before = compute_variance(&input_img);

        let filtered_img = bilateral_filter(&input_img);
        let var_after = compute_variance(&filtered_img);

        assert!(
            var_after < var_before * 0.5,
            "Variance should be significantly reduced (before: {}, after: {})",
            var_before,
            var_after
        );
    }

    #[test]
    fn test_bilateral_filter_preserves_sharp_edges() {
        let width = 20;
        let height = 20;
        let mut pixels = vec![0u8; (width * height * 4) as usize];

        // Left half red, right half cyan
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                if x < 10 {
                    pixels[idx] = 255; // R
                    pixels[idx + 1] = 0; // G
                    pixels[idx + 2] = 0; // B
                } else {
                    pixels[idx] = 0; // R
                    pixels[idx + 1] = 255; // G
                    pixels[idx + 2] = 255; // B
                }
                pixels[idx + 3] = 255;
            }
        }

        let input_img = RasterImage {
            width,
            height,
            pixels,
        };
        let filtered_img = bilateral_filter(&input_img);

        // Check values a few pixels away from the boundary (x = 10)
        let sample_left_idx = 4 * (10 * width + 7) as usize;
        assert!(
            filtered_img.pixels[sample_left_idx] > 240,
            "Red channel should remain high on the left"
        );
        assert!(
            filtered_img.pixels[sample_left_idx + 1] < 15,
            "Green channel should remain low on the left"
        );
        assert!(
            filtered_img.pixels[sample_left_idx + 2] < 15,
            "Blue channel should remain low on the left"
        );

        let sample_right_idx = 4 * (10 * width + 12) as usize;
        assert!(
            filtered_img.pixels[sample_right_idx] < 15,
            "Red channel should remain low on the right"
        );
        assert!(
            filtered_img.pixels[sample_right_idx + 1] > 240,
            "Green channel should remain high on the right"
        );
        assert!(
            filtered_img.pixels[sample_right_idx + 2] > 240,
            "Blue channel should remain high on the right"
        );
    }

    #[test]
    fn test_preprocess_mode_skip_bilateral() {
        let width = 16;
        let height = 16;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut rng = SimpleRng::new(99);

        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let val = (128 + rng.next_i32(-10, 10)) as u8;
                pixels[idx] = val;
                pixels[idx + 1] = val;
                pixels[idx + 2] = val;
                pixels[idx + 3] = 255;
            }
        }

        let input_img = RasterImage {
            width,
            height,
            pixels,
        };

        // Under Mode::PixelArt, preprocess should skip filtering and return byte-identical image
        let result_pixel_art = preprocess(input_img.clone(), &Mode::PixelArt, false);
        assert_eq!(
            result_pixel_art.pixels, input_img.pixels,
            "PixelArt mode must not modify image"
        );

        // Under other modes (e.g. Mode::Photo), preprocess must modify the image
        let result_photo = preprocess(input_img.clone(), &Mode::Photo, false);
        assert_ne!(
            result_photo.pixels, input_img.pixels,
            "Photo mode should apply bilateral filter"
        );
    }

    #[test]
    fn test_estimate_jpeg_quality_blocky_vs_smooth() {
        // Construct a blocky 16x16 image (jump at boundary x=8, y=8)
        let width = 16;
        let height = 16;
        let mut blocky_pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let color_val = if x < 8 {
                    if y < 8 {
                        50
                    } else {
                        150
                    }
                } else {
                    if y < 8 {
                        100
                    } else {
                        200
                    }
                };
                blocky_pixels[idx] = color_val;
                blocky_pixels[idx + 1] = color_val;
                blocky_pixels[idx + 2] = color_val;
                blocky_pixels[idx + 3] = 255;
            }
        }
        let blocky_img = RasterImage {
            width,
            height,
            pixels: blocky_pixels,
        };

        // Construct a smooth gradient image
        let mut smooth_pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let val = (x * 4 + y * 4) as u8;
                smooth_pixels[idx] = val;
                smooth_pixels[idx + 1] = val;
                smooth_pixels[idx + 2] = val;
                smooth_pixels[idx + 3] = 255;
            }
        }
        let smooth_img = RasterImage {
            width,
            height,
            pixels: smooth_pixels,
        };

        let q_blocky = estimate_jpeg_quality(&blocky_img);
        let q_smooth = estimate_jpeg_quality(&smooth_img);

        assert!(
            q_blocky < q_smooth,
            "Blocky image quality ({}) must be lower than smooth image quality ({})",
            q_blocky,
            q_smooth
        );
        assert!(q_blocky < 90.0, "Blocky image should score below threshold");
        assert!(
            q_smooth >= 90.0,
            "Smooth image should score above threshold"
        );
    }

    #[test]
    fn test_deblock_reduces_blockiness_and_preprocesses_conditionally() {
        let width = 16;
        let height = 16;
        let mut blocky_pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let color_val = if x < 8 {
                    if y < 8 {
                        50
                    } else {
                        150
                    }
                } else {
                    if y < 8 {
                        100
                    } else {
                        200
                    }
                };
                blocky_pixels[idx] = color_val;
                blocky_pixels[idx + 1] = color_val;
                blocky_pixels[idx + 2] = color_val;
                blocky_pixels[idx + 3] = 255;
            }
        }
        let blocky_img = RasterImage {
            width,
            height,
            pixels: blocky_pixels,
        };

        let q_before = estimate_jpeg_quality(&blocky_img);
        let deblocked = deblock(&blocky_img);
        let q_after = estimate_jpeg_quality(&deblocked);

        assert!(
            q_after > q_before,
            "Deblocking must improve (increase) estimated quality (before: {}, after: {})",
            q_before,
            q_after
        );

        // preprocess conditional checks
        // 1. was_jpeg = false -> no-op on deblock. Since we use Mode::PixelArt, bilateral is also skipped.
        // Thus, the result should be identical to input.
        let processed_no_jpeg = preprocess(blocky_img.clone(), &Mode::PixelArt, false);
        assert_eq!(
            processed_no_jpeg.pixels, blocky_img.pixels,
            "Should be no-op when was_jpeg is false and mode is PixelArt"
        );

        // 2. was_jpeg = true -> deblock applied, so image changes.
        let processed_was_jpeg = preprocess(blocky_img.clone(), &Mode::PixelArt, true);
        assert_ne!(
            processed_was_jpeg.pixels, blocky_img.pixels,
            "Should apply deblock when was_jpeg is true and quality < 90"
        );
    }

    #[test]
    fn test_downscale_large_photo_all_cases() {
        // 1. A 2048x1024 photo-mode image downscales to 1024x512 (aspect preserved, exact long-edge target)
        let pixels_2048_1024 = vec![0u8; 2048 * 1024 * 4];
        let img_2048_1024 = RasterImage {
            width: 2048,
            height: 1024,
            pixels: pixels_2048_1024,
        };
        let res = downscale_large_photo(img_2048_1024, &Mode::Photo);
        assert_eq!(res.width, 1024);
        assert_eq!(res.height, 512);

        // 2. A 2048x2048 image downscales to 1024x1024
        let pixels_2048_2048 = vec![0u8; 2048 * 2048 * 4];
        let img_2048_2048 = RasterImage {
            width: 2048,
            height: 2048,
            pixels: pixels_2048_2048,
        };
        let res = downscale_large_photo(img_2048_2048, &Mode::Photo);
        assert_eq!(res.width, 1024);
        assert_eq!(res.height, 1024);

        // 3. A photo-mode image at exactly 1600 (the threshold) is NOT downscaled
        let pixels_1600_1000 = vec![0u8; 1600 * 1000 * 4];
        let img_1600_1000 = RasterImage {
            width: 1600,
            height: 1000,
            pixels: pixels_1600_1000.clone(),
        };
        let res = downscale_large_photo(img_1600_1000, &Mode::Photo);
        assert_eq!(res.width, 1600);
        assert_eq!(res.height, 1000);
        assert_eq!(res.pixels.len(), pixels_1600_1000.len());

        // 4. An icon-mode image at 2048x2048 is NOT downscaled
        let pixels_icon = vec![0u8; 2048 * 2048 * 4];
        let img_icon = RasterImage {
            width: 2048,
            height: 2048,
            pixels: pixels_icon.clone(),
        };
        let res = downscale_large_photo(img_icon, &Mode::Icon);
        assert_eq!(res.width, 2048);
        assert_eq!(res.height, 2048);
        assert_eq!(res.pixels.len(), pixels_icon.len());

        // 5. The function never panics on a 1x1 image
        let pixels_1x1 = vec![0u8; 4];
        let img_1x1 = RasterImage {
            width: 1,
            height: 1,
            pixels: pixels_1x1.clone(),
        };
        let res = downscale_large_photo(img_1x1, &Mode::Photo);
        assert_eq!(res.width, 1);
        assert_eq!(res.height, 1);
        assert_eq!(res.pixels.len(), pixels_1x1.len());
    }

    // ── max_trace_dimension (issue #4) ───────────────────────────────────

    fn solid(width: u32, height: u32) -> RasterImage {
        RasterImage {
            width,
            height,
            pixels: vec![128u8; (width * height * 4) as usize],
        }
    }

    #[test]
    fn max_dimension_leaves_small_images_untouched() {
        let src = solid(64, 32);
        let out = downscale_to_max_dimension(src.clone(), 128, &Mode::Icon);
        assert_eq!((out.width, out.height), (64, 32));
        assert_eq!(out.pixels, src.pixels);
    }

    #[test]
    fn max_dimension_is_a_no_op_at_exactly_the_limit() {
        let out = downscale_to_max_dimension(solid(100, 50), 100, &Mode::Icon);
        assert_eq!((out.width, out.height), (100, 50));
    }

    #[test]
    fn max_dimension_shrinks_the_long_edge_to_the_limit() {
        let out = downscale_to_max_dimension(solid(1024, 512), 32, &Mode::Icon);
        assert_eq!(out.width, 32, "long edge is clamped to max_dim");
        assert_eq!(out.height, 16, "aspect ratio preserved");
        assert_eq!(out.pixels.len(), (32 * 16 * 4) as usize);
    }

    #[test]
    fn max_dimension_clamps_the_taller_edge_for_portrait_images() {
        let out = downscale_to_max_dimension(solid(512, 1024), 32, &Mode::Icon);
        assert_eq!((out.width, out.height), (16, 32));
    }

    #[test]
    fn max_dimension_never_collapses_an_extreme_aspect_ratio_to_zero() {
        // 2000x3 at max 32 would round the short edge to 0 without the max(1).
        let out = downscale_to_max_dimension(solid(2000, 3), 32, &Mode::Icon);
        assert_eq!(out.width, 32);
        assert!(out.height >= 1, "height must stay renderable, got {}", out.height);
    }

    #[test]
    fn max_dimension_zero_is_treated_as_no_downscale() {
        let out = downscale_to_max_dimension(solid(64, 64), 0, &Mode::Icon);
        assert_eq!((out.width, out.height), (64, 64));
    }

    #[test]
    fn max_dimension_composes_with_the_automatic_photo_rule() {
        // Issue #4 asks that the two downscale paths not fight. An explicit
        // limit tighter than the 1600px photo threshold wins outright: after
        // it runs, the automatic rule sees an image already under threshold
        // and does nothing.
        let explicit = downscale_to_max_dimension(solid(2048, 2048), 256, &Mode::Icon);
        assert_eq!(explicit.width, 256);
        let after_auto = downscale_large_photo(explicit, &Mode::Photo);
        assert_eq!(
            after_auto.width, 256,
            "automatic rule must not re-expand or re-shrink"
        );
    }

    /// Two hard-edged colour bands — the shape Lanczos3 rings around.
    fn two_bands(width: u32, height: u32) -> RasterImage {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let v = if x < width / 2 { 0u8 } else { 255u8 };
                pixels[idx] = v;
                pixels[idx + 1] = v;
                pixels[idx + 2] = v;
                pixels[idx + 3] = 255;
            }
        }
        RasterImage {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn flat_art_downscale_does_not_ring_past_the_source_range() {
        // Lanczos3's negative lobes overshoot below 0 and above 255 at a
        // hard edge, manufacturing colours the source never contained. The
        // triangle filter used for flat modes cannot overshoot, so every
        // output pixel must stay within the original two-value range.
        let out = downscale_to_max_dimension(two_bands(256, 256), 64, &Mode::Icon);
        let overshot = out
            .pixels
            .as_chunks::<4>().0.iter()
            .filter(|px| px[0] != 0 && px[0] != 255)
            .count();
        // Some genuine intermediate pixels appear at the edge from area
        // averaging; what must not happen is a large halo of them.
        let total = (out.width * out.height) as usize;
        assert!(
            overshot * 20 < total,
            "flat-art downscale produced {overshot} intermediate pixels of {total}; \
             expected only a thin edge"
        );
    }

    #[test]
    fn pixel_art_downscale_introduces_no_new_colors() {
        let out = downscale_to_max_dimension(two_bands(64, 64), 16, &Mode::PixelArt);
        assert!(
            out.pixels
                .as_chunks::<4>().0.iter()
                .all(|px| px[0] == 0 || px[0] == 255),
            "nearest-neighbour must never blend pixel-art colours"
        );
    }

    #[test]
    fn downscale_does_not_bleed_color_from_transparent_pixels() {
        // Regression guard: encoders routinely leave arbitrary RGB under
        // alpha==0. Resampling straight alpha averages that invisible
        // colour into visible edge pixels, producing a halo that survives
        // quantization as its own layer. Here the transparent margin holds
        // saturated green and the visible disc is pure red; no output pixel
        // with meaningful coverage may pick up green.
        let size = 64u32;
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        let c = size as f32 / 2.0;
        for y in 0..size {
            for x in 0..size {
                let idx = 4 * (y * size + x) as usize;
                let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
                if d < c * 0.6 {
                    pixels[idx] = 255; // opaque red disc
                    pixels[idx + 3] = 255;
                } else {
                    pixels[idx + 1] = 255; // invisible green margin
                    pixels[idx + 3] = 0;
                }
            }
        }
        let src = RasterImage {
            width: size,
            height: size,
            pixels,
        };

        let out = downscale_to_max_dimension(src, 16, &Mode::Icon);
        for px in out.pixels.as_chunks::<4>().0 {
            if px[3] > 32 {
                assert!(
                    px[1] < 64,
                    "visible pixel picked up green from the transparent margin: {px:?}"
                );
            }
        }
    }

    #[test]
    fn downscale_preserves_color_of_a_fully_opaque_image() {
        // The premultiply path must be a no-op when nothing is transparent.
        let src = two_bands(64, 64);
        let out = downscale_to_max_dimension(src, 16, &Mode::PixelArt);
        assert!(out
            .pixels
            .as_chunks::<4>().0.iter()
            .all(|px| px[3] == 255 && (px[0] == 0 || px[0] == 255)));
    }

    #[test]
    fn downscale_factor_matches_the_applied_resize() {
        assert_eq!(downscale_factor(1024, 512, 32), 32.0 / 1024.0);
        assert_eq!(downscale_factor(512, 1024, 32), 32.0 / 1024.0);
        assert_eq!(downscale_factor(64, 32, 128), 1.0, "no downscale needed");
        assert_eq!(downscale_factor(100, 50, 100), 1.0, "exactly at the limit");
        assert_eq!(downscale_factor(64, 64, 0), 1.0, "zero means no downscale");
    }
