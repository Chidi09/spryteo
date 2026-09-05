    use super::*;
    use base64::{prelude::BASE64_STANDARD, Engine};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    fn get_synthetic_png_b64(stroke: bool) -> String {
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        if stroke {
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
        } else {
            for y in 0..32 {
                for x in 0..32 {
                    let dx = x as f32 - 15.5;
                    let dy = y as f32 - 15.5;
                    if dx * dx + dy * dy <= 8.0 * 8.0 {
                        img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                    }
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();
        BASE64_STANDARD.encode(&png_bytes)
    }

    #[test]
    fn test_convert_image_valid_base64() {
        let b64 = get_synthetic_png_b64(false);
        let res = convert_image_inner(&b64, None).unwrap();
        assert!(res.svg.contains("<svg"));
        assert!(!res.svg.contains("pathLength="));
    }

    #[test]
    fn test_convert_image_invalid_base64() {
        let res = convert_image_inner("not a valid base64 string!!!", None);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("Failed to decode base64"));
    }

    #[test]
    fn test_convert_image_stroke_routing() {
        let b64 = get_synthetic_png_b64(true);
        let opts = Some(serde_json::json!({
            "stroke": true
        }));
        let res = convert_image_inner(&b64, opts).unwrap();
        assert!(res.svg.contains("pathLength="));
    }

    #[test]
    fn test_inspect_svg_valid() {
        let b64 = get_synthetic_png_b64(false);
        let res = convert_image_inner(&b64, None).unwrap();
        let meta_json = serde_json::to_string(&res.meta).unwrap();

        let summary = inspect_svg_inner(&meta_json).unwrap();
        assert!(summary.contains("Spryteo SVG Inspection Summary"));
        assert!(summary.contains(&format!("Total Nodes**: {}", res.meta.stats.node_count)));
        assert!(summary.contains("| Node ID |"));
    }

    #[test]
    fn test_inspect_svg_malformed_json() {
        let res = inspect_svg_inner("not valid json");
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("Failed to parse metadata JSON"));
    }

    /// A red disc, a dark blue disc inside it and a yellow core inside that,
    /// on a white ground: one object built from several colour layers.
    ///
    /// Grouping is what makes such an object animatable as an object rather
    /// than as a handful of unrelated quantization fragments, and #11 asks
    /// for it on every surface, so each surface checks it on this same
    /// fixture.
    const MULTICOLOR_OBJECT: &[u8] = include_bytes!("../../../testdata/multicolor_object_64.png");

    fn assert_multicolor_object_grouped(svg: &str, meta: &spryteo_core::ir::Meta) {
        // The ground stands on its own; the object is a single three-deep
        // tree rather than three siblings.
        assert_eq!(
            meta.groups.len(),
            2,
            "expected the ground and one object, got {:?}",
            meta.groups.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        assert_eq!(meta.groups[0].depth(), 1, "the ground adopts nothing");
        assert_eq!(meta.groups[1].depth(), 3, "the object nests three deep");

        // And the SVG carries the same shape the metadata describes.
        assert!(
            svg.contains("</g></g></g>"),
            "SVG should close a three-deep group chain: {svg}"
        );
    }

    #[test]
    fn a_multicolor_object_comes_back_as_one_nested_group_tree() {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(MULTICOLOR_OBJECT);
        let res = convert_image_inner(&b64, None).unwrap();
        assert_multicolor_object_grouped(&res.svg, &res.meta);
    }
