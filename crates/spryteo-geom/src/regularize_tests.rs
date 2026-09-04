    use super::*;
    use crate::recognize;

    fn sample_circle_pts(cx: f64, cy: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        for i in 0..n {
            let theta = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

    fn sample_arc_pts(
        cx: f64,
        cy: f64,
        r: f64,
        start_deg: f64,
        end_deg: f64,
        n: usize,
    ) -> Vec<(f64, f64)> {
        let mut pts = Vec::with_capacity(n);
        let start_rad = start_deg.to_radians();
        let end_rad = end_deg.to_radians();
        let total_sweep = if end_rad >= start_rad {
            end_rad - start_rad
        } else {
            end_rad + 2.0 * std::f64::consts::PI - start_rad
        };
        for i in 0..n {
            let t = i as f64 / (n - 1) as f64;
            let theta = start_rad + t * total_sweep;
            pts.push((cx + r * theta.cos(), cy + r * theta.sin()));
        }
        pts
    }

    #[test]
    fn test_fit_line_horizontal() {
        let pts = vec![(0.0, 5.0), (5.0, 5.0), (10.0, 5.0)];
        let res = fit_line(&pts, 0.1).expect("fit_line horizontal should succeed");
        assert!((res.0 .0 - 0.0).abs() < 1e-6);
        assert!((res.0 .1 - 5.0).abs() < 1e-6);
        assert!((res.1 .0 - 10.0).abs() < 1e-6);
        assert!((res.1 .1 - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_fit_line_vertical() {
        let pts = vec![(5.0, 0.0), (5.0, 5.0), (5.0, 10.0)];
        let res = fit_line(&pts, 0.1).expect("fit_line vertical should succeed");
        assert!((res.0 .0 - 5.0).abs() < 1e-6);
        assert!((res.0 .1 - 0.0).abs() < 1e-6);
        assert!((res.1 .0 - 5.0).abs() < 1e-6);
        assert!((res.1 .1 - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_fit_line_rejects_non_collinear() {
        let pts = vec![(0.0, 0.0), (5.0, 5.0), (10.0, 0.0)];
        assert!(fit_line(&pts, 0.1).is_none());
    }

    #[test]
    fn test_fit_line_too_few_points() {
        assert!(fit_line(&[], 0.1).is_none());
        assert!(fit_line(&[(0.0, 0.0)], 0.1).is_none());
    }

    #[test]
    fn test_fit_circle_recovers_known_circle() {
        let pts = sample_circle_pts(20.0, 30.0, 15.0, 32);
        let (cx, cy, r) = fit_circle(&pts, 1e-4).expect("fit_circle should succeed");
        assert!((cx - 20.0).abs() < 1e-6);
        assert!((cy - 30.0).abs() < 1e-6);
        assert!((r - 15.0).abs() < 1e-6);
    }

    #[test]
    fn test_fit_circle_rejects_collinear_points() {
        let pts = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
        assert!(fit_circle(&pts, 1.0).is_none());
    }

    #[test]
    fn test_fit_circle_too_few_points() {
        assert!(fit_circle(&[], 1.0).is_none());
        assert!(fit_circle(&[(0.0, 0.0), (1.0, 1.0)], 1.0).is_none());
    }

    #[test]
    fn test_fit_arc_quarter_circle() {
        let pts = sample_arc_pts(0.0, 0.0, 10.0, 0.0, 90.0, 16);
        let prim = fit_arc(&pts, 1e-4).expect("fit_arc quarter circle should succeed");
        match prim {
            Primitive::Arc {
                cx,
                cy,
                rx,
                ry,
                start_angle,
                end_angle,
                rotation,
            } => {
                assert!((cx - 0.0).abs() < 1e-4);
                assert!((cy - 0.0).abs() < 1e-4);
                assert!((rx - 10.0).abs() < 1e-4);
                assert!((ry - 10.0).abs() < 1e-4);
                assert!((rotation - 0.0).abs() < 1e-6);
                let sweep = (end_angle - start_angle).abs();
                assert!((sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-3);
            }
            other => panic!("expected Arc, got {other:?}"),
        }
    }

    #[test]
    fn test_fit_arc_across_pi_boundary() {
        let pts = sample_arc_pts(0.0, 0.0, 10.0, 170.0, -170.0, 16);
        let prim = fit_arc(&pts, 1e-4).expect("fit_arc across pi boundary should succeed");
        match prim {
            Primitive::Arc {
                start_angle,
                end_angle,
                ..
            } => {
                let sweep = (end_angle - start_angle).abs();
                let expected_sweep = 20.0_f64.to_radians();
                assert!(
                    (sweep - expected_sweep).abs() < 1e-3,
                    "expected ~20 deg sweep ({expected_sweep}), got {sweep}"
                );
            }
            other => panic!("expected Arc, got {other:?}"),
        }
    }

    #[test]
    fn test_fit_arc_full_circle_returns_none() {
        let pts = sample_circle_pts(0.0, 0.0, 10.0, 64);
        assert!(fit_arc(&pts, 1e-3).is_none());
    }

    #[test]
    fn test_snap_angle_snaps_near_horizontal() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 0.1);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 5.0);
        assert!(
            (na.1 - nb.1).abs() < 1e-6,
            "y-coordinates should match after snapping"
        );
    }

    #[test]
    fn test_snap_angle_leaves_far_angle_alone() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 3.0);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 2.0);
        assert_eq!((na, nb), (a, b));
    }

    #[test]
    fn test_snap_angle_preserves_length() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 0.5);
        let orig_len = (b.0 - a.0).hypot(b.1 - a.1);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 10.0);
        let new_len = (nb.0 - na.0).hypot(nb.1 - na.1);
        assert!((orig_len - new_len).abs() < 1e-6);
    }

    #[test]
    fn test_snap_angle_pivots_about_midpoint() {
        let a: (f64, f64) = (0.0, 0.0);
        let b: (f64, f64) = (10.0, 0.5);
        let orig_mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (na, nb) = snap_angle(a, b, &[0.0, 90.0], 10.0);
        let new_mid = ((na.0 + nb.0) / 2.0, (na.1 + nb.1) / 2.0);
        assert!((orig_mid.0 - new_mid.0).abs() < 1e-6);
        assert!((orig_mid.1 - new_mid.1).abs() < 1e-6);
    }

    #[test]
    fn test_snap_to_grid_snaps_within_tolerance() {
        let p = (10.1, 20.05);
        let snapped = snap_to_grid(p, 1.0, 0.2);
        assert_eq!(snapped, (10.0, 20.0));
    }

    #[test]
    fn test_snap_to_grid_rejects_when_one_axis_too_far() {
        let p = (10.1, 20.5);
        let snapped = snap_to_grid(p, 1.0, 0.2);
        assert_eq!(snapped, p);
    }

    #[test]
    fn test_weld_endpoints_merges_nearby() {
        let mut curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(10.05, 0.0),
                    PathElement::LineTo(20.0, 0.0),
                ],
                primitive: None,
            },
        ];
        let moved = weld_endpoints(&mut curves, 0.1);
        assert_eq!(moved, 2);
        let end1 = match &curves[0].segments[1] {
            PathElement::LineTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        let start2 = match &curves[1].segments[0] {
            PathElement::MoveTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        assert_eq!(end1, start2);
        assert!((end1.0 - 10.025).abs() < 1e-6);
    }

    #[test]
    fn test_weld_endpoints_leaves_distant_alone() {
        let mut curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(10.5, 0.0),
                    PathElement::LineTo(20.0, 0.0),
                ],
                primitive: None,
            },
        ];
        let moved = weld_endpoints(&mut curves, 0.1);
        assert_eq!(moved, 0);
    }

    #[test]
    fn test_weld_endpoints_is_deterministic() {
        let make_curves = || {
            vec![
                Curve {
                    segments: vec![
                        PathElement::MoveTo(0.0, 0.0),
                        PathElement::LineTo(10.0, 0.0),
                    ],
                    primitive: None,
                },
                Curve {
                    segments: vec![
                        PathElement::MoveTo(10.04, 0.0),
                        PathElement::LineTo(20.0, 0.0),
                    ],
                    primitive: None,
                },
            ]
        };

        let mut c1 = make_curves();
        let mut c2 = make_curves();

        let m1 = weld_endpoints(&mut c1, 0.1);
        let m2 = weld_endpoints(&mut c2, 0.1);

        assert_eq!(m1, m2);
        assert_eq!(format!("{c1:?}"), format!("{c2:?}"));
    }

    #[test]
    fn test_close_loops_closes_near_loop() {
        let mut curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(0.05, 0.02),
            ],
            primitive: None,
        };
        let changed = close_loops(&mut curve, 0.1);
        assert!(changed);
        assert_eq!(curve.segments.len(), 4);
        assert!(matches!(
            curve.segments.last(),
            Some(PathElement::ClosePath)
        ));
        let last_line = match &curve.segments[2] {
            PathElement::LineTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        assert_eq!(last_line, (0.0, 0.0));
    }

    #[test]
    fn test_close_loops_ignores_open_curve() {
        let mut curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(5.0, 5.0),
            ],
            primitive: None,
        };
        let changed = close_loops(&mut curve, 0.1);
        assert!(!changed);
        assert_eq!(curve.segments.len(), 3);
    }

    #[test]
    fn test_close_loops_idempotent() {
        let mut curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(0.05, 0.0),
            ],
            primitive: None,
        };
        assert!(close_loops(&mut curve, 0.1));
        assert!(!close_loops(&mut curve, 0.1));
        assert_eq!(curve.segments.len(), 4);
    }

    #[test]
    fn test_detect_mirror_axis_finds_symmetric() {
        let curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(5.0, 0.0),
                    PathElement::LineTo(5.0, 10.0),
                ],
                primitive: None,
            },
        ];
        let axis = detect_mirror_axis(&curves, 0.1).expect("should find mirror axis");
        assert!((axis - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_detect_mirror_axis_none_for_asymmetric() {
        let curves = vec![Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(2.0, 8.0),
            ],
            primitive: None,
        }];
        assert!(detect_mirror_axis(&curves, 0.1).is_none());
    }

    #[test]
    fn test_unify_widths_merges_cluster() {
        let mut widths = vec![2.9, 3.1, 3.0];
        let changed = unify_widths(&mut widths, 0.1, 0.5);
        assert!(changed);
        assert_eq!(widths, vec![3.0, 3.0, 3.0]);
    }

    #[test]
    fn test_unify_widths_keeps_distinct() {
        let mut widths = vec![3.0, 8.0];
        let changed = unify_widths(&mut widths, 0.1, 1.0);
        assert!(!changed);
        assert_eq!(widths, vec![3.0, 8.0]);
    }

    #[test]
    fn test_unify_widths_zero_quantum_does_not_divide_by_zero() {
        let mut widths = vec![2.9, 3.1, 3.0];
        let changed = unify_widths(&mut widths, 0.1, 0.0);
        assert!(changed);
        assert_eq!(widths, vec![3.0, 3.0, 3.0]);
    }

    #[test]
    fn test_recognize_output_is_unchanged() {
        let pts = sample_circle_pts(50.0, 30.0, 20.0, 64);
        let prim = recognize(&pts, 1.0).expect("existing recognize should return circle");
        match prim {
            Primitive::Circle { cx, cy, r } => {
                assert!((cx - 50.0).abs() < 0.5);
                assert!((cy - 30.0).abs() < 0.5);
                assert!((r - 20.0).abs() < 0.5);
            }
            other => panic!("expected Circle, got {other:?}"),
        }
    }

    #[test]
    fn test_regularize_empty_input_does_not_panic() {
        let mut curves = vec![];
        let mut widths = vec![];
        let cfg = RegularizeConfig::default();
        let rep = regularize(&mut curves, &mut widths, &cfg);
        assert_eq!(rep.welded, 0);
        assert_eq!(rep.points_gridded, 0);
        assert_eq!(rep.confidence, 1.0);
    }

    #[test]
    fn test_regularize_mismatched_lengths_does_not_panic() {
        let mut curves = vec![
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 5.0),
                    PathElement::LineTo(10.0, 5.0),
                ],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 10.0),
                    PathElement::LineTo(10.0, 10.0),
                ],
                primitive: None,
            },
        ];
        let mut widths = vec![1.0];
        let cfg = RegularizeConfig::default();
        let rep = regularize(&mut curves, &mut widths, &cfg);
        assert!(rep.confidence >= 0.0);
    }

    #[test]
    fn test_regularize_zero_pitch_skips_gridding() {
        let mut curves = vec![Curve {
            segments: vec![PathElement::MoveTo(0.1, 0.1), PathElement::LineTo(9.9, 0.1)],
            primitive: None,
        }];
        let mut widths = vec![1.0];
        let cfg = RegularizeConfig {
            grid_pitch: 0.0,
            ..Default::default()
        };
        let rep = regularize(&mut curves, &mut widths, &cfg);
        assert_eq!(rep.points_gridded, 0);
    }

    #[test]
    fn test_regularize_welds_before_snapping() {
        let mut curves = vec![
            Curve {
                segments: vec![PathElement::MoveTo(0.0, 0.0), PathElement::LineTo(9.8, 0.0)],
                primitive: None,
            },
            Curve {
                segments: vec![
                    PathElement::MoveTo(10.3, 0.0),
                    PathElement::LineTo(20.0, 0.0),
                ],
                primitive: None,
            },
        ];
        let mut widths = vec![1.0, 1.0];
        let cfg = RegularizeConfig {
            grid_pitch: 10.0,
            max_grid_dev: 1.5,
            weld_eps: 1.0,
            ..Default::default()
        };
        let _rep = regularize(&mut curves, &mut widths, &cfg);
        let c1_end = match &curves[0].segments[1] {
            PathElement::LineTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        let c2_start = match &curves[1].segments[0] {
            PathElement::MoveTo(x, y) => (*x, *y),
            _ => panic!(),
        };
        // Welding runs first, so the two ends that should be one junction end
        // up at one exact coordinate.
        assert_eq!(c1_end, c2_start);
        // And they land on the welded centroid, NOT on the grid line at 10.0:
        // these curves carry no recognised primitive, so their points are
        // traced samples and gridding must leave them alone. Asserting the
        // unsnapped value is what pins that rule -- an implementation that
        // gridded raw points would put this at exactly (10.0, 0.0).
        assert!(
            (c1_end.0 - 10.05).abs() < 1e-9,
            "expected the welded centroid 10.05, got {:?} -- raw traced points \
             must not be snapped to the grid",
            c1_end
        );
    }

    #[test]
    fn test_regularize_confidence_drops_with_displacement() {
        let mut curves1 = vec![Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.1),
            ],
            primitive: None,
        }];
        let mut widths1 = vec![1.0];
        let cfg1 = RegularizeConfig {
            grid_pitch: 10.0,
            max_grid_dev: 2.0,
            max_angle_dev_deg: 10.0,
            ..Default::default()
        };
        let rep1 = regularize(&mut curves1, &mut widths1, &cfg1);

        let mut curves2 = vec![Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 1.2),
            ],
            primitive: None,
        }];
        let mut widths2 = vec![1.0];
        let cfg2 = RegularizeConfig {
            grid_pitch: 10.0,
            max_grid_dev: 2.0,
            max_angle_dev_deg: 10.0,
            ..Default::default()
        };
        let rep2 = regularize(&mut curves2, &mut widths2, &cfg2);

        assert!(
            rep1.confidence > rep2.confidence,
            "rep1.confidence ({}) should be > rep2.confidence ({})",
            rep1.confidence,
            rep2.confidence
        );
    }

    #[test]
    fn test_regularize_clean_geometry_is_near_untouched() {
        let mut curves = vec![Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
            ],
            primitive: None,
        }];
        let mut widths = vec![1.0];
        let cfg = RegularizeConfig::default();
        let rep = regularize(&mut curves, &mut widths, &cfg);
        assert!(rep.max_displacement < 1e-6);
        assert!((rep.confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_regularize_is_deterministic() {
        let make_input = || {
            (
                vec![
                    Curve {
                        segments: vec![
                            PathElement::MoveTo(0.05, 0.02),
                            PathElement::LineTo(9.95, 0.01),
                        ],
                        primitive: None,
                    },
                    Curve {
                        segments: vec![
                            PathElement::MoveTo(10.02, 0.03),
                            PathElement::LineTo(20.01, 0.02),
                        ],
                        primitive: None,
                    },
                ],
                vec![1.02, 0.98],
            )
        };
        let (mut c1, mut w1) = make_input();
        let (mut c2, mut w2) = make_input();
        let cfg = RegularizeConfig::default();

        let rep1 = regularize(&mut c1, &mut w1, &cfg);
        let rep2 = regularize(&mut c2, &mut w2, &cfg);

        assert_eq!(rep1, rep2);
        assert_eq!(format!("{c1:?}"), format!("{c2:?}"));
        assert_eq!(w1, w2);
    }
