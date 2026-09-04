    use super::*;

    fn generate_synthetic_sheet_png() -> Vec<u8> {
        let w = 200u32;
        let h = 140u32;
        let mut imgbuf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
            w,
            h,
            image::Rgba([255, 255, 255, 255]),
        );

        let draw_rect = |img: &mut image::ImageBuffer<image::Rgba<u8>, _>,
                         x1: u32,
                         y1: u32,
                         x2: u32,
                         y2: u32,
                         color: image::Rgba<u8>| {
            for y in y1..=y2 {
                for x in x1..=x2 {
                    if x < w && y < h {
                        img.put_pixel(x, y, color);
                    }
                }
            }
        };

        let blue = image::Rgba([0, 0, 200, 255]);
        let red = image::Rgba([200, 0, 0, 255]);

        // Cell (0,0): L shape (Blue)
        draw_rect(&mut imgbuf, 24, 24, 31, 48, blue);
        draw_rect(&mut imgbuf, 24, 41, 48, 48, blue);

        // Cell (1,0): Horizontal bar (Red)
        draw_rect(&mut imgbuf, 84, 32, 108, 40, red);

        let mut bytes: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        imgbuf
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn test_run_sheet_pipeline_in_memory() {
        let bytes = generate_synthetic_sheet_png();
        let opts = ConvertOptions::default();
        let cfg = PipelineOptions::default();

        let outcome1 = run_sheet_pipeline(&bytes, &opts, &cfg).expect("pipeline run 1");
        assert_eq!(outcome1.report.written, outcome1.icons.len());
        assert!(!outcome1.icons.is_empty());
        for icon in &outcome1.icons {
            assert!(
                icon.svg.starts_with("<svg"),
                "SVG should start with <svg, got: {}",
                &icon.svg[..icon.svg.len().min(20)]
            );
        }

        let outcome2 = run_sheet_pipeline(&bytes, &opts, &cfg).expect("pipeline run 2");
        assert_eq!(outcome1.icons.len(), outcome2.icons.len());
        for (i1, i2) in outcome1.icons.iter().zip(outcome2.icons.iter()) {
            assert_eq!(i1.name, i2.name);
            assert_eq!(i1.svg, i2.svg);
        }
    }

    fn generate_synthetic_chroma_sheet_with_labels_png() -> Vec<u8> {
        let w = 200u32;
        let h = 150u32;
        let mut imgbuf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
            w,
            h,
            image::Rgba([255, 255, 255, 255]),
        );

        let draw_rect = |img: &mut image::ImageBuffer<image::Rgba<u8>, _>,
                         x1: u32,
                         y1: u32,
                         x2: u32,
                         y2: u32,
                         color: image::Rgba<u8>| {
            for y in y1..=y2 {
                for x in x1..=x2 {
                    if x < w && y < h {
                        img.put_pixel(x, y, color);
                    }
                }
            }
        };

        let blue = image::Rgba([0, 0, 200, 255]);
        let red = image::Rgba([200, 0, 0, 255]);
        let black = image::Rgba([0, 0, 0, 255]);

        // Cell (0,0): L shape (Blue)
        draw_rect(&mut imgbuf, 24, 24, 31, 48, blue);
        draw_rect(&mut imgbuf, 24, 41, 48, 48, blue);

        // Cell (1,0): L shape (Red)
        draw_rect(&mut imgbuf, 84, 24, 91, 48, red);
        draw_rect(&mut imgbuf, 84, 41, 108, 48, red);

        // Row 1 text label (Black)
        draw_rect(&mut imgbuf, 15, 58, 55, 68, black);
        draw_rect(&mut imgbuf, 75, 58, 115, 68, black);

        // Cell (0,1): L shape (Blue)
        draw_rect(&mut imgbuf, 24, 84, 31, 108, blue);
        draw_rect(&mut imgbuf, 24, 101, 48, 108, blue);

        // Cell (1,1): L shape (Red)
        draw_rect(&mut imgbuf, 84, 84, 91, 108, red);
        draw_rect(&mut imgbuf, 84, 101, 108, 108, red);

        // Row 2 text label (Black)
        draw_rect(&mut imgbuf, 15, 118, 55, 128, black);
        draw_rect(&mut imgbuf, 75, 118, 115, 128, black);

        let mut bytes: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut bytes);
        imgbuf
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn test_chroma_mode_populates_label_rect() {
        let bytes = generate_synthetic_chroma_sheet_with_labels_png();
        let opts = ConvertOptions::default();
        let cfg = PipelineOptions {
            seg: SegChoice::Chroma,
            sat_threshold: Some(90),
            ..Default::default()
        };

        let outcome = run_sheet_pipeline(&bytes, &opts, &cfg).expect("pipeline run in chroma mode");
        assert_eq!(outcome.report.segmentation, "chroma");
        assert_eq!(outcome.report.text_rows_removed, 0);
        assert!(
            outcome
                .report
                .icons
                .iter()
                .any(|icon| icon.label_rect.is_some()),
            "Expected at least one icon to have label_rect populated in chroma mode"
        );
    }
