    use super::*;

    /// Sample points along a circle centred at (cx, cy) with radius r.
    fn sample_circle(cx: f64, cy: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

    /// Sample the perimeter of an axis-aligned rectangle.
    fn sample_rect(x: f64, y: f64, w: f64, h: f64, points_per_side: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x + t * w, y));
        }
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x + w, y + t * h));
        }
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x + (1.0 - t) * w, y + h));
        }
        for i in 0..points_per_side {
            let t = i as f64 / points_per_side as f64;
            pts.push((x, y + (1.0 - t) * h));
        }
        pts
    }

    /// Sample a rounded-rectangle perimeter.
    fn sample_rounded_rect(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
        pts_per_side: usize,
        pts_per_corner: usize,
    ) -> Vec<(f64, f64)> {
        let mut pts = Vec::new();
        // Top edge (left to right), excluding corner arcs
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let px = x + r + t * (w - 2.0 * r);
            pts.push((px, y));
        }
        // Top-right corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + w - r + r * angle.sin();
            let py = y + r - r * angle.cos();
            pts.push((px, py));
        }
        // Right edge (top to bottom)
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let py = y + r + t * (h - 2.0 * r);
            pts.push((x + w, py));
        }
        // Bottom-right corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + w - r + r * angle.cos();
            let py = y + h - r + r * angle.sin();
            pts.push((px, py));
        }
        // Bottom edge (right to left)
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let px = x + w - r - t * (w - 2.0 * r);
            pts.push((px, y + h));
        }
        // Bottom-left corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + r - r * angle.sin();
            let py = y + h - r + r * angle.cos();
            pts.push((px, py));
        }
        // Left edge (bottom to top)
        for i in 0..=pts_per_side {
            let t = i as f64 / pts_per_side as f64;
            let py = y + h - r - t * (h - 2.0 * r);
            pts.push((x, py));
        }
        // Top-left corner arc
        for i in 0..=pts_per_corner {
            let t = i as f64 / pts_per_corner as f64;
            let angle = std::f64::consts::FRAC_PI_2 * t;
            let px = x + r - r * angle.cos();
            let py = y + r - r * angle.sin();
            pts.push((px, py));
        }
        pts
    }

    #[test]
    fn test_recognize_circle() {
        let pts = sample_circle(50.0, 30.0, 20.0, 64);
        let prim = recognize(&pts, 1.0).expect("should recognise a circle");
        match prim {
            Primitive::Circle { cx, cy, r } => {
                assert!((cx - 50.0).abs() < 0.5, "cx off");
                assert!((cy - 30.0).abs() < 0.5, "cy off");
                assert!((r - 20.0).abs() < 0.5, "r off");
            }
            other => panic!("expected Circle, got {other:?}"),
        }
    }

    #[test]
    fn test_recognize_square() {
        let pts = sample_rect(0.0, 0.0, 10.0, 10.0, 10);
        let prim = recognize(&pts, 0.5).expect("should recognise a rect");
        match prim {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
            } => {
                assert!((x - 0.0).abs() < 0.01);
                assert!((y - 0.0).abs() < 0.01);
                assert!((width - 10.0).abs() < 0.01);
                assert!((height - 10.0).abs() < 0.01);
                assert!(rx.is_none(), "sharp rect should have rx=None");
                assert!(ry.is_none(), "sharp rect should have ry=None");
            }
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    #[test]
    fn test_recognize_rounded_square() {
        let corner_r = 2.0;
        let pts = sample_rounded_rect(0.0, 0.0, 10.0, 10.0, corner_r, 10, 8);
        let prim = recognize(&pts, 0.5).expect("should recognise a rounded rect");
        match prim {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
            } => {
                assert!((x - 0.0).abs() < 0.01);
                assert!((y - 0.0).abs() < 0.01);
                assert!((width - 10.0).abs() < 0.01);
                assert!((height - 10.0).abs() < 0.01);
                let actual_r = rx.expect("rx should be Some for rounded rect");
                let actual_ry = ry.expect("ry should be Some for rounded rect");
                assert!(
                    (actual_r - corner_r).abs() < 1.0,
                    "rx {actual_r} not close to {corner_r}"
                );
                assert!(
                    (actual_ry - corner_r).abs() < 1.0,
                    "ry {actual_ry} not close to {corner_r}"
                );
            }
            other => panic!("expected Rect, got {other:?}"),
        }
    }

    #[test]
    fn test_recognize_scribble_returns_none() {
        // Random-ish points that don't form any recognisable shape
        let pts = vec![
            (0.0, 0.0),
            (3.0, 1.0),
            (5.0, 4.0),
            (2.0, 7.0),
            (1.0, 3.0),
            (6.0, 2.0),
            (8.0, 6.0),
            (4.0, 9.0),
            (0.5, 6.0),
        ];
        assert!(recognize(&pts, 1.0).is_none());
    }

    // ── Stable IDs (#7) ────────────────────────────────────────────────

    fn red() -> Option<Rgb> {
        Some(Rgb {
            r: 255,
            g: 128,
            b: 0,
        })
    }

    /// A closed triangle, as a path.
    fn triangle() -> Vec<PathElement> {
        vec![
            PathElement::MoveTo(1.234, 5.678),
            PathElement::LineTo(9.012, 3.456),
            PathElement::LineTo(4.0, 8.0),
            PathElement::ClosePath,
        ]
    }

    #[test]
    fn test_stable_id_deterministic() {
        let path = triangle();
        assert_eq!(
            stable_id(&path, None, red()),
            stable_id(&path, None, red()),
            "deterministic IDs must match"
        );
    }

    #[test]
    fn test_stable_id_floating_point_stability() {
        // Differing only in the 6th decimal place, well below the rounding
        // precision of 3 dp: same ID.
        let a = vec![PathElement::MoveTo(1.234567, 2.345678)];
        let b = vec![PathElement::MoveTo(1.234599, 2.345601)];
        assert_eq!(
            stable_id(&a, None, red()),
            stable_id(&b, None, red()),
            "6th-dp noise should produce the same ID"
        );

        // Differing in the 2nd decimal place, above the rounding precision:
        // different IDs.
        let c = vec![PathElement::MoveTo(1.23, 2.34)];
        let d = vec![PathElement::MoveTo(1.24, 2.35)];
        assert_ne!(
            stable_id(&c, None, red()),
            stable_id(&d, None, red()),
            "2nd-dp difference should produce different IDs"
        );
    }

    #[test]
    fn test_stable_id_negative_zero_is_normalized() {
        let pos = vec![PathElement::MoveTo(0.0, 0.0)];
        let neg = vec![PathElement::MoveTo(-0.0, -0.0)];
        assert_eq!(stable_id(&pos, None, None), stable_id(&neg, None, None));
    }

    #[test]
    fn test_stable_id_without_fill() {
        let id = stable_id(&[PathElement::MoveTo(0.0, 0.0)], None, None);
        assert!(id.starts_with("s-"), "ID should start with s-");
        assert_eq!(id.len(), 10, "s- + 8 hex chars = 10");
    }

    /// The regression that motivated #7: the ID used to include the shape's
    /// index in the paint array, so inserting or reordering an unrelated
    /// earlier shape renamed every later one.
    #[test]
    fn test_stable_id_survives_unrelated_insertion_and_reordering() {
        let subject = triangle();
        let id = stable_id(&subject, None, red());

        let others = [
            vec![PathElement::MoveTo(100.0, 100.0), PathElement::ClosePath],
            vec![PathElement::MoveTo(200.0, 200.0), PathElement::ClosePath],
        ];
        for other in &others {
            assert_ne!(stable_id(other, None, red()), id, "fixture sanity");
        }
        assert_eq!(
            stable_id(&subject, None, red()),
            id,
            "an unchanged shape must keep its ID regardless of what else \
             was inserted or reordered around it"
        );
    }

    /// The other half of #7: endpoints alone were hashed, so two curves
    /// that bow in opposite directions between the same endpoints collided.
    #[test]
    fn test_stable_id_is_sensitive_to_cubic_control_points() {
        let start = PathElement::MoveTo(0.0, 0.0);
        let bows_up = vec![
            start.clone(),
            PathElement::CurveTo(3.0, 10.0, 7.0, 10.0, 10.0, 0.0),
        ];
        let bows_down = vec![
            start.clone(),
            PathElement::CurveTo(3.0, -10.0, 7.0, -10.0, 10.0, 0.0),
        ];
        let straight = vec![start, PathElement::LineTo(10.0, 0.0)];

        let up = stable_id(&bows_up, None, red());
        let down = stable_id(&bows_down, None, red());
        let line = stable_id(&straight, None, red());

        assert_ne!(up, down, "opposite curvature must not share an ID");
        assert_ne!(up, line, "a curve and a line must not share an ID");
        assert_ne!(down, line, "a curve and a line must not share an ID");
    }

    #[test]
    fn test_stable_id_is_sensitive_to_the_primitive() {
        let path = triangle();
        let circle = Primitive::Circle {
            cx: 5.0,
            cy: 5.0,
            r: 3.0,
        };
        let bigger = Primitive::Circle {
            cx: 5.0,
            cy: 5.0,
            r: 4.0,
        };

        let as_path = stable_id(&path, None, red());
        let as_circle = stable_id(&path, Some(&circle), red());
        let as_bigger = stable_id(&path, Some(&bigger), red());

        assert_ne!(
            as_path, as_circle,
            "promoting an outline to a primitive changes what is emitted"
        );
        assert_ne!(
            as_circle, as_bigger,
            "primitive parameters are part of the identity"
        );
    }

    /// `None` and `Some(0.0)` corner radii are different declarations.
    #[test]
    fn test_stable_id_distinguishes_absent_from_zero_radius() {
        let path = triangle();
        let square = Primitive::Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            rx: None,
            ry: None,
        };
        let zero_radius = Primitive::Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            rx: Some(0.0),
            ry: Some(0.0),
        };
        assert_ne!(
            stable_id(&path, Some(&square), red()),
            stable_id(&path, Some(&zero_radius), red())
        );
    }

    #[test]
    fn test_stable_id_is_sensitive_to_fill() {
        let path = triangle();
        let blue = Some(Rgb { r: 0, g: 0, b: 255 });
        assert_ne!(stable_id(&path, None, red()), stable_id(&path, None, blue));
        assert_ne!(stable_id(&path, None, red()), stable_id(&path, None, None));
    }

    /// The coordinate encoding must be prefix-free, or `(1.0, 23.0)` and
    /// `(12.0, 3.0)` would hash to the same bytes.
    #[test]
    fn test_stable_id_coordinates_are_not_ambiguous() {
        let a = vec![PathElement::MoveTo(1.0, 23.0)];
        let b = vec![PathElement::MoveTo(12.0, 3.0)];
        assert_ne!(stable_id(&a, None, None), stable_id(&b, None, None));
    }

    /// Duplicate identical shapes hash the same by design; collision
    /// handling is `dedupe_ids`, which suffixes only the duplicates and
    /// leaves the first occurrence — and every unrelated ID — untouched.
    #[test]
    fn test_identical_shapes_collide_and_are_deduped_stably() {
        let path = triangle();
        let dup = stable_id(&path, None, red());
        let other = stable_id(
            &[PathElement::MoveTo(50.0, 50.0), PathElement::ClosePath],
            None,
            red(),
        );

        let mut two = vec![dup.clone(), other.clone(), dup.clone()];
        dedupe_ids(&mut two);
        assert_eq!(two, vec![dup.clone(), other.clone(), format!("{dup}-2")]);

        // Adding a third copy must not rename the first two.
        let mut three = vec![dup.clone(), other.clone(), dup.clone(), dup.clone()];
        dedupe_ids(&mut three);
        assert_eq!(three[0], two[0], "first copy keeps its ID");
        assert_eq!(three[1], two[1], "unrelated shape keeps its ID");
        assert_eq!(three[2], two[2], "second copy keeps its suffix");
        assert_eq!(three[3], format!("{dup}-3"));
    }

    #[test]
    fn test_dedupe_ids() {
        let mut ids = vec![
            "s-aaa".to_string(),
            "s-aaa".to_string(),
            "s-bbb".to_string(),
            "s-aaa".to_string(),
        ];
        dedupe_ids(&mut ids);
        assert_eq!(ids[0], "s-aaa");
        assert_eq!(ids[1], "s-aaa-2");
        assert_eq!(ids[2], "s-bbb");
        assert_eq!(ids[3], "s-aaa-3");
    }

    #[test]
    fn test_dedupe_ids_no_duplicates() {
        let mut ids = vec![
            "s-aaa".to_string(),
            "s-bbb".to_string(),
            "s-ccc".to_string(),
        ];
        let original = ids.clone();
        dedupe_ids(&mut ids);
        assert_eq!(ids, original);
    }

    #[test]
    fn test_dedupe_ids_empty() {
        let mut ids: Vec<String> = vec![];
        dedupe_ids(&mut ids);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_recognize_few_points() {
        // < 3 points should always return None
        assert!(recognize(&[], 1.0).is_none());
        assert!(recognize(&[(0.0, 0.0)], 1.0).is_none());
        assert!(recognize(&[(0.0, 0.0), (1.0, 0.0)], 1.0).is_none());
    }

    #[test]
    fn test_stable_id_prefix_format() {
        let fill = Some(Rgb { r: 0, g: 0, b: 0 });
        let id = stable_id(&[PathElement::MoveTo(0.0, 0.0)], None, fill);
        assert_eq!(id.len(), 10, "expected s-xxxxxxxx (10 chars)");
        assert!(id.starts_with("s-"), "expected s- prefix");
        // The hex part should be valid hex
        let hex_part = &id[2..];
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_dedupe_ids_consecutive_duplicates() {
        let mut ids = vec!["s-x".to_string(), "s-x".to_string(), "s-x".to_string()];
        dedupe_ids(&mut ids);
        assert_eq!(ids[0], "s-x");
        assert_eq!(ids[1], "s-x-2");
        assert_eq!(ids[2], "s-x-3");
    }
