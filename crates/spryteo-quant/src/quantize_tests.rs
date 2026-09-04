    use super::*;
    use spryteo_core::{ClassifiedInput, Mode, RasterImage};

    fn make_image(width: u32, height: u32, pixels: Vec<u8>) -> RasterImage {
        RasterImage {
            width,
            height,
            pixels,
        }
    }

    fn rgb_pixel(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        [r, g, b, a]
    }

    fn classified_input(img: RasterImage, mode: Mode) -> ClassifiedInput {
        ClassifiedInput {
            image: img,
            mode,
            background_color: None,
        }
    }

    // ── Auto → 2 layers for a 2-colour image ─────────────────────────────

    #[test]
    fn auto_quantize_two_colour() {
        // 4×4 checkerboard of red and white
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 0 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);

        assert_eq!(
            result.layers.len(),
            2,
            "expected 2 layers for a 2-colour image, got {}",
            result.layers.len()
        );
    }

    // ── N(1) collapses to 1 layer ─────────────────────────────────────────

    #[test]
    fn n1_collapses_to_one_layer() {
        // Multi-colour image → forced to 1 colour
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let r = (x * 64) as u8;
                let g = (y * 64) as u8;
                let b = 128u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::N(1), &Layering::Cutout, 42);

        assert_eq!(
            result.layers.len(),
            1,
            "expected 1 layer with N(1), got {}",
            result.layers.len()
        );
    }

    // ── Determinism ───────────────────────────────────────────────────────

    #[test]
    fn quantize_is_deterministic() {
        // Multi-colour image large enough to trigger k-means
        let w = 8u32;
        let h = 8u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 32) as u8).wrapping_add(10);
                let g = ((y * 32) as u8).wrapping_add(20);
                let b = ((x + y) * 16) as u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Icon);
        let seed = 12345u64;

        let r1 = quantize(&input, &ColorSpec::N(4), &Layering::Cutout, seed);
        let r2 = quantize(&input, &ColorSpec::N(4), &Layering::Cutout, seed);

        let json1 = serde_json::to_vec(&r1).unwrap();
        let json2 = serde_json::to_vec(&r2).unwrap();

        assert_eq!(
            json1, json2,
            "quantize produced different results for the same seed"
        );
    }

    // ── Palette mode ──────────────────────────────────────────────────────

    #[test]
    fn palette_assigns_to_nearest() {
        // 2-colour image (red, white) → palette of [blue, green]
        // All pixels should assign to nearest between blue and green
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 0 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let palette = vec![Rgb { r: 0, g: 0, b: 255 }, Rgb { r: 0, g: 255, b: 0 }];
        let result = quantize(&input, &ColorSpec::Palette(palette), &Layering::Cutout, 42);

        assert_eq!(result.layers.len(), 2);
        // Both palette colours should be present
        let colors: Vec<Rgb> = result.layers.iter().map(|l| l.color).collect();
        assert!(colors.contains(&Rgb { r: 0, g: 0, b: 255 }));
        assert!(colors.contains(&Rgb { r: 0, g: 255, b: 0 }));
    }

    // ── Transparent pixels excluded from clustering ───────────────────────

    #[test]
    fn transparent_pixels_excluded() {
        // 4×4 image: 8 opaque red pixels, 8 fully transparent pixels
        let mut pixels = Vec::new();
        for _y in 0..4 {
            for x in 0..4 {
                if x < 2 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(0, 0, 0, 0));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);

        // Only red pixels are clustered → 1 layer (1 unique colour)
        assert_eq!(result.layers.len(), 1);
        // Transparent pixels should have mask = 0
        assert_eq!(result.layers[0].mask[0], 255); // pixel (0,0) red
        assert_eq!(result.layers[0].mask[2], 0); // pixel (2,0) transparent
    }

    // ── Empty image ───────────────────────────────────────────────────────

    #[test]
    fn empty_image_returns_empty() {
        let img = make_image(1, 1, vec![0, 0, 0, 0]); // fully transparent
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);
        assert_eq!(result.layers.len(), 0);
    }

    #[test]
    fn stacked_layering_covers_more_pixels_and_respects_transparency() {
        let mut pixels = Vec::new();
        // 4x4 image:
        // Row 0-1: 2 red, 2 blue
        // Row 2-3: 2 red, 2 transparent
        for y in 0..4 {
            for x in 0..4 {
                if y < 2 {
                    if x >= 2 {
                        pixels.extend_from_slice(&rgb_pixel(0, 0, 255, 255)); // blue
                    } else {
                        pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255)); // red
                    }
                } else {
                    if x >= 2 {
                        pixels.extend_from_slice(&rgb_pixel(0, 0, 0, 0)); // transparent
                    } else {
                        pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255)); // red
                    }
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);

        // Run in Cutout mode
        let cutout_res = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);
        assert_eq!(cutout_res.layers.len(), 2);
        let red_cutout = cutout_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 255, g: 0, b: 0 })
            .unwrap();
        let blue_cutout = cutout_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 0, g: 0, b: 255 })
            .unwrap();

        // Run in Stacked mode
        let stacked_res = quantize(&input, &ColorSpec::Auto, &Layering::Stacked, 42);
        assert_eq!(stacked_res.layers.len(), 2);
        let red_stacked = stacked_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 255, g: 0, b: 0 })
            .unwrap();
        let blue_stacked = stacked_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 0, g: 0, b: 255 })
            .unwrap();

        // In cutout mode:
        // Red mask should cover 8 pixels (the red ones).
        // Blue mask should cover 4 pixels (the blue ones).
        let red_cutout_count = red_cutout.mask.iter().filter(|&&v| v > 0).count();
        let blue_cutout_count = blue_cutout.mask.iter().filter(|&&v| v > 0).count();
        assert_eq!(red_cutout_count, 8);
        assert_eq!(blue_cutout_count, 4);

        // In stacked mode, Red has z_order = 0 and Blue has z_order = 1.
        // Red stacked mask should be union of Red disjoint and Blue disjoint -> 12 pixels.
        // Blue stacked mask is topmost -> 4 pixels.
        let red_stacked_count = red_stacked.mask.iter().filter(|&&v| v > 0).count();
        let blue_stacked_count = blue_stacked.mask.iter().filter(|&&v| v > 0).count();

        assert_eq!(red_stacked_count, 12);
        assert_eq!(blue_stacked_count, 4);

        // Assert the actual pixel-count difference for the bottom layer is exactly 4
        assert_eq!(red_stacked_count - red_cutout_count, 4);

        // Confirm that transparent pixels are NEVER included in any layer's mask in stacked mode either.
        let transparent_indices = vec![10, 11, 14, 15];
        for idx in transparent_indices {
            assert_eq!(red_stacked.mask[idx], 0);
            assert_eq!(blue_stacked.mask[idx], 0);
        }
    }

    #[test]
    fn stacked_quantize_is_deterministic() {
        let w = 8u32;
        let h = 8u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 32) as u8).wrapping_add(10);
                let g = ((y * 32) as u8).wrapping_add(20);
                let b = ((x + y) * 16) as u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Photo);
        let seed = 98765u64;

        let r1 = quantize(&input, &ColorSpec::N(4), &Layering::Stacked, seed);
        let r2 = quantize(&input, &ColorSpec::N(4), &Layering::Stacked, seed);

        let json1 = serde_json::to_vec(&r1).unwrap();
        let json2 = serde_json::to_vec(&r2).unwrap();

        assert_eq!(json1, json2);
    }

    #[test]
    fn test_flat_art_quantization_consolidates_palette() {
        // 3 flat colours: Black (0, 0, 0), White (255, 255, 255), Orange (245, 80, 40)
        // plus anti-aliased edge pixels (near-identical oranges within 3..8 Lab)
        let w = 100u32;
        let h = 100u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let p = if y < 35 {
                    rgb_pixel(0, 0, 0, 255)
                } else if y < 70 {
                    rgb_pixel(255, 255, 255, 255)
                } else if y < 85 {
                    let v = ((x * 3 + y * 5) % 4) as u8;
                    match v {
                        0 => rgb_pixel(243, 81, 44, 255),
                        1 => rgb_pixel(252, 98, 41, 255),
                        2 => rgb_pixel(249, 93, 43, 255),
                        _ => rgb_pixel(247, 73, 32, 255),
                    }
                } else {
                    rgb_pixel(245, 80, 40, 255)
                };
                pixels.extend_from_slice(&p);
            }
        }
        let mut n_idx = 0usize;
        for dx in 0..5u8 {
            for dy in 0..5u8 {
                for dz in 0..3u8 {
                    let pixel_offset = n_idx * 4;
                    pixels[pixel_offset] = dx * 20;
                    pixels[pixel_offset + 1] = dy * 20;
                    pixels[pixel_offset + 2] = dz * 20;
                    pixels[pixel_offset + 3] = 255;
                    n_idx += 1;
                }
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Icon);

        let mut pdata = Vec::new();
        for i in 0..(w * h) as usize {
            let idx = i * 4;
            let rgb = Rgb {
                r: input.image.pixels[idx],
                g: input.image.pixels[idx + 1],
                b: input.image.pixels[idx + 2],
            };
            pdata.push(PixelInfo {
                pixel_index: i,
                rgb,
                lab: color::srgb_to_lab(&rgb),
            });
        }
        assert!(
            is_flat_art(&pdata),
            "synthetic flat image should be classified as flat art"
        );

        let result = quantize(&input, &ColorSpec::N(3), &Layering::Cutout, 42);

        assert_eq!(
            result.layers.len(),
            3,
            "expected 3 layers for 3-colour flat art, got {}",
            result.layers.len()
        );

        let labs: Vec<Lab> = result
            .layers
            .iter()
            .map(|l| color::srgb_to_lab(&l.color))
            .collect();
        for i in 0..labs.len() {
            for j in (i + 1)..labs.len() {
                let dist = color::lab_distance_sq(&labs[i], &labs[j]).sqrt();
                assert!(
                    dist >= FLAT_ART_DEDUPE_DIST,
                    "palette colors {:?} and {:?} are within FLAT_ART_DEDUPE_DIST ({:.2} < 10.0)",
                    result.layers[i].color,
                    result.layers[j].color,
                    dist
                );
            }
        }
    }

    #[test]
    fn test_smooth_gradient_is_not_flat_art() {
        let w = 32u32;
        let h = 32u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let r = (x * 8) as u8;
                let g = (y * 8) as u8;
                let b = ((x + y) * 4) as u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Photo);

        let mut pdata = Vec::new();
        for i in 0..(w * h) as usize {
            let idx = i * 4;
            let rgb = Rgb {
                r: input.image.pixels[idx],
                g: input.image.pixels[idx + 1],
                b: input.image.pixels[idx + 2],
            };
            pdata.push(PixelInfo {
                pixel_index: i,
                rgb,
                lab: color::srgb_to_lab(&rgb),
            });
        }
        assert!(
            !is_flat_art(&pdata),
            "smooth gradient image should NOT be classified as flat art"
        );
    }

    #[test]
    fn test_flat_art_determinism() {
        let w = 16u32;
        let h = 16u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let p = if (x + y) % 3 == 0 {
                    rgb_pixel(10, 10, 10, 255)
                } else if (x + y) % 3 == 1 {
                    rgb_pixel(200, 50, 30, 255)
                } else {
                    rgb_pixel(250, 250, 250, 255)
                };
                pixels.extend_from_slice(&p);
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Icon);
        let seed = 9999u64;

        let r1 = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, seed);
        let r2 = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, seed);

        let json1 = serde_json::to_vec(&r1).unwrap();
        let json2 = serde_json::to_vec(&r2).unwrap();

        assert_eq!(
            json1, json2,
            "flat art quantization must be byte-identical on repeated runs"
        );
    }
