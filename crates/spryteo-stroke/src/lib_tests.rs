    use super::*;
    use spryteo_core::ir::{PathElement, RasterImage};

    fn make_empty_image(w: u32, h: u32) -> RasterImage {
        let pixels = vec![255; (w * h * 4) as usize];
        RasterImage {
            width: w,
            height: h,
            pixels,
        }
    }

    fn draw_pixel(image: &mut RasterImage, x: u32, y: u32) {
        if x >= image.width || y >= image.height {
            return;
        }
        let idx = ((y * image.width + x) * 4) as usize;
        image.pixels[idx] = 0;
        image.pixels[idx + 1] = 0;
        image.pixels[idx + 2] = 0;
        image.pixels[idx + 3] = 255;
    }

    fn draw_line(image: &mut RasterImage, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs());
        for s in 0..=steps {
            let t = if steps == 0 {
                0.0
            } else {
                s as f64 / steps as f64
            };
            let cx = x0 as f64 + t * (x1 - x0) as f64;
            let cy = y0 as f64 + t * (y1 - y0) as f64;
            for dy in -thickness..=thickness {
                for dx in -thickness..=thickness {
                    if dx * dx + dy * dy <= thickness * thickness {
                        let px = (cx + dx as f64).round() as i32;
                        let py = (cy + dy as f64).round() as i32;
                        if px >= 0 && px < image.width as i32 && py >= 0 && py < image.height as i32
                        {
                            draw_pixel(image, px as u32, py as u32);
                        }
                    }
                }
            }
        }
    }

    fn get_endpoints(segments: &[PathElement]) -> ((f64, f64), (f64, f64)) {
        let start = match segments[0] {
            PathElement::MoveTo(x, y) => (x, y),
            _ => panic!("Expected MoveTo"),
        };
        let end = match *segments.last().unwrap() {
            PathElement::MoveTo(x, y) => (x, y),
            PathElement::LineTo(x, y) => (x, y),
            PathElement::CurveTo(_, _, _, _, x, y) => (x, y),
            PathElement::ClosePath => panic!("Did not expect ClosePath"),
        };
        (start, end)
    }

    fn check_finiteness(segments: &[PathElement]) {
        for elem in segments {
            match *elem {
                PathElement::MoveTo(x, y) => {
                    assert!(x.is_finite());
                    assert!(y.is_finite());
                }
                PathElement::LineTo(x, y) => {
                    assert!(x.is_finite());
                    assert!(y.is_finite());
                }
                PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                    assert!(x1.is_finite());
                    assert!(y1.is_finite());
                    assert!(x2.is_finite());
                    assert!(y2.is_finite());
                    assert!(x3.is_finite());
                    assert!(y3.is_finite());
                }
                PathElement::ClosePath => {}
            }
        }
    }

    #[test]
    fn test_straight_line() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res = trace_stroke(&img, 0.5);
        assert_eq!(res.path_count, 1);
        let segments = &res.curves.curves[0].segments;
        check_finiteness(segments);

        let (start, end) = get_endpoints(segments);
        let d1 = (start.0 - 5.0).abs()
            + (start.1 - 10.0).abs()
            + (end.0 - 25.0).abs()
            + (end.1 - 10.0).abs();
        let d2 = (start.0 - 25.0).abs()
            + (start.1 - 10.0).abs()
            + (end.0 - 5.0).abs()
            + (end.1 - 10.0).abs();
        assert!(
            d1 < 4.0 || d2 < 4.0,
            "Endpoints did not match (start: {:?}, end: {:?})",
            start,
            end
        );
    }

    #[test]
    fn test_plus_shape() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 25, 15, 0);
        draw_line(&mut img, 15, 5, 15, 25, 0);

        let res = trace_stroke(&img, 0.5);
        assert!(res.path_count >= 2, "Path count was {}", res.path_count);
        for curve in &res.curves.curves {
            check_finiteness(&curve.segments);
        }
    }

    #[test]
    fn test_tiny_dot() {
        let mut img = make_empty_image(16, 16);
        draw_pixel(&mut img, 8, 8);
        let res = trace_stroke(&img, 0.5);
        assert!(res.path_count <= 1);
        for curve in &res.curves.curves {
            check_finiteness(&curve.segments);
        }
    }

    #[test]
    fn test_stroke_width() {
        let mut img_thin = make_empty_image(32, 32);
        draw_line(&mut img_thin, 5, 16, 25, 16, 0);
        let res_thin = trace_stroke(&img_thin, 0.5);
        assert_eq!(res_thin.path_count, 1);
        let w_thin = res_thin.widths[0];

        let mut img_fat = make_empty_image(32, 32);
        draw_line(&mut img_fat, 5, 16, 25, 16, 2);
        let res_fat = trace_stroke(&img_fat, 0.5);
        assert_eq!(res_fat.path_count, 1);
        let w_fat = res_fat.widths[0];

        assert!(
            w_fat > w_thin * 1.5,
            "Fat width ({}) should be significantly larger than thin width ({})",
            w_fat,
            w_thin
        );
    }

    #[test]
    fn test_spur_pruning() {
        let mut img_short = make_empty_image(32, 32);
        draw_line(&mut img_short, 5, 10, 25, 10, 0);
        draw_pixel(&mut img_short, 15, 9);
        draw_pixel(&mut img_short, 15, 8);

        let res_short = trace_stroke(&img_short, 0.5);
        assert_eq!(res_short.path_count, 1);

        let mut img_long = make_empty_image(32, 32);
        draw_line(&mut img_long, 5, 10, 25, 10, 0);
        for y in 5..=9 {
            draw_pixel(&mut img_long, 15, y as u32);
        }

        let res_long = trace_stroke(&img_long, 0.5);
        assert!(
            res_long.path_count >= 2,
            "Long spur did not survive, path count was {}",
            res_long.path_count
        );
    }

    #[test]
    fn test_determinism() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 25, 15, 1);
        draw_line(&mut img, 15, 5, 15, 25, 1);

        let res1 = trace_stroke(&img, 0.5);
        let res2 = trace_stroke(&img, 0.5);

        let json1 = serde_json::to_vec(&res1.curves).unwrap();
        let json2 = serde_json::to_vec(&res2.curves).unwrap();
        assert_eq!(json1, json2);
        assert_eq!(res1.widths, res2.widths);
    }

    #[test]
    fn test_trace_stroke_ex_matches_trace_stroke_on_defaults() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res = trace_stroke(&img, 1.0);
        let res_ex = trace_stroke_ex(&img, &StrokeOptions::default());

        assert_eq!(res.path_count, res_ex.paths.len());
        for (w1, p2) in res.widths.iter().zip(res_ex.paths.iter()) {
            assert_eq!(*w1, p2.width);
        }
    }

    #[test]
    fn test_ink_source_mask_is_used_instead_of_luminance() {
        let w = 32;
        let h = 32;
        let pixels = vec![240; (w * h * 4) as usize];
        let img = RasterImage {
            width: w,
            height: h,
            pixels,
        };

        let res_lum = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res_lum.paths.len(), 0);

        let mut mask = vec![false; (w * h) as usize];
        for x in 5..=25 {
            mask[(10 * w + x) as usize] = true;
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            ..StrokeOptions::default()
        };
        let res_mask = trace_stroke_ex(&img, &opts);
        assert!(!res_mask.paths.is_empty());
    }

    #[test]
    fn test_ink_source_mask_wrong_length_returns_empty() {
        let img = make_empty_image(32, 32);
        let wrong_mask = vec![true; 10];
        let opts = StrokeOptions {
            ink: InkSource::Mask(wrong_mask),
            ..StrokeOptions::default()
        };
        let res = trace_stroke_ex(&img, &opts);
        assert_eq!(res.paths.len(), 0);
        assert_eq!(res.component_count, 0);
    }

    #[test]
    fn test_component_indices_are_assigned() {
        let mut img = make_empty_image(64, 64);
        draw_line(&mut img, 5, 5, 15, 5, 0);
        draw_line(&mut img, 5, 25, 15, 25, 0);
        draw_line(&mut img, 5, 45, 15, 45, 0);

        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res.component_count, 3);
        let mut components: Vec<usize> = res.paths.iter().map(|p| p.component).collect();
        components.sort_unstable();
        components.dedup();
        assert_eq!(components, vec![0, 1, 2]);
    }

    #[test]
    fn test_min_component_pixels_drops_small_components() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);
        draw_pixel(&mut img, 2, 2);
        draw_pixel(&mut img, 3, 2);

        let opts_default = StrokeOptions {
            prune_spurs: false,
            min_component_pixels: 0,
            ..StrokeOptions::default()
        };
        let res_default = trace_stroke_ex(&img, &opts_default);

        let opts_filtered = StrokeOptions {
            prune_spurs: false,
            min_component_pixels: 5,
            ..StrokeOptions::default()
        };
        let res_filtered = trace_stroke_ex(&img, &opts_filtered);

        assert!(
            res_default.component_count > res_filtered.component_count,
            "default should have more components ({}) than filtered ({})",
            res_default.component_count,
            res_filtered.component_count
        );
    }

    #[test]
    fn test_width_profile_is_not_collapsed() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 12, 15, 0);
        draw_line(&mut img, 13, 15, 25, 15, 2);

        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert!(!res.paths.is_empty());
        let path = &res.paths[0];
        assert!(path.width_profile.len() > 2);

        let min_val = path
            .width_profile
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        let max_val = path
            .width_profile
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_val > min_val,
            "width profile should contain distinct values"
        );

        let expected_median_width = 2.0 * median(path.width_profile.clone());
        assert!((path.width - expected_median_width).abs() < 1e-6);
    }

    #[test]
    fn test_closed_flag_for_loop() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 10, 10, 20, 10, 0);
        draw_line(&mut img, 20, 10, 20, 20, 0);
        draw_line(&mut img, 20, 20, 10, 20, 0);
        draw_line(&mut img, 10, 20, 10, 10, 0);

        let res_closed = trace_stroke_ex(&img, &StrokeOptions::default());
        assert!(
            res_closed.paths.iter().any(|p| p.closed),
            "Loop image should yield at least one closed path"
        );

        let mut img_line = make_empty_image(32, 32);
        draw_line(&mut img_line, 5, 10, 25, 10, 0);
        let res_line = trace_stroke_ex(&img_line, &StrokeOptions::default());
        assert!(
            res_line.paths.iter().all(|p| !p.closed),
            "Open line should yield no closed paths"
        );
    }

    #[test]
    fn test_endpoints_match_chain_ends() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res.paths.len(), 1);
        let path = &res.paths[0];

        let (curve_start, curve_end) = get_endpoints(&path.curve.segments);
        assert_eq!(path.endpoints[0], curve_start);
        assert_eq!(path.endpoints[1], curve_end);
    }

    #[test]
    fn test_trace_stroke_ex_is_deterministic() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 25, 15, 1);
        draw_line(&mut img, 15, 5, 15, 25, 1);

        let res1 = trace_stroke_ex(&img, &StrokeOptions::default());
        let res2 = trace_stroke_ex(&img, &StrokeOptions::default());

        assert_eq!(res1.paths.len(), res2.paths.len());
        assert_eq!(res1.component_count, res2.component_count);

        for (p1, p2) in res1.paths.iter().zip(res2.paths.iter()) {
            assert_eq!(p1.width, p2.width);
            assert_eq!(p1.width_profile, p2.width_profile);
            assert_eq!(p1.component, p2.component);
            assert_eq!(p1.closed, p2.closed);
            assert_eq!(p1.endpoints, p2.endpoints);
        }
    }

    #[test]
    fn test_zero_size_image_does_not_panic() {
        let img = RasterImage {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res.paths.len(), 0);
        assert_eq!(res.component_count, 0);
    }

    #[test]
    fn test_extend_caps_lengthens_open_stroke() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 16, 25, 16, 1);

        let opts_off = StrokeOptions {
            extend_caps: false,
            ..StrokeOptions::default()
        };
        let res_off = trace_stroke_ex(&img, &opts_off);

        let opts_on = StrokeOptions {
            extend_caps: true,
            ..StrokeOptions::default()
        };
        let res_on = trace_stroke_ex(&img, &opts_on);

        assert_eq!(res_off.paths.len(), res_on.paths.len());
        assert!(!res_off.paths.is_empty());

        let ep_off = res_off.paths[0].endpoints;
        let ep_on = res_on.paths[0].endpoints;

        let dist_off = (ep_off[0].0 - ep_off[1].0).hypot(ep_off[0].1 - ep_off[1].1);
        let dist_on = (ep_on[0].0 - ep_on[1].0).hypot(ep_on[0].1 - ep_on[1].1);

        assert!(
            dist_on > dist_off,
            "Endpoints with extend_caps=true ({}) should be further apart than extend_caps=false ({})",
            dist_on,
            dist_off
        );

        let mut img_loop = make_empty_image(32, 32);
        draw_line(&mut img_loop, 10, 10, 20, 10, 0);
        draw_line(&mut img_loop, 20, 10, 20, 20, 0);
        draw_line(&mut img_loop, 20, 20, 10, 20, 0);
        draw_line(&mut img_loop, 10, 20, 10, 10, 0);

        let res_loop_off = trace_stroke_ex(&img_loop, &opts_off);
        let res_loop_on = trace_stroke_ex(&img_loop, &opts_on);

        assert_eq!(res_loop_off.paths.len(), res_loop_on.paths.len());
        for (p_off, p_on) in res_loop_off.paths.iter().zip(res_loop_on.paths.iter()) {
            assert_eq!(p_off.closed, p_on.closed);
        }
    }

    #[test]
    fn test_trace_stroke_delegation_is_output_identical() {
        let tolerance = 0.5;

        // 1. Open line
        let mut img_line = make_empty_image(32, 32);
        draw_line(&mut img_line, 5, 10, 25, 10, 0);

        // 2. Closed loop
        let mut img_loop = make_empty_image(32, 32);
        draw_line(&mut img_loop, 10, 10, 20, 10, 0);
        draw_line(&mut img_loop, 20, 10, 20, 20, 0);
        draw_line(&mut img_loop, 20, 20, 10, 20, 0);
        draw_line(&mut img_loop, 10, 20, 10, 10, 0);

        // 3. Two disjoint strokes
        let mut img_disjoint = make_empty_image(64, 64);
        draw_line(&mut img_disjoint, 5, 5, 15, 5, 0);
        draw_line(&mut img_disjoint, 5, 25, 15, 25, 0);

        let images = vec![img_line, img_loop, img_disjoint];

        for img in images {
            let res = trace_stroke(&img, tolerance);
            let opts = StrokeOptions {
                tolerance,
                ..StrokeOptions::default()
            };
            let res_ex = trace_stroke_ex(&img, &opts);

            assert_eq!(res.path_count, res_ex.paths.len());
            assert_eq!(res.curves.curves.len(), res_ex.paths.len());
            assert_eq!(res.widths.len(), res_ex.paths.len());

            for (i, p) in res_ex.paths.iter().enumerate() {
                assert_eq!(res.widths[i], p.width);
                assert_eq!(res.curves.curves[i].segments.len(), p.curve.segments.len());
                let curve_json = serde_json::to_vec(&res.curves.curves[i]).unwrap();
                let ex_curve_json = serde_json::to_vec(&p.curve).unwrap();
                assert_eq!(curve_json, ex_curve_json);
            }
        }
    }

    #[test]
    fn test_component_widths_measures_stroke_and_blob() {
        let w = 64;
        let h = 64;
        let mut mask = vec![false; (w * h) as usize];

        // 3px-wide bar: x in 5..=7, y in 5..=25
        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
            }
        }

        // 12x12 solid square: x in 30..=41, y in 30..=41
        for y in 30..=41 {
            for x in 30..=41 {
                mask[(y * w + x) as usize] = true;
            }
        }

        let widths = component_widths(&mask, w, h);
        assert_eq!(widths.len(), 2);
        assert_eq!(widths[0].component, 0);
        assert!(
            widths[0].max_full_width < 5.0,
            "Bar max_full_width should be < 5, got {}",
            widths[0].max_full_width
        );
        assert_eq!(widths[1].component, 1);
        assert!(
            widths[1].max_full_width >= 11.0,
            "Square max_full_width should be >= 11, got {}",
            widths[1].max_full_width
        );
    }

    #[test]
    fn test_route_fill_above_diverts_blob() {
        let w = 64;
        let h = 64;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        // 3px-wide bar
        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        // 12x12 solid square
        for y in 30..=41 {
            for x in 30..=41 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(8.0),
            ..StrokeOptions::default()
        };

        let res = trace_stroke_ex(&img, &opts);

        assert_eq!(res.filled_count, 1);
        assert_eq!(res.filled_mask.len(), (w * h) as usize);

        // Check filled_mask is true for square pixels
        assert!(res.filled_mask[(35 * w + 35) as usize]);
        assert!(res.filled_mask[(30 * w + 30) as usize]);
        // Check filled_mask is false for bar pixels and background
        assert!(!res.filled_mask[(15 * w + 6) as usize]);
        assert!(!res.filled_mask[0]);

        // Assert path(s) exist for the bar, but not the square
        assert!(!res.paths.is_empty());
        for p in &res.paths {
            assert!(p.endpoints[0].1 < 30.0);
            assert!(p.endpoints[1].1 < 30.0);
        }
    }

    #[test]
    fn test_route_fill_above_none_is_unchanged() {
        let w = 64;
        let h = 64;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }
        for y in 30..=41 {
            for x in 30..=41 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts_none = StrokeOptions {
            ink: InkSource::Mask(mask.clone()),
            route_fill_above: None,
            ..StrokeOptions::default()
        };

        let opts_max = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(f64::MAX),
            ..StrokeOptions::default()
        };

        let res_none = trace_stroke_ex(&img, &opts_none);
        let res_max = trace_stroke_ex(&img, &opts_max);

        assert!(res_none.filled_mask.is_empty());
        assert_eq!(res_none.filled_count, 0);

        assert!(res_max.filled_mask.is_empty());
        assert_eq!(res_max.filled_count, 0);

        assert_eq!(res_none.paths.len(), res_max.paths.len());
        for (p1, p2) in res_none.paths.iter().zip(res_max.paths.iter()) {
            assert_eq!(p1.width, p2.width);
            assert_eq!(p1.endpoints, p2.endpoints);
        }
    }

    #[test]
    fn test_route_fill_above_all_thin_routes_nothing() {
        let w = 64;
        let h = 64;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(8.0),
            ..StrokeOptions::default()
        };

        let res = trace_stroke_ex(&img, &opts);

        assert_eq!(res.filled_count, 0);
        assert!(res.filled_mask.is_empty());
        assert!(!res.paths.is_empty());
    }

    #[test]
    fn test_route_fill_above_excludes_long_fat_bar() {
        // A 12x60 bar crosses the width threshold (2*maxDT = 12 >= 8) but its
        // area/(pi*maxDT^2) ~ 6.4 fails the compactness cut: it is a thick
        // STROKE, not a filled glyph, and must stay on the skeleton path.
        // This is the exact failure mode seen at 4x supersample, where a
        // junction blob on a stroke outline matches a small glyph's width.
        let w = 80;
        let h = 80;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        for y in 10..=69 {
            for x in 10..=21 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(8.0),
            ..StrokeOptions::default()
        };

        let res = trace_stroke_ex(&img, &opts);

        assert_eq!(res.filled_count, 0, "long fat bar must not route to fill");
        assert!(res.filled_mask.is_empty());
        assert!(!res.paths.is_empty(), "bar must still be stroke-traced");
    }

    #[test]
    fn test_repair_junctions_option_false_by_default() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res_default = trace_stroke_ex(&img, &StrokeOptions::default());
        let res_false = trace_stroke_ex(
            &img,
            &StrokeOptions {
                repair_junctions: false,
                ..StrokeOptions::default()
            },
        );

        assert_eq!(res_default.paths.len(), res_false.paths.len());
        assert_eq!(res_default.component_count, res_false.component_count);
    }
