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
