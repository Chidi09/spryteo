    use super::*;
    use spryteo_core::ir::Primitive;

    fn sample_circle(cx: f64, cy: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

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

    fn assert_finite_elements(elements: &[PathElement]) {
        for elem in elements {
            match elem {
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
    fn test_fit_circle_end_to_end() {
        let pts = sample_circle(50.0, 30.0, 20.0, 64);
        let contour = Contour {
            points: pts,
            children: vec![],
        };
        let set = ContourSet {
            layers: vec![vec![contour]],
        };

        // tolerance = 1.0, smoothness = 1.0
        let curve_set = fit_contours(&set, 1.0, 1.0);
        assert_eq!(curve_set.curves.len(), 1);
        let curve = &curve_set.curves[0];

        // Resulting segments should be significantly fewer than 64
        assert!(
            curve.segments.len() < 10,
            "too many segments: {}",
            curve.segments.len()
        );
        assert_finite_elements(&curve.segments);

        // Primitive recognized should be Some(Primitive::Circle)
        match curve.primitive {
            Some(Primitive::Circle { cx, cy, r }) => {
                assert!((cx - 50.0).abs() < 1.0);
                assert!((cy - 30.0).abs() < 1.0);
                assert!((r - 20.0).abs() < 1.0);
            }
            ref other => panic!("expected Circle primitive, got {:?}", other),
        }
    }

    #[test]
    fn test_fit_square_tight_smoothness() {
        // 4 corners, straight edges
        let pts = sample_rect(0.0, 0.0, 10.0, 10.0, 10);
        let contour = Contour {
            points: pts,
            children: vec![],
        };
        let set = ContourSet {
            layers: vec![vec![contour]],
        };

        // Tight smoothness (0.1) so corners are preserved
        let curve_set = fit_contours(&set, 0.5, 0.1);
        assert_eq!(curve_set.curves.len(), 1);
        let curve = &curve_set.curves[0];

        // Should use LineTo for straight edges and have no CurveTo elements
        let mut line_to_count = 0;
        let mut curve_to_count = 0;
        for elem in &curve.segments {
            match elem {
                PathElement::LineTo(_, _) => line_to_count += 1,
                PathElement::CurveTo(_, _, _, _, _, _) => curve_to_count += 1,
                _ => {}
            }
        }
        assert!(line_to_count >= 3);
        assert_eq!(
            curve_to_count, 0,
            "should not have spurious CurveTo elements"
        );
        assert_finite_elements(&curve.segments);
    }

    #[test]
    fn test_fit_square_high_smoothness() {
        let pts = sample_rect(0.0, 0.0, 10.0, 10.0, 10);
        let contour = Contour {
            points: pts,
            children: vec![],
        };
        let set = ContourSet {
            layers: vec![vec![contour]],
        };

        // Very high smoothness (1.3) treats corners as smooth, but
        // straight-run recovery still represents the square's exactly
        // straight edges as LineTos rather than melting them into cubics.
        let curve_set = fit_contours(&set, 0.5, 1.3);
        assert_eq!(curve_set.curves.len(), 1);
        let curve = &curve_set.curves[0];

        let mut line_to_count = 0;
        for elem in &curve.segments {
            if let PathElement::LineTo(_, _) = elem {
                line_to_count += 1;
            }
        }
        assert!(
            line_to_count >= 3,
            "straight edges should be recovered as lines even without detected corners"
        );
        assert_finite_elements(&curve.segments);
    }

    #[test]
    fn test_node_count_reduction_and_perturbation() {
        // Build 100 collinear points along bottom: (i*0.1, 0)
        let mut pts = Vec::new();
        for i in 0..100 {
            pts.push((i as f64 * 0.1, 0.0));
        }
        // Complete the square with a few points
        pts.push((10.0, 2.0));
        pts.push((10.0, 10.0));
        pts.push((0.0, 10.0));

        let contour = Contour {
            points: pts.clone(),
            children: vec![],
        };
        let set = ContourSet {
            layers: vec![vec![contour]],
        };

        let curve_set = fit_contours(&set, 0.5, 0.1);
        let curve = &curve_set.curves[0];

        // Output segments should be far fewer than 103 points
        assert!(
            curve.segments.len() < 10,
            "too many segments: {}",
            curve.segments.len()
        );
        assert_finite_elements(&curve.segments);

        // Perturb one point in the dense chain by more than tolerance (0.5)
        // Let's perturb the 50th point: from (5.0, 0.0) to (5.0, 1.0)
        let mut perturbed_pts = pts;
        perturbed_pts[50] = (5.0, 1.0);

        let perturbed_contour = Contour {
            points: perturbed_pts,
            children: vec![],
        };
        let perturbed_set = ContourSet {
            layers: vec![vec![perturbed_contour]],
        };

        let perturbed_curve_set = fit_contours(&perturbed_set, 0.5, 0.1);
        let perturbed_curve = &perturbed_curve_set.curves[0];

        // The perturbation should force extra vertices/segments compared to the unperturbed one
        assert!(
            perturbed_curve.segments.len() > curve.segments.len(),
            "perturbed curve should have more segments (got {}) than unperturbed (got {})",
            perturbed_curve.segments.len(),
            curve.segments.len()
        );
        assert_finite_elements(&perturbed_curve.segments);
    }

    #[test]
    fn test_determinism() {
        let pts = sample_rect(0.0, 0.0, 10.0, 10.0, 15);
        let contour = Contour {
            points: pts,
            children: vec![],
        };
        let set = ContourSet {
            layers: vec![vec![contour]],
        };

        let res1 = fit_contours(&set, 0.5, 0.5);
        let res2 = fit_contours(&set, 0.5, 0.5);

        let json1 = serde_json::to_vec(&res1).unwrap();
        let json2 = serde_json::to_vec(&res2).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn test_flattening_with_hole() {
        let outer = Contour {
            points: sample_rect(0.0, 0.0, 20.0, 20.0, 10),
            children: vec![Contour {
                points: sample_rect(5.0, 5.0, 10.0, 10.0, 10),
                children: vec![],
            }],
        };
        let set = ContourSet {
            layers: vec![vec![outer]],
        };

        let curve_set = fit_contours(&set, 0.5, 0.5);
        // One shape: the hole is a second subpath of the same evenodd path,
        // never a separate same-colored curve painted on top.
        assert_eq!(curve_set.curves.len(), 1);
        let segments = &curve_set.curves[0].segments;
        assert_finite_elements(segments);
        let move_tos = segments
            .iter()
            .filter(|e| matches!(e, PathElement::MoveTo(_, _)))
            .count();
        assert_eq!(move_tos, 2, "outer boundary + hole subpath");
        assert!(curve_set.curves[0].primitive.is_none());
    }

    #[test]
    fn test_merge_reduces_curve_count() {
        let n = 128;
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            let r = 30.0 * (1.0 + 0.3 * theta.cos());
            pts.push((50.0 + r * theta.cos(), 50.0 + r * theta.sin()));
        }

        let mut contour_tangents = vec![(0.0, 0.0); n];
        for i in 0..n {
            let prev = pts[(i + n - 1) % n];
            let next = pts[(i + 1) % n];
            let dx = next.0 - prev.0;
            let dy = next.1 - prev.1;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1e-9 {
                contour_tangents[i] = (dx / len, dy / len);
            } else {
                contour_tangents[i] = (1.0, 0.0);
            }
        }

        let mut span_indices = Vec::new();
        for i in 0..n {
            span_indices.push(i);
        }
        span_indices.push(0);

        let mut found_reduction = false;
        for &tolerance in &[0.05, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.8, 1.0, 1.2, 1.5] {
            let mut unmerged_elements = Vec::new();
            fit_recursive(
                &pts,
                &contour_tangents,
                &span_indices,
                true,
                true,
                tolerance,
                0,
                span_indices.len() - 1,
                &mut unmerged_elements,
            );

            let merged_elements = merge_bezier_segments(
                &pts,
                &contour_tangents,
                &span_indices,
                true,
                true,
                tolerance,
                &unmerged_elements,
            );

            println!(
                "tolerance: {}, unmerged: {}, merged: {}",
                tolerance,
                unmerged_elements.len(),
                merged_elements.len()
            );

            if merged_elements.len() < unmerged_elements.len() {
                found_reduction = true;
                break;
            }
        }

        assert!(
            found_reduction,
            "Expected merge pass to reduce node count for at least one tolerance value"
        );
    }

    #[test]
    fn test_merge_respects_tolerance() {
        let pts = sample_circle(50.0, 50.0, 25.0, 64);

        let n = pts.len();
        let mut contour_tangents = vec![(0.0, 0.0); n];
        for i in 0..n {
            let prev = pts[(i + n - 1) % n];
            let next = pts[(i + 1) % n];
            let dx = next.0 - prev.0;
            let dy = next.1 - prev.1;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1e-9 {
                contour_tangents[i] = (dx / len, dy / len);
            } else {
                contour_tangents[i] = (1.0, 0.0);
            }
        }

        let mut span_indices = Vec::new();
        for i in 0..n {
            span_indices.push(i);
        }
        span_indices.push(0);

        let tolerance = 1.0;

        let mut unmerged_elements = Vec::new();
        fit_recursive(
            &pts,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance,
            0,
            span_indices.len() - 1,
            &mut unmerged_elements,
        );

        let merged_elements = merge_bezier_segments(
            &pts,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance,
            &unmerged_elements,
        );

        let mut current_idx = 0;
        for elem in &merged_elements {
            if let PathElement::CurveTo(x1, y1, x2, y2, x3, y3) = *elem {
                let mut end_idx = current_idx + 1;
                while end_idx < span_indices.len() {
                    let pt = pts[span_indices[end_idx]];
                    if (pt.0 - x3).abs() < 1e-9 && (pt.1 - y3).abs() < 1e-9 {
                        break;
                    }
                    end_idx += 1;
                }
                assert!(
                    end_idx < span_indices.len(),
                    "Could not find end point of CurveTo in span_indices"
                );

                let run_points: Vec<(f64, f64)> = span_indices[current_idx..=end_idx]
                    .iter()
                    .map(|&idx| pts[idx])
                    .collect();

                let max_dev = evaluate_bezier_error(&run_points, (x1, y1), (x2, y2));
                assert!(
                    max_dev <= tolerance + 1e-9,
                    "Max deviation {} exceeded tolerance {}",
                    max_dev,
                    tolerance
                );

                current_idx = end_idx;
            }
        }
    }

    #[test]
    fn test_merge_determinism() {
        let pts = sample_circle(50.0, 50.0, 25.0, 64);

        let n = pts.len();
        let mut contour_tangents = vec![(0.0, 0.0); n];
        for i in 0..n {
            let prev = pts[(i + n - 1) % n];
            let next = pts[(i + 1) % n];
            let dx = next.0 - prev.0;
            let dy = next.1 - prev.1;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1e-9 {
                contour_tangents[i] = (dx / len, dy / len);
            } else {
                contour_tangents[i] = (1.0, 0.0);
            }
        }

        let mut span_indices = Vec::new();
        for i in 0..n {
            span_indices.push(i);
        }
        span_indices.push(0);

        let tolerance = 1.0;

        let mut unmerged1 = Vec::new();
        fit_recursive(
            &pts,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance,
            0,
            span_indices.len() - 1,
            &mut unmerged1,
        );

        let merged1 = merge_bezier_segments(
            &pts,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance,
            &unmerged1,
        );

        let mut unmerged2 = Vec::new();
        fit_recursive(
            &pts,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance,
            0,
            span_indices.len() - 1,
            &mut unmerged2,
        );

        let merged2 = merge_bezier_segments(
            &pts,
            &contour_tangents,
            &span_indices,
            true,
            true,
            tolerance,
            &unmerged2,
        );

        assert_eq!(merged1.len(), merged2.len());
        for (e1, e2) in merged1.iter().zip(merged2.iter()) {
            match (e1, e2) {
                (
                    PathElement::CurveTo(ax1, ay1, ax2, ay2, ax3, ay3),
                    PathElement::CurveTo(bx1, by1, bx2, by2, bx3, by3),
                ) => {
                    assert!((ax1 - bx1).abs() < 1e-15);
                    assert!((ay1 - by1).abs() < 1e-15);
                    assert!((ax2 - bx2).abs() < 1e-15);
                    assert!((ay2 - by2).abs() < 1e-15);
                    assert!((ax3 - bx3).abs() < 1e-15);
                    assert!((ay3 - by3).abs() < 1e-15);
                }
                _ => panic!("Expected CurveTo elements only"),
            }
        }
    }
