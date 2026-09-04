    use super::*;
    use spryteo_core::{Layer, RasterImage, Rgb};

    #[test]
    fn linear_gradient_detection() {
        let mut pixels = Vec::new();
        // 10x10 horizontal linear ramp from Red at x=0 to Blue at x=9
        for _y in 0..10 {
            for x in 0..10 {
                let r = (255.0 * (1.0 - x as f64 / 9.0)).round() as u8;
                let g = 0;
                let b = (255.0 * (x as f64 / 9.0)).round() as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let image = RasterImage {
            width: 10,
            height: 10,
            pixels,
        };
        let layer = Layer {
            mask: vec![255; 100],
            color: Rgb {
                r: 128,
                g: 0,
                b: 128,
            },
            z_order: 0,
        };

        let fill = detect_gradient(&image, &layer, 15.0).expect("should detect linear gradient");
        if let Fill::LinearGradient {
            x1,
            y1,
            x2,
            y2,
            stops,
        } = fill
        {
            assert_eq!(stops.len(), 2);
            // Verify stop offsets are 0.0 and 1.0
            assert_eq!(stops[0].offset, 0.0);
            assert_eq!(stops[1].offset, 1.0);

            // Verify stops are close to endpoints Red/Blue (or Blue/Red) in Lab space
            let d1_red = color::lab_distance_sq(
                &color::srgb_to_lab(&stops[0].color),
                &color::srgb_to_lab(&Rgb { r: 255, g: 0, b: 0 }),
            );
            let d2_blue = color::lab_distance_sq(
                &color::srgb_to_lab(&stops[1].color),
                &color::srgb_to_lab(&Rgb { r: 0, g: 0, b: 255 }),
            );
            let d1_blue = color::lab_distance_sq(
                &color::srgb_to_lab(&stops[0].color),
                &color::srgb_to_lab(&Rgb { r: 0, g: 0, b: 255 }),
            );
            let d2_red = color::lab_distance_sq(
                &color::srgb_to_lab(&stops[1].color),
                &color::srgb_to_lab(&Rgb { r: 255, g: 0, b: 0 }),
            );

            let normal = d1_red < 250.0 && d2_blue < 250.0;
            let reversed = d1_blue < 250.0 && d2_red < 250.0;
            assert!(
                normal || reversed,
                "Stops colors {:?} are not close to Red and Blue",
                stops
            );

            // Verify roughly horizontal (dx is much larger than dy)
            let dx = x2 - x1;
            let dy = y2 - y1;
            assert!(
                dx.abs() > 3.0 * dy.abs(),
                "gradient should be horizontal: dx={}, dy={}",
                dx,
                dy
            );
        } else {
            panic!("Expected LinearGradient, got {:?}", fill);
        }
    }

    #[test]
    fn radial_gradient_detection() {
        let mut pixels = Vec::new();
        let cx_true = 5.0;
        let cy_true = 5.0;
        let max_d = 50.0_f64.sqrt(); // max distance from (5,5) in 11x11 image is sqrt(5^2+5^2)
                                     // 11x11 image with radial color variation from center (5,5)
        for y in 0..11 {
            for x in 0..11 {
                let dx = x as f64 - cx_true;
                let dy = y as f64 - cy_true;
                let d = (dx * dx + dy * dy).sqrt();
                let r = (255.0 * (1.0 - d / max_d)).round() as u8;
                let g = 0;
                let b = (255.0 * (d / max_d)).round() as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let image = RasterImage {
            width: 11,
            height: 11,
            pixels,
        };
        let layer = Layer {
            mask: vec![255; 121],
            color: Rgb {
                r: 128,
                g: 0,
                b: 128,
            },
            z_order: 0,
        };

        let fill = detect_gradient(&image, &layer, 15.0).expect("should detect radial gradient");
        if let Fill::RadialGradient { cx, cy, r, stops } = fill {
            assert_eq!(stops.len(), 2);
            assert_eq!(stops[0].offset, 0.0);
            assert_eq!(stops[1].offset, 1.0);

            // Verify centroid/radius are close to true values
            assert!((cx - cx_true).abs() < 0.5, "cx is not close: {}", cx);
            assert!((cy - cy_true).abs() < 0.5, "cy is not close: {}", cy);
            assert!((r - max_d).abs() < 0.5, "radius is not close: {}", r);
        } else {
            panic!("Expected RadialGradient, got {:?}", fill);
        }
    }

    #[test]
    fn flat_region_returns_solid() {
        let mut pixels = Vec::new();
        // 10x10 flat single color Red
        for _ in 0..100 {
            pixels.extend_from_slice(&[255, 0, 0, 255]);
        }
        let image = RasterImage {
            width: 10,
            height: 10,
            pixels,
        };
        let layer = Layer {
            mask: vec![255; 100],
            color: Rgb { r: 255, g: 0, b: 0 },
            z_order: 0,
        };

        let fill = detect_gradient(&image, &layer, 5.0);
        assert!(
            fill.is_none(),
            "flat region should return None, got {:?}",
            fill
        );
    }

    #[test]
    fn noisy_region_returns_solid() {
        let mut pixels = Vec::new();
        // 10x10 checkerboard of Red and Green (highly noisy, no smooth gradient)
        for y in 0..10 {
            for x in 0..10 {
                if (x + y) % 2 == 0 {
                    pixels.extend_from_slice(&[255, 0, 0, 255]);
                } else {
                    pixels.extend_from_slice(&[0, 255, 0, 255]);
                }
            }
        }
        let image = RasterImage {
            width: 10,
            height: 10,
            pixels,
        };
        let layer = Layer {
            mask: vec![255; 100],
            color: Rgb {
                r: 128,
                g: 128,
                b: 0,
            },
            z_order: 0,
        };

        let fill = detect_gradient(&image, &layer, 15.0);
        assert!(
            fill.is_none(),
            "noisy region should exceed tolerance and return None, got {:?}",
            fill
        );
    }

    #[test]
    fn gradient_detection_is_deterministic() {
        let mut pixels = Vec::new();
        for _y in 0..10 {
            for x in 0..10 {
                let r = (255.0 * (1.0 - x as f64 / 9.0)).round() as u8;
                let g = 0;
                let b = (255.0 * (x as f64 / 9.0)).round() as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let image = RasterImage {
            width: 10,
            height: 10,
            pixels,
        };
        let layer = Layer {
            mask: vec![255; 100],
            color: Rgb {
                r: 128,
                g: 0,
                b: 128,
            },
            z_order: 0,
        };

        let fill1 = detect_gradient(&image, &layer, 15.0);
        let fill2 = detect_gradient(&image, &layer, 15.0);
        assert_eq!(fill1, fill2);
    }
