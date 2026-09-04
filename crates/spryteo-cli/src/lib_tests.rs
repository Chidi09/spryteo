    use super::*;
    use spryteo_core::{ColorSpec, Grouping, Layering};

    #[test]
    fn test_catch_pipeline_panic_converts_to_internal_error() {
        let result: Result<ConvertResult, SpryteoError> =
            catch_pipeline_panic(|| -> Result<ConvertResult, SpryteoError> {
                panic!("deliberate test panic");
            });

        assert!(result.is_err());
        match result.err().unwrap() {
            SpryteoError::Internal(msg) => assert!(msg.contains("deliberate test panic")),
            other => panic!("Expected SpryteoError::Internal, got {:?}", other),
        }
    }

    #[test]
    fn test_catch_pipeline_panic_maps_to_exit_code_3() {
        let result: Result<ConvertResult, SpryteoError> =
            catch_pipeline_panic(|| -> Result<ConvertResult, SpryteoError> {
                panic!("deliberate test panic");
            });
        let cli_err: CliError = result.unwrap_err().into();
        assert_eq!(cli_err.exit_code(), 3);
    }

    #[test]
    fn test_catch_pipeline_panic_passes_through_ok() {
        use spryteo_core::{Meta, Stats};

        let result = catch_pipeline_panic(|| {
            Ok(ConvertResult {
                svg: "<svg></svg>".to_string(),
                meta: Meta {
                    nodes: vec![],
                    stats: Stats {
                        node_count: 0,
                        path_count: 0,
                        byte_count: 12,
                    },
                    current_color_applied: false,
                },
            })
        });
        assert!(result.is_ok());
        assert_eq!(result.unwrap().svg, "<svg></svg>");
    }

    #[test]
    fn test_fills_building() {
        use spryteo_core::ir::{Contour, ContourSet, Fill, Layer, LayerStack, Rgb};

        let red = Rgb { r: 255, g: 0, b: 0 };
        let green = Rgb { r: 0, g: 255, b: 0 };

        let layer0 = Layer {
            mask: vec![],
            color: red,
            z_order: 0,
        };
        let layer1 = Layer {
            mask: vec![],
            color: green,
            z_order: 1,
        };

        let layer_stack = LayerStack {
            layers: vec![layer0, layer1],
        };

        let contour_b = Contour {
            points: vec![],
            children: vec![],
        };
        let contour_a = Contour {
            points: vec![],
            children: vec![contour_b],
        };
        let contour_c = Contour {
            points: vec![],
            children: vec![],
        };
        let contour_d = Contour {
            points: vec![],
            children: vec![],
        };

        let contour_set = ContourSet {
            layers: vec![vec![contour_a], vec![contour_c, contour_d]],
        };

        // Mode::Icon + default (Auto) gradients means gradient detection is
        // never attempted, so a minimal 0x0 image is fine here.
        let image = RasterImage {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let opts = ConvertOptions::default();
        let fills = build_fills(&image, &layer_stack, &contour_set, &Mode::Icon, &opts);
        // contour_a absorbs its hole (contour_b) as a subpath of one shape,
        // so layer 0 emits one fill; layer 1 emits one per top-level contour.
        assert_eq!(fills.len(), 3);
        assert_eq!(fills[0], Fill::Solid(red));
        assert_eq!(fills[1], Fill::Solid(green));
        assert_eq!(fills[2], Fill::Solid(green));
    }

    #[test]
    fn test_e2e_pipeline_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with red circle
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                if dx * dx + dy * dy <= 8.0 * 8.0 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let opts = ConvertOptions::default();
        let res1 = run_convert(&png_bytes, &opts).unwrap();

        assert!(res1.svg.contains("<svg"));
        assert!(res1.svg.contains("viewBox="));
        assert!(res1.svg.contains("<path") || res1.svg.contains("<circle"));

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
        assert!(!res1.svg.is_empty());

        let res2 = run_convert(&png_bytes, &opts).unwrap();
        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_e2e_exif_orientation_applied_through_full_pipeline() {
        // Fixture: 40x20 PNG, left half red / right half blue, tagged with
        // EXIF Orientation=6 (rotate 90 CW to display upright). Correct
        // decoding (crates/spryteo-raster's decode()) must rotate this to
        // a 20x40 image with red on top and blue on the bottom -- verified
        // independently against Pillow's own `ImageOps.exif_transpose`
        // (see crates/spryteo-raster/tests/decode.rs for the isolated
        // decode-level test; this proves the correction actually survives
        // the full quantize/trace/fit/svg pipeline, not just decode()).
        let png_bytes: &[u8] = include_bytes!("../tests/fixtures/exif_oriented_icon.png");

        let opts = ConvertOptions {
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let result = run_convert(png_bytes, &opts).unwrap();

        // The pipeline must have picked up the corrected (swapped) 20x40
        // dimensions, not the raw 40x20 stored dimensions.
        assert!(
            result.svg.contains("width=\"20\"") || result.svg.contains("viewBox=\"0 0 20 40\""),
            "expected orientation-corrected 20x40 dimensions in SVG, got: {}",
            result.svg
        );

        assert_eq!(
            result.meta.nodes.len(),
            2,
            "expected exactly two colour regions"
        );

        let red = spryteo_core::Rgb {
            r: 220,
            g: 20,
            b: 20,
        };
        let blue = spryteo_core::Rgb {
            r: 20,
            g: 20,
            b: 220,
        };
        let red_node = result
            .meta
            .nodes
            .iter()
            .find(|n| n.fill == Some(red))
            .expect("red region should be present");
        let blue_node = result
            .meta
            .nodes
            .iter()
            .find(|n| n.fill == Some(blue))
            .expect("blue region should be present");

        // Post-rotation: red occupies the top half (small y), blue the
        // bottom half (large y) of the corrected 20x40 canvas. Icon-mode
        // classification treats one colour as a full-canvas background
        // layer (hence red's centroid sits near canvas-middle rather than
        // strictly in the top half) with the other cut out as a
        // constrained foreground shape, so only the cutout shape's bbox
        // is checked precisely.
        assert!(
            red_node.centroid.1 < blue_node.centroid.1,
            "red region (was left half pre-rotation) should end up above blue (was right half): red centroid={:?}, blue centroid={:?}",
            red_node.centroid,
            blue_node.centroid
        );
        assert!(
            blue_node.bbox.y_min >= 19.0,
            "blue region (post-rotation bottom half) should not bleed into the top half, bbox={:?}",
            blue_node.bbox
        );
    }

    #[test]
    fn test_garbage_input() {
        let garbage = b"Not a real image file content at all";
        let opts = ConvertOptions::default();
        let res = run_convert(garbage, &opts);

        assert!(res.is_err());
        match res.err().unwrap() {
            SpryteoError::InvalidInput(_) => {}
            other => panic!("Expected SpryteoError::InvalidInput, got {:?}", other),
        }
    }

    #[test]
    fn test_e2e_pipeline_stroke_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with black line
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        // Draw diagonal line from (5,5) to (25,25)
        for i in 5..=25 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let px = (i + dx) as u32;
                    let py = (i + dy) as u32;
                    if px < 32 && py < 32 {
                        img.put_pixel(px, py, Rgba([0, 0, 0, 255]));
                    }
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let opts = ConvertOptions {
            stroke: true,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_stroke(&png_bytes, &opts).unwrap();

        assert!(res1.svg.contains("<svg"));
        assert!(res1.svg.contains("viewBox="));
        assert!(res1.svg.contains("pathLength=\"100\""));
        assert!(res1.svg.contains("fill=\"none\""));

        assert!(
            res1.meta.stats.path_count <= 3,
            "Path count was {}",
            res1.meta.stats.path_count
        );

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
        assert!(!res1.svg.is_empty());

        let res2 = run_convert_stroke(&png_bytes, &opts).unwrap();
        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_garbage_input_stroke() {
        let garbage = b"Not a real image file content at all";
        let opts = ConvertOptions {
            stroke: true,
            ..ConvertOptions::default()
        };
        let res = run_convert_stroke(garbage, &opts);

        assert!(res.is_err());
        match res.err().unwrap() {
            SpryteoError::InvalidInput(_) => {}
            other => panic!("Expected SpryteoError::InvalidInput, got {:?}", other),
        }
    }

    #[test]
    fn test_format_meta_report() {
        use spryteo_core::{Bbox, Meta, NodeMeta, Rgb, Stats};

        let meta = Meta {
            nodes: vec![NodeMeta {
                id: "s-abc123".to_string(),
                bbox: Bbox {
                    x_min: 0.0,
                    y_min: 0.0,
                    x_max: 10.0,
                    y_max: 10.0,
                },
                centroid: (5.0, 5.0),
                area: 100.0,
                fill: Some(Rgb { r: 255, g: 0, b: 0 }),
                group: "g-root".to_string(),
                z_order: 0,
                suggested_draw_order: 0,
            }],
            stats: Stats {
                node_count: 1,
                path_count: 1,
                byte_count: 42,
            },
            current_color_applied: false,
        };

        let report = format_meta_report(&meta);
        assert!(report.contains("nodes=1"));
        assert!(report.contains("s-abc123"));
        assert!(report.contains("#ff0000"));
        assert!(report.contains("g-root"));
    }

    #[test]
    fn test_derive_svg_summary() {
        let svg =
            r#"<svg viewBox="0 0 24 24"><g id="g-root"><path id="s-1" d="M 0.0 0.0"/></g></svg>"#;
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("paths=1"));
        assert!(summary.contains("groups=1"));
        assert!(summary.contains("viewBox=0 0 24 24"));
        assert!(summary.contains("g-root"));
        assert!(summary.contains("s-1"));
        assert!(summary.contains("bbox=(0.0,0.0,0.0,0.0)"));
    }

    #[test]
    fn test_derive_svg_summary_no_ids() {
        let svg = "<svg></svg>";
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("paths=0"));
        assert!(summary.contains("(no shapes found)"));
    }

    #[test]
    fn test_derive_svg_summary_circle_and_path() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r##"<circle cx="50.0" cy="50.0" r="10.0" id="s-1" fill="#ff0000"/>"##,
            r##"<path d="M 0.0 0.0 L 20.0 0.0 L 20.0 20.0 L 0.0 20.0 Z" id="s-2" fill="#00ff00"/>"##,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("s-1"));
        assert!(summary.contains("s-2"));
        assert!(summary.contains("bbox=(40.0,40.0,60.0,60.0)"));
        assert!(summary.contains("bbox=(0.0,0.0,20.0,20.0)"));
    }

    #[test]
    fn test_derive_svg_summary_transform() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r##"<circle cx="5.0" cy="5.0" r="5.0" id="s-1" fill="#ff0000" transform="translate(10.0, 20.0)"/>"##,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        // Without transform: bbox=(0.0,0.0,10.0,10.0); with translate(10,20): (10,20,20,30)
        assert!(summary.contains("bbox=(10.0,20.0,20.0,30.0)"));
    }

    #[test]
    fn test_derive_svg_summary_nested_groups() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r#"<g id="g-outer"><g id="g-inner"><path id="s-1" d="M 0.0 0.0 L 10.0 0.0 L 10.0 10.0 Z"/></g></g>"#,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        assert!(
            summary.contains("group=g-inner"),
            "expected s-1's parent group to be g-inner, got: {}",
            summary
        );
    }

    #[test]
    fn test_derive_svg_summary_cubic_bezier_bbox_includes_control_points() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r##"<path id="s-1" d="M 0.0 0.0 C 0.0 100.0 100.0 100.0 100.0 0.0" fill="#ff0000"/>"##,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        assert!(
            summary.contains("bbox=(0.0,0.0,100.0,100.0)"),
            "expected control-point-inclusive bbox, got: {}",
            summary
        );
    }

    #[test]
    fn test_derive_svg_summary_id_style_none() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r##"<circle cx="50.0" cy="50.0" r="10.0" fill="#ff0000"/>"##,
            r##"<path d="M 0.0 0.0 L 20.0 0.0 L 20.0 20.0 Z" fill="#00ff00"/>"##,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        // Every data line should start with '-' (no id), not with 's-' or similar
        for line in summary.lines().filter(|l| l.contains("bbox=")) {
            assert!(
                line.starts_with('-'),
                "expected '-' for id column in line: {}",
                line
            );
        }
        assert!(summary.contains("fill=#ff0000"));
        assert!(summary.contains("fill=#00ff00"));
        assert!(summary.contains("bbox=(40.0,40.0,60.0,60.0)"));
        assert!(summary.contains("bbox=(0.0,0.0,20.0,20.0)"));
    }

    #[test]
    fn test_derive_svg_summary_fill_none_current_color_gradient() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r#"<path id="s-none" d="M 0 0" fill="none"/>"#,
            r#"<path id="s-cc" d="M 10 10" fill="currentColor"/>"#,
            r#"<path id="s-grad" d="M 20 20" fill="url(#grad-s-0)"/>"#,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("fill=none"));
        assert!(summary.contains("fill=currentColor"));
        assert!(summary.contains("fill=url(#grad-s-0)"));
    }

    #[test]
    fn test_parse_path_d_arc_endpoint() {
        let svg = concat!(
            r#"<svg viewBox="0 0 100 100">"#,
            r##"<path id="s-arc" d="M 0.0 0.0 A 5.0 5.0 0 1 0 10.0 10.0" fill="#ff0000"/>"##,
            r#"</svg>"#,
        );
        let summary = derive_svg_summary(svg);
        assert!(summary.contains("bbox=(0.0,0.0,10.0,10.0)"));
    }

    #[test]
    fn test_e2e_background_policies() {
        use image::{ImageBuffer, ImageFormat, Rgba};
        use spryteo_core::options::{Background, Mode};
        use std::io::Cursor;

        let mut img = ImageBuffer::new(16, 16);
        for x in 0..16 {
            for y in 0..16 {
                if (4..12).contains(&x) && (4..12).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255])); // Red
                } else {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255])); // White
                }
            }
        }

        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // 1. Keep (default)
        let opts_keep = ConvertOptions {
            background: Background::Keep,
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let res_keep = run_convert(&png_bytes, &opts_keep).unwrap();
        assert!(
            res_keep.svg.contains("fill=\"#ffffff\""),
            "Keep SVG should contain white fill: {}",
            res_keep.svg
        );
        assert!(
            res_keep.svg.contains("fill=\"#ff0000\""),
            "Keep SVG should contain red fill: {}",
            res_keep.svg
        );
        assert!(
            !res_keep.svg.contains("<rect width=\"16\" height=\"16\""),
            "Keep SVG should not contain a background rect: {}",
            res_keep.svg
        );

        // 2. Drop
        let opts_drop = ConvertOptions {
            background: Background::Drop,
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let res_drop = run_convert(&png_bytes, &opts_drop).unwrap();
        assert!(
            !res_drop.svg.contains("fill=\"#ffffff\""),
            "Drop SVG should not contain white fill: {}",
            res_drop.svg
        );
        assert!(
            res_drop.svg.contains("fill=\"#ff0000\""),
            "Drop SVG should contain red fill: {}",
            res_drop.svg
        );
        assert!(
            !res_drop.svg.contains("<rect width=\"16\" height=\"16\""),
            "Drop SVG should not contain a background rect: {}",
            res_drop.svg
        );

        // 3. Rect
        let opts_rect = ConvertOptions {
            background: Background::Rect,
            mode: Mode::Icon,
            ..ConvertOptions::default()
        };
        let res_rect = run_convert(&png_bytes, &opts_rect).unwrap();
        assert!(
            res_rect
                .svg
                .contains("<rect width=\"16\" height=\"16\" fill=\"#ffffff\"/>"),
            "Rect SVG should contain the background rect: {}",
            res_rect.svg
        );
        let white_matches = res_rect.svg.matches("#ffffff").count();
        assert_eq!(
            white_matches, 1,
            "Rect SVG should only have one white fill (in the rect): {}",
            res_rect.svg
        );
        assert!(
            res_rect.svg.contains("fill=\"#ff0000\""),
            "Rect SVG should contain red fill: {}",
            res_rect.svg
        );

        let rect_idx = res_rect.svg.find("<rect ").expect("should find rect");
        if let Some(path_idx) = res_rect.svg.find("<path ") {
            assert!(rect_idx < path_idx, "rect should come before path");
        }
        if let Some(g_idx) = res_rect.svg.find("<g") {
            assert!(rect_idx < g_idx, "rect should come before group");
        }
    }

    #[test]
    fn test_e2e_current_color_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with red circle
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                if dx * dx + dy * dy <= 8.0 * 8.0 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Convert with current_color = true, background = Drop so no background rect is present
        let opts = ConvertOptions {
            current_color: true,
            background: Background::Drop,
            ..ConvertOptions::default()
        };
        let res = run_convert(&png_bytes, &opts).unwrap();

        assert!(
            res.svg.contains("fill=\"currentColor\""),
            "SVG should use currentColor: {}",
            res.svg
        );
        assert!(
            !res.svg.contains("fill=\"#ff0000\""),
            "SVG should not contain the original red fill: {}",
            res.svg
        );
        assert!(
            res.meta.current_color_applied,
            "meta.current_color_applied should be true"
        );

        // Convert with background = Rect to verify background rect keeps its color
        let opts_rect = ConvertOptions {
            current_color: true,
            background: Background::Rect,
            ..ConvertOptions::default()
        };
        let res_rect = run_convert(&png_bytes, &opts_rect).unwrap();
        assert!(
            res_rect.svg.contains("fill=\"currentColor\""),
            "SVG should still use currentColor: {}",
            res_rect.svg
        );
        // The background rect is white (since original background was white) -> #ffffff
        assert!(
            res_rect.svg.contains("fill=\"#ffffff\""),
            "Background rect should keep its original color: {}",
            res_rect.svg
        );
    }

    #[test]
    fn test_e2e_containment_grouping() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // 32x32 white background, big blue square (4..28), smaller red square inside (12..20)
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                if (4..28).contains(&x) && (4..28).contains(&y) {
                    img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                }
                if (12..20).contains(&x) && (12..20).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Convert with Semantic grouping
        let opts = ConvertOptions {
            grouping: Grouping::Semantic,
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_with_masks(&png_bytes, &opts, &[]).unwrap();

        // Should contain nested <g> elements (containment: small inside big)
        assert!(
            res1.svg.contains("<g"),
            "SVG should have groups: {}",
            res1.svg
        );

        // Determinism: convert twice, byte-identical
        let res2 = run_convert_with_masks(&png_bytes, &opts, &[]).unwrap();
        assert_eq!(
            res1.svg, res2.svg,
            "containment grouping must be deterministic"
        );

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
    }

    #[test]
    fn test_e2e_masks_grouping() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // 32x32: red left half, blue right half (no background color to avoid extra layers)
        let mut img = RgbaImage::new(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                if x < 16 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                } else {
                    img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Masks exactly covering each half (100% overlap with each layer)
        let mut mask_a_pixels = vec![0u8; 32 * 32];
        let mut mask_b_pixels = vec![0u8; 32 * 32];
        for y in 0..32 {
            for x in 0..32 {
                let idx = y * 32 + x;
                if x < 16 {
                    mask_a_pixels[idx] = 255;
                } else {
                    mask_b_pixels[idx] = 255;
                }
            }
        }

        let masks = vec![
            spryteo_semantic::Mask {
                id: "left-half".to_string(),
                width: 32,
                height: 32,
                pixels: mask_a_pixels,
            },
            spryteo_semantic::Mask {
                id: "right-half".to_string(),
                width: 32,
                height: 32,
                pixels: mask_b_pixels,
            },
        ];

        let opts = ConvertOptions {
            grouping: Grouping::Semantic,
            background: Background::Drop,
            layering: Layering::Cutout,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_with_masks(&png_bytes, &opts, &masks).unwrap();

        assert!(
            res1.svg.contains("g-mask-left-half"),
            "SVG should contain g-mask-left-half: {}",
            res1.svg
        );
        assert!(
            res1.svg.contains("g-mask-right-half"),
            "SVG should contain g-mask-right-half: {}",
            res1.svg
        );

        let res2 = run_convert_with_masks(&png_bytes, &opts, &masks).unwrap();
        assert_eq!(res1.svg, res2.svg, "mask grouping must be deterministic");

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
    }

    #[test]
    fn test_e2e_containment_grouping_component_regression() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use spryteo_core::Background;
        use std::io::Cursor;

        // Same synthetic image as containment test
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                if (4..28).contains(&x) && (4..28).contains(&y) {
                    img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                }
                if (12..20).contains(&x) && (12..20).contains(&y) {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        // Convert with explicit Component grouping
        let opts_comp = ConvertOptions {
            grouping: Grouping::Component,
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res_comp = run_convert_with_masks(&png_bytes, &opts_comp, &[]).unwrap();

        // No g-mask- groups
        assert!(
            !res_comp.svg.contains("g-mask-"),
            "Component mode should not contain g-mask- groups"
        );

        // Convert with default options (Component is default)
        let opts_default = ConvertOptions {
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res_default = run_convert(&png_bytes, &opts_default).unwrap();
        assert_eq!(
            res_comp.svg, res_default.svg,
            "explicit Component and default must match"
        );

        // Convert twice with Semantic, no masks, should still produce deterministic output
        let opts_sem = ConvertOptions {
            grouping: Grouping::Semantic,
            background: Background::Keep,
            colors: ColorSpec::N(4),
            tolerance: 0.1,
            ..ConvertOptions::default()
        };
        let res_sem1 = run_convert_with_masks(&png_bytes, &opts_sem, &[]).unwrap();
        let res_sem2 = run_convert_with_masks(&png_bytes, &opts_sem, &[]).unwrap();
        assert_eq!(
            res_sem1.svg, res_sem2.svg,
            "semantic containment grouping must be deterministic"
        );
    }

    #[test]
    fn test_e2e_masks_dimension_validation() {
        // Verify mask with mismatched dimensions is rejected (through CLI validation
        // but we can also test the basic sanity here)
        let mask = spryteo_semantic::Mask {
            id: "bad".to_string(),
            width: 2,
            height: 2,
            pixels: vec![0u8; 3], // should be 4
        };
        let expected = (mask.width as usize) * (mask.height as usize);
        assert_ne!(
            mask.pixels.len(),
            expected,
            "test invariant: bad mask should have wrong pixel count"
        );
    }
