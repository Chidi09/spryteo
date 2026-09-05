    use super::*;

    #[test]
    fn test_crop_full_rect_is_identity() {
        let pixels: Vec<u8> = (0..64).map(|i| (i * 17) as u8).collect();
        let img = RasterImage {
            width: 4,
            height: 4,
            pixels,
        };
        let result = crop(&img, 0, 0, 4, 4);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
        assert_eq!(result.pixels, img.pixels);
    }

    #[test]
    fn test_crop_clamps_out_of_bounds() {
        let pixels: Vec<u8> = vec![0u8; 4 * 4 * 4];
        let img = RasterImage {
            width: 4,
            height: 4,
            pixels,
        };

        // Request larger than source
        let result = crop(&img, 0, 0, 100, 100);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);

        // Origin past edge
        let result = crop(&img, 5, 0, 10, 10);
        assert_eq!(result.width, 0);
        assert_eq!(result.height, 0);

        let result = crop(&img, 0, 5, 10, 10);
        assert_eq!(result.width, 0);
        assert_eq!(result.height, 0);
    }

    #[test]
    fn test_crop_subregion_pixels() {
        let mut pixels = Vec::with_capacity(4 * 4 * 4);
        for r in 0..4u8 {
            for c in 0..4u8 {
                let val = r * 4 + c;
                pixels.push(val);
                pixels.push(val);
                pixels.push(val);
                pixels.push(255);
            }
        }
        let img = RasterImage {
            width: 4,
            height: 4,
            pixels,
        };
        let result = crop(&img, 1, 1, 2, 2);
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 2);
        let expected: Vec<u8> = vec![5, 5, 5, 255, 6, 6, 6, 255, 9, 9, 9, 255, 10, 10, 10, 255];
        assert_eq!(result.pixels, expected);
    }

    #[test]
    fn test_crop_zero_size_image() {
        let img = RasterImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        let result = crop(&img, 0, 0, 10, 10);
        assert_eq!(result.width, 0);
        assert_eq!(result.height, 0);
        assert!(result.pixels.is_empty());
    }

    #[test]
    fn test_upscale_factor_one_is_identity() {
        let pixels: Vec<u8> = (0..64).map(|i| (i * 17) as u8).collect();
        let img = RasterImage {
            width: 4,
            height: 4,
            pixels: pixels.clone(),
        };
        let result = upscale_bicubic(&img, 1);
        assert_eq!(result.width, 4);
        assert_eq!(result.height, 4);
        assert_eq!(result.pixels, pixels);
    }

    #[test]
    fn test_upscale_dimensions() {
        let pixels = vec![128u8; 4 * 3 * 5];
        let img = RasterImage {
            width: 3,
            height: 5,
            pixels,
        };
        let result = upscale_bicubic(&img, 4);
        assert_eq!(result.width, 12);
        assert_eq!(result.height, 20);
        assert_eq!(result.pixels.len(), 4 * 12 * 20);
    }

    #[test]
    fn test_upscale_flat_color_is_preserved() {
        let pixels = [100u8, 150, 200, 255].repeat(4 * 4);
        let img = RasterImage {
            width: 4,
            height: 4,
            pixels,
        };
        let result = upscale_bicubic(&img, 4);
        let expected_pixel = [100u8, 150, 200, 255];
        for chunk in result.pixels.as_chunks::<4>().0 {
            assert_eq!(chunk, &expected_pixel);
        }
    }

    #[test]
    fn test_upscale_all_channels_finite_and_in_range() {
        let mut pixels = Vec::with_capacity(4 * 2 * 2);
        let colors: [(u8, u8, u8, u8); 4] = [
            (0, 0, 0, 255),
            (255, 255, 255, 255),
            (255, 255, 255, 255),
            (0, 0, 0, 255),
        ];
        for &(r, g, b, a) in &colors {
            pixels.push(r);
            pixels.push(g);
            pixels.push(b);
            pixels.push(a);
        }
        let img = RasterImage {
            width: 2,
            height: 2,
            pixels,
        };
        let result = upscale_bicubic(&img, 4);
        assert_eq!(result.width, 8);
        assert_eq!(result.height, 8);
        assert_eq!(result.pixels.len(), 4 * 8 * 8);
    }

    /// Guards the sample mapping specifically. Flat-colour and dimension tests
    /// both pass under the nearest-neighbour mapping `src = ox / factor`, which
    /// shifts the whole image half a source pixel -- and a half-pixel shift is
    /// exactly the error supersampling exists to avoid. A 2x1 black|white edge
    /// is antisymmetric about x = 0.5, so a correctly centred 4-wide upscale
    /// must be antisymmetric too: mirror pairs sum to 255.
    #[test]
    fn test_upscale_sample_positions_are_pixel_centred() {
        let img = RasterImage {
            width: 2,
            height: 1,
            pixels: vec![0, 0, 0, 255, 255, 255, 255, 255],
        };
        let out = upscale_bicubic(&img, 2);
        assert_eq!(out.width, 4);
        let lum: Vec<i32> = (0..4).map(|i| out.pixels[i * 4] as i32).collect();
        assert_eq!(lum[0] + lum[3], 255, "outer pair not mirrored: {:?}", lum);
        assert_eq!(lum[1] + lum[2], 255, "inner pair not mirrored: {:?}", lum);
        assert!(
            lum[0] < lum[1] && lum[1] < lum[2] && lum[2] < lum[3],
            "edge should ramp monotonically dark->light: {:?}",
            lum
        );
    }

    /// The vertical twin of the test above, and the one that was missing when
    /// a row-stride bug shipped: that test used a 2x1 image, so with height 1
    /// the row offset was always 0 and an incorrect row stride was invisible.
    /// Any test on a single-row image is blind to this whole class of bug.
    #[test]
    fn test_upscale_sample_positions_are_pixel_centred_vertically() {
        let img = RasterImage {
            width: 1,
            height: 2,
            pixels: vec![0, 0, 0, 255, 255, 255, 255, 255],
        };
        let out = upscale_bicubic(&img, 2);
        assert_eq!((out.width, out.height), (2, 4));
        // Column 0 of each of the 4 output rows.
        let lum: Vec<i32> = (0..4).map(|r| out.pixels[r * 2 * 4] as i32).collect();
        assert_eq!(lum[0] + lum[3], 255, "outer pair not mirrored: {:?}", lum);
        assert_eq!(lum[1] + lum[2], 255, "inner pair not mirrored: {:?}", lum);
        assert!(
            lum[0] < lum[1] && lum[1] < lum[2] && lum[2] < lum[3],
            "edge should ramp monotonically dark->light: {:?}",
            lum
        );
    }

    /// A row-stride error scrambles rows into each other, so a source whose
    /// rows are strongly distinct must upscale to something that still varies
    /// down the column in the same direction. Catches stride bugs that a
    /// symmetric fixture can mask.
    #[test]
    fn test_upscale_preserves_row_ordering() {
        // 2x3: rows are pure red, pure green, pure blue.
        let img = RasterImage {
            width: 2,
            height: 3,
            #[rustfmt::skip]
            pixels: vec![
                255, 0, 0, 255,  255, 0, 0, 255,
                0, 255, 0, 255,  0, 255, 0, 255,
                0, 0, 255, 255,  0, 0, 255, 255,
            ],
        };
        let out = upscale_bicubic(&img, 3);
        assert_eq!((out.width, out.height), (6, 9));
        let px = |x: usize, y: usize| {
            let i = (y * out.width as usize + x) * 4;
            (out.pixels[i], out.pixels[i + 1], out.pixels[i + 2])
        };
        let (r_top, g_top, b_top) = px(2, 1);
        let (r_mid, g_mid, b_mid) = px(2, 4);
        let (r_bot, g_bot, b_bot) = px(2, 7);
        assert!(
            r_top > g_top && r_top > b_top,
            "top band should stay red-dominant, got {:?}",
            (r_top, g_top, b_top)
        );
        assert!(
            g_mid > r_mid && g_mid > b_mid,
            "middle band should stay green-dominant, got {:?}",
            (r_mid, g_mid, b_mid)
        );
        assert!(
            b_bot > r_bot && b_bot > g_bot,
            "bottom band should stay blue-dominant, got {:?}",
            (r_bot, g_bot, b_bot)
        );
    }

    #[test]
    fn test_upscale_zero_size_image() {
        let img = RasterImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        let result = upscale_bicubic(&img, 4);
        assert_eq!(result.width, 0);
        assert_eq!(result.height, 0);
        assert!(result.pixels.is_empty());
    }
