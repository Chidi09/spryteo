    use super::*;
    use spryteo_core::RasterImage;

    /// Build a tiny RGBA raster by hand (no decoder needed).
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

    // ── Icon classification ──────────────────────────────────────────────

    #[test]
    fn two_colour_rgba_classifies_as_icon() {
        // 4×4 image: left half red (with alpha = 128), right half white
        // The flat alpha fills trigger icon detection.
        let mut pixels = Vec::new();
        for _y in 0..4 {
            for x in 0..4 {
                if x < 2 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 128));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let result = classify(img, &Mode::Auto);
        assert!(
            matches!(result.mode, Mode::Icon),
            "expected Icon, got {:?}",
            result.mode
        );
    }

    // ── Pixel-art classification ─────────────────────────────────────────

    #[test]
    fn opaque_few_colours_hard_edges_classifies_as_pixel_art() {
        // 8×8 image with 10 distinct opaque colours, each as solid 2×2 blocks.
        // No alpha, no blended edges → pixel-art.
        let colours: Vec<[u8; 4]> = vec![
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
            [255, 0, 255, 255],
            [0, 255, 255, 255],
            [128, 0, 0, 255],
            [0, 128, 0, 255],
            [0, 0, 128, 255],
            [128, 128, 0, 255],
        ];

        let mut pixels = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                let ci = ((y / 2) * 4 + (x / 2)) % colours.len();
                pixels.extend_from_slice(&colours[ci]);
            }
        }

        let img = make_image(8, 8, pixels);
        let result = classify(img, &Mode::Auto);
        assert!(
            matches!(result.mode, Mode::PixelArt),
            "expected PixelArt, got {:?}",
            result.mode
        );
    }

    #[test]
    fn pixel_art_and_icon_classify_differently() {
        // ── Pixel-art input (opaque, hard edges, 10 colours) ──
        let colours_pa: Vec<[u8; 4]> = vec![
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
            [255, 0, 255, 255],
            [0, 255, 255, 255],
            [128, 0, 0, 255],
            [0, 128, 0, 255],
            [0, 0, 128, 255],
            [128, 128, 0, 255],
        ];
        let mut pixels_pa = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                let ci = ((y / 2) * 4 + (x / 2)) % colours_pa.len();
                pixels_pa.extend_from_slice(&colours_pa[ci]);
            }
        }
        let img_pa = make_image(8, 8, pixels_pa);

        // ── Icon input (same RGB arrangement but with flat alpha) ──
        let colours_icon: Vec<[u8; 4]> = vec![
            [255, 0, 0, 128],
            [0, 255, 0, 128],
            [0, 0, 255, 128],
            [255, 255, 0, 128],
            [255, 0, 255, 128],
            [0, 255, 255, 128],
            [128, 0, 0, 128],
            [0, 128, 0, 128],
            [0, 0, 128, 128],
            [128, 128, 0, 128],
        ];
        let mut pixels_icon = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                let ci = ((y / 2) * 4 + (x / 2)) % colours_icon.len();
                pixels_icon.extend_from_slice(&colours_icon[ci]);
            }
        }
        let img_icon = make_image(8, 8, pixels_icon);

        let result_pa = classify(img_pa, &Mode::Auto);
        let result_icon = classify(img_icon, &Mode::Auto);

        assert!(
            matches!(result_pa.mode, Mode::PixelArt),
            "expected PixelArt, got {:?}",
            result_pa.mode
        );
        assert!(
            matches!(result_icon.mode, Mode::Icon),
            "expected Icon, got {:?}",
            result_icon.mode
        );
    }

    // ── Forced mode ──────────────────────────────────────────────────────

    #[test]
    fn forced_mode_skips_heuristics() {
        let img = make_image(4, 4, vec![0u8; 64]); // black, opaque
        let result = classify(img, &Mode::Photo);
        assert!(matches!(result.mode, Mode::Photo));
    }

    // ── Background detection ──────────────────────────────────────────────

    #[test]
    fn background_detection_works() {
        // 10×10 image: 2-pixel solid white border, red (255,0,0) interior
        let w = 10u32;
        let h = 10u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if x < 2 || x >= w - 2 || y < 2 || y >= h - 2 {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                }
            }
        }
        let img = make_image(w, h, pixels);
        let bg = detect_background(&img);
        assert!(bg.is_some(), "expected a background colour");
        let bg = bg.unwrap();
        assert_eq!(bg.r, 255);
        assert_eq!(bg.g, 255);
        assert_eq!(bg.b, 255);
    }

    #[test]
    fn background_none_when_corners_differ() {
        // 4×4 image: each corner a different colour
        let pixels: Vec<u8> = vec![
            // row 0: red, *, *, green
            255, 0, 0, 255, 128, 128, 128, 255, 128, 128, 128, 255, 0, 255, 0, 255,
            // row 1
            128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255,
            // row 2
            128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255, 128, 128, 128, 255,
            // row 3: blue, *, *, yellow
            0, 0, 255, 255, 128, 128, 128, 255, 128, 128, 128, 255, 255, 255, 0, 255,
        ];
        let img = make_image(4, 4, pixels);
        let bg = detect_background(&img);
        assert!(bg.is_none(), "expected no background, got {:?}", bg);
    }

    // ── Line-art simplified heuristics ───────────────────────────────────

    #[test]
    fn line_art_classification() {
        // A mostly-white image with thin dark lines → line-art candidate
        let w = 16u32;
        let h = 16u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                // Draw a few dark pixels as "ink"
                if x == y || x == w - 1 - y {
                    pixels.extend_from_slice(&rgb_pixel(0, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                }
            }
        }
        let img = make_image(w, h, pixels);
        let result = classify(img, &Mode::Auto);

        // The two diagonal lines (16 + 16 - 1 ≈ 31 ink pixels out of 256)
        // give an ink ratio of ~12% which is < 25%
        assert!(
            matches!(result.mode, Mode::LineArt),
            "expected LineArt, got {:?}",
            result.mode
        );
    }
