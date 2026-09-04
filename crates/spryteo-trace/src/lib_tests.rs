    use super::*;
    use spryteo_core::ir::{Layer, Rgb};

    fn make_layer(mask: Vec<u8>) -> Layer {
        Layer {
            mask,
            color: Rgb { r: 255, g: 0, b: 0 },
            z_order: 0,
        }
    }

    #[test]
    fn test_single_solid_filled_square() {
        // A 6x6 image with a 4x4 filled square in the center (from 1,1 to 4,4 inclusive)
        let mut mask = vec![0u8; 36];
        for r in 1..=4 {
            for c in 1..=4 {
                mask[r * 6 + c] = 255;
            }
        }
        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };
        let set = extract_contours(&stack, 6, 6, 0);

        assert_eq!(set.layers.len(), 1);
        let contours = &set.layers[0];
        assert_eq!(contours.len(), 1);

        let contour = &contours[0];
        assert!(contour.children.is_empty());

        // Bounding box of the contour
        let min_x = contour
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min);
        let max_x = contour
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = contour
            .points
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min);
        let max_y = contour
            .points
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max);

        // Pixels 1..=4 cover image space 1.0..5.0; the traced boundary sits
        // half a pixel outside the on-pixel centers.
        assert!((min_x - 1.0).abs() < 0.1);
        assert!((max_x - 5.0).abs() < 0.1);
        assert!((min_y - 1.0).abs() < 0.1);
        assert!((max_y - 5.0).abs() < 0.1);
    }

    #[test]
    fn test_soft_edge_interpolation() {
        // Create two 6x6 images. One with a hard edge, one with a soft edge.
        // Hard edge:
        // 0 0 0 0 0 0
        // 0 0 255 255 0 0
        // ...
        let mut hard_mask = vec![0u8; 36];
        hard_mask[8] = 255;
        hard_mask[9] = 255;

        // Soft edge:
        // 0 0 0 0 0 0
        // 0 0 160 255 0 0
        let mut soft_mask = vec![0u8; 36];
        soft_mask[8] = 160;
        soft_mask[9] = 255;

        let hard_stack = LayerStack {
            layers: vec![make_layer(hard_mask)],
        };
        let soft_stack = LayerStack {
            layers: vec![make_layer(soft_mask)],
        };

        let hard_set = extract_contours(&hard_stack, 6, 6, 0);
        let soft_set = extract_contours(&soft_stack, 6, 6, 0);

        let hard_pts = &hard_set.layers[0][0].points;
        let soft_pts = &soft_set.layers[0][0].points;

        assert_ne!(hard_pts, soft_pts);

        // The point on the left edge of the soft mask should be interpolated
        // and have a different subpixel X coordinate.
        // For hard mask, 0 -> 255 cross at 0.5 fraction (coordinate 1.5).
        // For soft mask, 0 -> 160 crosses at (127.5 / 160) = 0.796875 fraction (coordinate 1.796875).
        let hard_x_coords: Vec<f64> = hard_pts.iter().map(|p| p.0).collect();
        let soft_x_coords: Vec<f64> = soft_pts.iter().map(|p| p.0).collect();
        assert_ne!(hard_x_coords, soft_x_coords);
    }

    #[test]
    fn test_ring_shape_with_hole() {
        // A 7x7 image with a 5x5 filled square having a 1x1 hole in the center.
        // Filled: 1..5 inclusive, hole at 3,3.
        let mut mask = vec![0u8; 49];
        for r in 1..=5 {
            for c in 1..=5 {
                if r == 3 && c == 3 {
                    mask[r * 7 + c] = 0;
                } else {
                    mask[r * 7 + c] = 255;
                }
            }
        }

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };
        let set = extract_contours(&stack, 7, 7, 0);

        assert_eq!(set.layers[0].len(), 1);
        let parent = &set.layers[0][0];
        assert_eq!(parent.children.len(), 1);

        let hole = &parent.children[0];
        assert!(hole.children.is_empty());

        // Hole pixel 3 covers image space 3.0..4.0; the traced boundary sits
        // half a pixel inside the surrounding on-pixel centers.
        let min_x = hole
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min);
        let max_x = hole
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((min_x - 3.0).abs() < 0.1);
        assert!((max_x - 4.0).abs() < 0.1);
    }

    #[test]
    fn test_despeckle() {
        // A tiny 1-pixel speck in a 6x6 image
        let mut mask = vec![0u8; 36];
        mask[2 * 6 + 2] = 255;

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };

        // turdsize = 5 should drop it
        let set_dropped = extract_contours(&stack, 6, 6, 5);
        assert!(set_dropped.layers[0].is_empty());

        // turdsize = 0 should keep it
        let set_kept = extract_contours(&stack, 6, 6, 0);
        assert_eq!(set_kept.layers[0].len(), 1);
    }

    #[test]
    fn test_determinism() {
        // An image with two distinct components and a hole
        let mut mask = vec![0u8; 64];
        // Component 1: 3x3 square at 1,1
        for r in 1..=3 {
            for c in 1..=3 {
                mask[r * 8 + c] = 255;
            }
        }
        // Component 2: 4x4 square at 4,4 with a hole at 5,5
        for r in 4..=7 {
            for c in 4..=7 {
                if r == 5 && c == 5 {
                    mask[r * 8 + c] = 0;
                } else {
                    mask[r * 8 + c] = 255;
                }
            }
        }

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };

        let result1 = extract_contours(&stack, 8, 8, 0);
        let result2 = extract_contours(&stack, 8, 8, 0);

        let json1 = serde_json::to_string(&result1).unwrap();
        let json2 = serde_json::to_string(&result2).unwrap();

        assert_eq!(json1, json2);
    }

    #[test]
    fn test_two_separate_regions() {
        // An 8x8 image with two 2x2 squares
        let mut mask = vec![0u8; 64];
        for r in 1..=2 {
            for c in 1..=2 {
                mask[r * 8 + c] = 255;
            }
        }
        for r in 5..=6 {
            for c in 5..=6 {
                mask[r * 8 + c] = 255;
            }
        }

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };
        let set = extract_contours(&stack, 8, 8, 0);

        assert_eq!(set.layers[0].len(), 2);
    }
