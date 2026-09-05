    use super::*;
    use spryteo_core::ir::{
        Curve, CurveSet, Fill, GradientStop, PathElement, Primitive, Rgb, Shape,
    };
    use spryteo_core::options::{ConvertOptions, IdStyle, OutputFormat, Preset, TOrigin};

    fn make_test_options() -> ConvertOptions {
        ConvertOptions::default()
    }

    fn assert_all_numbers_finite(svg: &str) {
        let mut s = String::new();
        for c in svg.chars() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                s.push(c);
            } else {
                s.push(' ');
            }
        }
        for token in s.split_whitespace() {
            if token == "-" || token == "+" || token == "." || token == "e" || token == "E" {
                continue;
            }
            if let Ok(val) = token.parse::<f64>() {
                assert!(
                    val.is_finite(),
                    "Found non-finite number: {} in token: {}",
                    val,
                    token
                );
            }
        }
    }

    #[test]
    fn test_two_curves_distinct_groups() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(30.0, 20.0),
                PathElement::LineTo(30.0, 30.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(&curves, &IdStyle::Hash, &TOrigin::Centroid, &fills, false);

        assert_eq!(scene.groups.len(), 2);
        assert_eq!(scene.groups[0].nodes.len(), 1);
        assert_eq!(scene.groups[1].nodes.len(), 1);
        assert_ne!(scene.groups[0].nodes[0].id, scene.groups[1].nodes[0].id);

        assert_ne!(scene.groups[0].nodes[0].transform.translate_x, 0.0);
        assert_ne!(scene.groups[0].nodes[0].transform.translate_y, 0.0);
    }

    #[test]
    fn test_emit_primitive_circle() {
        let curve = Curve {
            segments: vec![],
            primitive: Some(Primitive::Circle {
                cx: 50.0,
                cy: 50.0,
                r: 10.0,
            }),
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 0, g: 255, b: 0 })];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(res.svg.contains("<circle"));
        assert!(res.svg.contains("cx=\"50.00\""));
        assert!(res.svg.contains("cy=\"50.00\""));
        assert!(res.svg.contains("r=\"10.00\""));
        assert!(!res.svg.contains("<path"));
        assert_all_numbers_finite(&res.svg);
    }

    #[test]
    fn test_emit_path() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb {
            r: 128,
            g: 128,
            b: 128,
        })];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(res.svg.contains("<path"));
        assert!(res
            .svg
            .contains("d=\"M 0.00 0.00 L 10.00 0.00 L 10.00 10.00 Z\""));
        assert_all_numbers_finite(&res.svg);
    }

    #[test]
    fn test_id_styles() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];

        let scene_hash = build_scene_graph(&curves, &IdStyle::Hash, &TOrigin::Baked, &fills, false);
        let mut opts = make_test_options();
        opts.id_style = IdStyle::Hash;
        let res_hash = emit_svg(&scene_hash, 100, 100, &opts, None);
        assert!(res_hash.svg.contains("id=\"s-"));

        let scene_none = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);
        opts.id_style = IdStyle::None;
        let res_none = emit_svg(&scene_none, 100, 100, &opts, None);
        assert!(!res_none.svg.contains("id="));

        let scene_seq = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );
        opts.id_style = IdStyle::Sequential;
        let res_seq = emit_svg(&scene_seq, 100, 100, &opts, None);
        assert!(res_seq.svg.contains("id=\"s-0\""));
    }

    #[test]
    fn test_precision() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(12.3456, 78.91011),
                PathElement::LineTo(0.0001, -0.0),
                PathElement::LineTo(5.0, 5.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb {
            r: 255,
            g: 255,
            b: 255,
        })];

        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);

        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;
        opts.precision = 1;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(res.svg.contains("12.3"));
        assert!(res.svg.contains("78.9"));
        assert!(res.svg.contains("0.0"));
        assert!(!res.svg.contains("12.35"));
        assert_all_numbers_finite(&res.svg);
    }

    #[test]
    fn test_pretty_vs_minified() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 0, g: 0, b: 0 })];

        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);

        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;

        opts.output = OutputFormat::Svg;
        let res_min = emit_svg(&scene, 100, 100, &opts, None);

        opts.output = OutputFormat::SvgPretty;
        let res_pretty = emit_svg(&scene, 100, 100, &opts, None);

        assert!(res_pretty.svg.contains('\n'));
        assert!(res_min.svg.len() < res_pretty.svg.len());
        assert_all_numbers_finite(&res_min.svg);
        assert_all_numbers_finite(&res_pretty.svg);
    }

    #[test]
    fn test_viewbox() {
        let curves = CurveSet { curves: vec![] };
        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &[], false);

        let opts = make_test_options();
        let res = emit_svg(&scene, 412, 927, &opts, None);
        assert!(res.svg.contains("viewBox=\"0 0 412 927\""));
    }

    #[test]
    fn test_sanitize_injection() {
        let node = Node {
            id: "<script>alert('hack')</script>".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 1.0,
                cy: 1.0,
                r: 1.0,
            }),
        };
        let group = Group {
            id: "<script>alert('group')</script>".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Hash;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        assert!(!res.svg.contains("<script>"));
        assert!(res.svg.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_determinism() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(1.23, 4.56),
                PathElement::LineTo(7.89, 0.12),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb {
            r: 255,
            g: 100,
            b: 50,
        })];

        let scene = build_scene_graph(&curves, &IdStyle::Hash, &TOrigin::Centroid, &fills, false);

        let opts = make_test_options();
        let res1 = emit_svg(&scene, 500, 500, &opts, None);
        let res2 = emit_svg(&scene, 500, 500, &opts, None);

        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_stroke_emit_open_path() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let scene = build_stroke_scene_graph(&curves, &IdStyle::Sequential, &[5.432]);
        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;
        opts.precision = 2;

        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = res.svg;

        assert!(svg.contains("<path"), "SVG should contain path element");
        assert!(svg.contains("fill=\"none\""), "fill should be none");
        assert!(
            svg.contains("stroke=\"#000000\""),
            "stroke color should be #000000"
        );
        assert!(
            svg.contains("stroke-width=\"5.43\""),
            "stroke-width should match and be rounded to precision"
        );
        assert!(
            svg.contains("pathLength=\"100\""),
            "pathLength should be 100"
        );

        // Assert open path (d attribute does not end with Z or z)
        assert!(
            svg.contains("d=\"M 10.00 10.00 L 20.00 30.00\""),
            "d attribute should not end with Z/z"
        );
        assert!(!svg.contains('Z'), "should not contain Z");
        assert!(!svg.contains('z'), "should not contain z");
    }

    #[test]
    fn test_stroke_emit_css_draw() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(40.0, 40.0),
                PathElement::LineTo(50.0, 60.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let scene = build_stroke_scene_graph(&curves, &IdStyle::Sequential, &[3.0, 4.0]);

        let mut opts_with_css = make_test_options();
        opts_with_css.id_style = IdStyle::Sequential;
        opts_with_css.emit_css = Some(Preset::Draw);
        opts_with_css.output = OutputFormat::SvgPretty;

        let res_with = emit_stroke_svg(&scene, 100, 100, &opts_with_css);
        assert!(res_with.svg.contains("<style"), "should contain style tag");
        assert!(
            res_with.svg.contains("@keyframes"),
            "should contain keyframes"
        );
        assert!(
            res_with.svg.contains("stroke-dasharray"),
            "should contain stroke-dasharray"
        );

        // Staggered delays check
        assert!(
            res_with.svg.contains("animation-delay: 0ms;"),
            "should contain delay for first node"
        );
        assert!(
            res_with.svg.contains("animation-delay: 100ms;"),
            "should contain delay for second node"
        );

        let mut opts_no_css = make_test_options();
        opts_no_css.id_style = IdStyle::Sequential;
        opts_no_css.emit_css = None;

        let res_without = emit_stroke_svg(&scene, 100, 100, &opts_no_css);
        assert!(
            !res_without.svg.contains("<style"),
            "should not contain style tag"
        );
        assert!(
            !res_without.svg.contains("@keyframes"),
            "should not contain keyframes"
        );
        assert!(
            !res_without.svg.contains("stroke-dasharray"),
            "should not contain stroke-dasharray in CSS"
        );
    }

    #[test]
    fn test_stroke_viewbox_and_id_styles() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let scene_hash = build_stroke_scene_graph(&curves, &IdStyle::Hash, &[2.0]);
        let mut opts = make_test_options();
        opts.id_style = IdStyle::Hash;
        let res_hash = emit_stroke_svg(&scene_hash, 150, 250, &opts);
        assert!(
            res_hash.svg.contains("viewBox=\"0 0 150 250\""),
            "should preserve viewBox dimensions"
        );
        assert!(
            res_hash.svg.contains("id=\"s-"),
            "should contain hash-based ID"
        );

        let scene_none = build_stroke_scene_graph(&curves, &IdStyle::None, &[2.0]);
        opts.id_style = IdStyle::None;
        let res_none = emit_stroke_svg(&scene_none, 150, 250, &opts);
        assert!(!res_none.svg.contains("id="), "should omit ID attribute");
    }

    #[test]
    fn test_stroke_determinism() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 30.0),
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(40.0, 40.0),
                PathElement::LineTo(50.0, 60.0),
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let scene = build_stroke_scene_graph(&curves, &IdStyle::Hash, &[3.0, 4.0]);
        let opts = make_test_options();

        let res1 = emit_stroke_svg(&scene, 100, 100, &opts);
        let res2 = emit_stroke_svg(&scene, 100, 100, &opts);

        assert_eq!(res1.svg, res2.svg, "outputs should be byte-identical");
        assert_eq!(res1.meta.stats.byte_count, res2.meta.stats.byte_count);
    }

    #[test]
    fn test_gradient_emission_linear_radial_determinism() {
        // 1. Linear gradient test
        let curve_linear = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let fills_linear = vec![Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 10.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        }];
        let curves_linear = CurveSet {
            curves: vec![curve_linear],
        };
        let scene_linear = build_scene_graph(
            &curves_linear,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_linear,
            false,
        );
        let opts = make_test_options();
        let res_linear = emit_svg(&scene_linear, 100, 100, &opts, None);

        assert!(
            res_linear.svg.contains("<defs>"),
            "linear: should contain defs block"
        );
        assert!(res_linear.svg.contains("<linearGradient id=\"grad-s-0\" gradientUnits=\"userSpaceOnUse\" x1=\"0.00\" y1=\"0.00\" x2=\"10.00\" y2=\"10.00\">"), "linear: should contain linearGradient with correct coordinates");
        assert!(
            res_linear
                .svg
                .contains("<stop offset=\"0%\" stop-color=\"#ff0000\" />"),
            "linear: stop 1"
        );
        assert!(
            res_linear
                .svg
                .contains("<stop offset=\"100%\" stop-color=\"#0000ff\" />"),
            "linear: stop 2"
        );
        assert!(
            res_linear.svg.contains("fill=\"url(#grad-s-0)\""),
            "linear: shape should reference gradient"
        );

        // 2. Radial gradient test
        let curve_radial = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(20.0, 0.0),
                PathElement::LineTo(20.0, 20.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let fills_radial = vec![Fill::RadialGradient {
            cx: 50.0,
            cy: 50.0,
            r: 30.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb {
                        r: 255,
                        g: 255,
                        b: 0,
                    },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb {
                        r: 0,
                        g: 255,
                        b: 255,
                    },
                },
            ],
        }];
        let curves_radial = CurveSet {
            curves: vec![curve_radial],
        };
        let scene_radial = build_scene_graph(
            &curves_radial,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_radial,
            false,
        );
        let res_radial = emit_svg(&scene_radial, 100, 100, &opts, None);

        assert!(
            res_radial.svg.contains("<defs>"),
            "radial: should contain defs block"
        );
        assert!(
            res_radial
                .svg
                .contains("<radialGradient id=\"grad-s-0\" gradientUnits=\"userSpaceOnUse\" cx=\"50.00\" cy=\"50.00\" r=\"30.00\">"),
            "radial: should contain radialGradient with correct coordinates"
        );
        assert!(
            res_radial
                .svg
                .contains("<stop offset=\"0%\" stop-color=\"#ffff00\" />"),
            "radial: stop 1"
        );
        assert!(
            res_radial
                .svg
                .contains("<stop offset=\"100%\" stop-color=\"#00ffff\" />"),
            "radial: stop 2"
        );
        assert!(
            res_radial.svg.contains("fill=\"url(#grad-s-0)\""),
            "radial: shape should reference gradient"
        );

        // 3. Two different nodes with gradients - check no clash and two distinct definitions
        let curve_two_1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve_two_2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 0.0),
                PathElement::LineTo(30.0, 0.0),
                PathElement::LineTo(30.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves_two = CurveSet {
            curves: vec![curve_two_1, curve_two_2],
        };
        let fills_two = vec![
            Fill::LinearGradient {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Rgb { r: 255, g: 0, b: 0 },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Rgb { r: 0, g: 0, b: 255 },
                    },
                ],
            },
            Fill::RadialGradient {
                cx: 2.0,
                cy: 2.0,
                r: 3.0,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Rgb { r: 0, g: 255, b: 0 },
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Rgb {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                    },
                ],
            },
        ];
        let scene_two = build_scene_graph(
            &curves_two,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_two,
            false,
        );
        let res_two = emit_svg(&scene_two, 100, 100, &opts, None);
        assert!(res_two.svg.contains("id=\"grad-s-0\""));
        assert!(res_two.svg.contains("id=\"grad-s-1\""));
        assert!(res_two.svg.contains("fill=\"url(#grad-s-0)\""));
        assert!(res_two.svg.contains("fill=\"url(#grad-s-1)\""));

        // 4. Solid color round-trip (regression guard)
        let curve_solid = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves_solid = CurveSet {
            curves: vec![curve_solid],
        };
        let fills_solid = vec![Fill::Solid(Rgb {
            r: 12,
            g: 34,
            b: 56,
        })];
        let scene_solid = build_scene_graph(
            &curves_solid,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_solid,
            false,
        );
        let res_solid = emit_svg(&scene_solid, 100, 100, &opts, None);
        assert!(
            res_solid.svg.contains("fill=\"#0c2238\""),
            "solid fill serialization regression check"
        );

        // 5. Determinism: emit twice and assert identical output
        let res_two_second = emit_svg(&scene_two, 100, 100, &opts, None);
        assert_eq!(
            res_two.svg, res_two_second.svg,
            "SVG outputs must be byte-identical"
        );
    }

    /// Gradient coordinates are produced in image space, but under `TOrigin::Centroid` a
    /// node is emitted in centroid-relative space with a compensating `translate(..)`.
    /// Because `userSpaceOnUse` resolves against the element's pre-transform space, the
    /// translate must be subtracted from the gradient geometry. Without that, the ramp
    /// lands entirely off the shape and every pixel clamps to a single stop -- the shape
    /// renders flat. Regression guard for that.
    #[test]
    fn test_gradient_coords_compensate_centroid_translate() {
        // A 100x100 square centred at (50,50): under Centroid origin this is emitted at
        // local (-50,-50)..(50,50) with transform="translate(50.00, 50.00)".
        let curves = CurveSet {
            curves: vec![Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(100.0, 0.0),
                    PathElement::LineTo(100.0, 100.0),
                    PathElement::LineTo(0.0, 100.0),
                    PathElement::ClosePath,
                ],
                primitive: None,
            }],
        };
        let fills = vec![Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        }];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Centroid,
            &fills,
            false,
        );
        let opts = make_test_options();
        let svg = emit_svg(&scene, 100, 100, &opts, None).svg;

        assert!(
            svg.contains("transform=\"translate(50.00, 50.00)\""),
            "precondition: node should carry a centroid translate, got: {svg}"
        );
        // Image-space 0..100 minus the (50,50) translate => local-space -50..50, which is
        // exactly the shape's own extent. Un-compensated output would read x1="0.00"
        // x2="100.00" and cover only the right half of the shape.
        assert!(
            svg.contains(
                "<linearGradient id=\"grad-s-0\" gradientUnits=\"userSpaceOnUse\" \
                 x1=\"-50.00\" y1=\"-50.00\" x2=\"50.00\" y2=\"-50.00\">"
            ),
            "gradient coords must be shifted into the node's local space, got: {svg}"
        );
    }

    #[test]
    fn test_current_color_behavior() {
        // 1. Single-color scene + flag -> currentColor and meta flag true
        let curves_single = CurveSet {
            curves: vec![Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                    PathElement::LineTo(10.0, 10.0),
                    PathElement::ClosePath,
                ],
                primitive: None,
            }],
        };
        let fills_single = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];
        let scene_single = build_scene_graph(
            &curves_single,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_single,
            false,
        );
        let mut opts = make_test_options();
        opts.current_color = true;

        let res_single = emit_svg(
            &scene_single,
            100,
            100,
            &opts,
            Some(Rgb { r: 0, g: 255, b: 0 }),
        );
        assert!(res_single.svg.contains("fill=\"currentColor\""));
        // background rect keeps its real color
        assert!(res_single.svg.contains("fill=\"#00ff00\""));
        assert!(res_single.meta.current_color_applied);

        // 2. Two-color scene + flag -> hex fills unchanged and meta flag false
        let curves_two = CurveSet {
            curves: vec![
                Curve {
                    segments: vec![
                        PathElement::MoveTo(0.0, 0.0),
                        PathElement::LineTo(10.0, 0.0),
                        PathElement::LineTo(10.0, 10.0),
                        PathElement::ClosePath,
                    ],
                    primitive: None,
                },
                Curve {
                    segments: vec![
                        PathElement::MoveTo(20.0, 10.0),
                        PathElement::LineTo(30.0, 10.0),
                        PathElement::LineTo(30.0, 20.0),
                        PathElement::ClosePath,
                    ],
                    primitive: None,
                },
            ],
        };
        let fills_two = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];
        let scene_two = build_scene_graph(
            &curves_two,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_two,
            false,
        );
        let res_two = emit_svg(&scene_two, 100, 100, &opts, None);
        assert!(!res_two.svg.contains("fill=\"currentColor\""));
        assert!(res_two.svg.contains("fill=\"#ff0000\""));
        assert!(res_two.svg.contains("fill=\"#0000ff\""));
        assert!(!res_two.meta.current_color_applied);

        // 3. Gradient present + flag -> unchanged/false
        let curves_grad = CurveSet {
            curves: vec![Curve {
                segments: vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(10.0, 0.0),
                    PathElement::LineTo(10.0, 10.0),
                    PathElement::ClosePath,
                ],
                primitive: None,
            }],
        };
        let fills_grad = vec![Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        }];
        let scene_grad = build_scene_graph(
            &curves_grad,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills_grad,
            false,
        );
        let res_grad = emit_svg(&scene_grad, 100, 100, &opts, None);
        assert!(!res_grad.svg.contains("fill=\"currentColor\""));
        assert!(!res_grad.meta.current_color_applied);

        // 4. Flag off -> unchanged/false even for single color
        opts.current_color = false;
        let res_off = emit_svg(&scene_single, 100, 100, &opts, None);
        assert!(!res_off.svg.contains("fill=\"currentColor\""));
        assert!(res_off.svg.contains("fill=\"#ff0000\""));
        assert!(!res_off.meta.current_color_applied);
    }

    #[test]
    fn test_containment_sorting() {
        // (a) Shape inside shape gets higher depth and paints later.
        // We input Inner first, then Outer.
        // Inner: square (20, 20) to (80, 80)
        let inner = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(80.0, 20.0),
                PathElement::LineTo(80.0, 80.0),
                PathElement::LineTo(20.0, 80.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // Outer: square (0, 0) to (100, 100)
        let outer = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(100.0, 0.0),
                PathElement::LineTo(100.0, 100.0),
                PathElement::LineTo(0.0, 100.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };

        let curves = CurveSet {
            curves: vec![inner.clone(), outer.clone()],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }), // Inner fill
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }), // Outer fill
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        // Outer should be painted first (index 0) because it contains Inner.
        // Inner (index 1) should be painted second.
        assert_eq!(scene.groups.len(), 2);
        // We can check their IDs. Inner has original index 0, so its ID is "s-0". Outer has original index 1, ID is "s-1".
        // With reordering, "s-1" (outer) should come first, then "s-0" (inner).
        assert_eq!(scene.groups[0].nodes[0].id, "s-1");
        assert_eq!(scene.groups[1].nodes[0].id, "s-0");

        // (b) Two disjoint shapes keep order.
        // Shape A: (0, 0) to (10, 10)
        let shape_a = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::LineTo(0.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // Shape B: (20, 0) to (30, 10)
        let shape_b = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 0.0),
                PathElement::LineTo(30.0, 0.0),
                PathElement::LineTo(30.0, 10.0),
                PathElement::LineTo(20.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };

        // Order [A, B] -> [A, B]
        let curves_ab = CurveSet {
            curves: vec![shape_a.clone(), shape_b.clone()],
        };
        let scene_ab = build_scene_graph(
            &curves_ab,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_ab.groups[0].nodes[0].id, "s-0");
        assert_eq!(scene_ab.groups[1].nodes[0].id, "s-1");

        // Order [B, A] -> [B, A]
        let curves_ba = CurveSet {
            curves: vec![shape_b.clone(), shape_a.clone()],
        };
        let scene_ba = build_scene_graph(
            &curves_ba,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_ba.groups[0].nodes[0].id, "s-0"); // original index 0 (which was shape_b)
        assert_eq!(scene_ba.groups[1].nodes[0].id, "s-1"); // original index 1 (which was shape_a)

        // (c) Near-identical boundaries keep order.
        // Shape C: (0, 0) to (100, 100) -> area 10000
        let shape_c = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(100.0, 0.0),
                PathElement::LineTo(100.0, 100.0),
                PathElement::LineTo(0.0, 100.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // Shape D: (0.5, 0.5) to (99.5, 99.5) -> area 99 * 99 = 9801.
        // 9801 >= 9800 (which is 10000 * 0.98). So they are near-identical!
        let shape_d = Curve {
            segments: vec![
                PathElement::MoveTo(0.5, 0.5),
                PathElement::LineTo(99.5, 0.5),
                PathElement::LineTo(99.5, 99.5),
                PathElement::LineTo(0.5, 99.5),
                PathElement::ClosePath,
            ],
            primitive: None,
        };

        // Order [C, D] -> [C, D]
        let curves_cd = CurveSet {
            curves: vec![shape_c.clone(), shape_d.clone()],
        };
        let scene_cd = build_scene_graph(
            &curves_cd,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_cd.groups[0].nodes[0].id, "s-0");
        assert_eq!(scene_cd.groups[1].nodes[0].id, "s-1");

        // Order [D, C] -> [D, C]
        let curves_dc = CurveSet {
            curves: vec![shape_d.clone(), shape_c.clone()],
        };
        let scene_dc = build_scene_graph(
            &curves_dc,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
                Fill::Solid(Rgb { r: 0, g: 0, b: 0 }),
            ],
            false,
        );
        assert_eq!(scene_dc.groups[0].nodes[0].id, "s-0");
        assert_eq!(scene_dc.groups[1].nodes[0].id, "s-1");
    }

    #[test]
    fn test_nested_groups_build_node_metas() {
        let inner_node = Node {
            id: "s-inner".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 5.0,
                cy: 5.0,
                r: 3.0,
            }),
        };
        let outer_node = Node {
            id: "s-outer".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 0.0,
                cy: 0.0,
                r: 10.0,
            }),
        };
        let inner_group = Group {
            id: "g-s-inner".to_string(),
            nodes: vec![inner_node],
            groups: vec![],
        };
        let outer_group = Group {
            id: "g-s-outer".to_string(),
            nodes: vec![outer_node],
            groups: vec![inner_group],
        };
        let scene = SceneGraph {
            groups: vec![outer_group],
        };

        let metas = build_node_metas(&scene, false);
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].id, "s-outer");
        assert_eq!(metas[0].group, "g-s-outer");
        assert_eq!(metas[0].z_order, 0);
        assert_eq!(metas[1].id, "s-inner");
        assert_eq!(metas[1].group, "g-s-inner");
        assert_eq!(metas[1].z_order, 1);
    }

    #[test]
    fn test_nested_groups_serialization() {
        let inner_node = Node {
            id: "s-inner".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 5.0,
                cy: 5.0,
                r: 3.0,
            }),
        };
        let outer_node = Node {
            id: "s-outer".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 0.0,
                cy: 0.0,
                r: 10.0,
            }),
        };
        let inner_group = Group {
            id: "g-s-inner".to_string(),
            nodes: vec![inner_node],
            groups: vec![],
        };
        let outer_group = Group {
            id: "g-s-outer".to_string(),
            nodes: vec![outer_node],
            groups: vec![inner_group],
        };
        let scene = SceneGraph {
            groups: vec![outer_group],
        };

        let opts = make_test_options();
        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(svg.contains("<g id=\"g-s-outer\">"));
        assert!(svg.contains("<g id=\"g-s-inner\">"));

        let outer_open = svg.find("<g id=\"g-s-outer\">").unwrap();
        let inner_open = svg.find("<g id=\"g-s-inner\">").unwrap();
        let outer_close = svg.rfind("</svg>").unwrap();

        assert!(
            outer_open < inner_open,
            "outer group must open before inner"
        );
        assert!(
            inner_open < outer_close,
            "inner group must close before svg end"
        );

        let outer_nodes = svg.find("id=\"s-outer\"").unwrap();
        let _inner_nodes = svg.find("id=\"s-inner\"").unwrap();
        assert!(
            outer_open < outer_nodes,
            "outer node after outer group open"
        );
        assert!(outer_nodes < inner_open, "outer node before inner group");
    }

    #[test]
    fn test_nested_group_wrapper_no_nodes() {
        let inner_node = Node {
            id: "s-leaf".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 255, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 2.0,
                cy: 2.0,
                r: 1.0,
            }),
        };
        let leaf_group = Group {
            id: "g-s-leaf".to_string(),
            nodes: vec![inner_node],
            groups: vec![],
        };
        // g-mask wrapper with no nodes, one child group
        let mask_group = Group {
            id: "g-mask-A".to_string(),
            nodes: vec![],
            groups: vec![leaf_group],
        };
        let scene = SceneGraph {
            groups: vec![mask_group],
        };

        let opts = make_test_options();
        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        // mask wrapper must be emitted even though it has no nodes
        assert!(
            svg.contains("<g id=\"g-mask-A\">"),
            "mask wrapper must be present"
        );
        assert!(
            svg.contains("<g id=\"g-s-leaf\">"),
            "leaf group must be nested"
        );

        let mask_open = svg.find("<g id=\"g-mask-A\">").unwrap();
        let leaf_open = svg.find("<g id=\"g-s-leaf\">").unwrap();
        assert!(
            mask_open < leaf_open,
            "mask wrapper opens before leaf group"
        );
    }

    #[test]
    fn test_flat_scene_unchanged_via_build_node_metas() {
        let node1 = Node {
            id: "s-0".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 5.0,
                translate_y: 10.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 0.0,
                cy: 0.0,
                r: 3.0,
            }),
        };
        let node2 = Node {
            id: "s-1".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::ClosePath,
            ]),
        };
        let group1 = Group {
            id: "g-s-0".to_string(),
            nodes: vec![node1],
            groups: vec![],
        };
        let group2 = Group {
            id: "g-s-1".to_string(),
            nodes: vec![node2],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group1, group2],
        };

        let opts = make_test_options();
        let res_old = emit_svg(&scene, 100, 100, &opts, None);

        // Verify meta structure: build_node_metas gives the same result as before
        let metas = build_node_metas(&scene, false);
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].id, "s-0");
        assert_eq!(metas[0].group, "g-s-0");
        assert_eq!(metas[0].z_order, 0);
        assert_eq!(metas[1].id, "s-1");
        assert_eq!(metas[1].group, "g-s-1");
        assert_eq!(metas[1].z_order, 1);

        // Verify SVG output is deterministic
        let res_new = emit_svg(&scene, 100, 100, &opts, None);
        assert_eq!(res_old.svg, res_new.svg);
    }

    #[test]
    fn test_deep_nesting_four_levels() {
        fn make_node(id: &str) -> Node {
            Node {
                id: id.to_string(),
                fill: Some(Fill::Solid(Rgb {
                    r: 128,
                    g: 128,
                    b: 128,
                })),
                stroke: None,
                transform: Transform {
                    translate_x: 0.0,
                    translate_y: 0.0,
                },
                shape: Shape::Primitive(Primitive::Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                    rx: None,
                    ry: None,
                }),
            }
        }
        fn make_group(id: &str, node: Node, children: Vec<Group>) -> Group {
            Group {
                id: id.to_string(),
                nodes: vec![node],
                groups: children,
            }
        }

        let d = make_group("g-d", make_node("s-d"), vec![]);
        let c = make_group("g-c", make_node("s-c"), vec![d]);
        let b = make_group("g-b", make_node("s-b"), vec![c]);
        let a = make_group("g-a", make_node("s-a"), vec![b]);
        let scene = SceneGraph { groups: vec![a] };

        let metas = build_node_metas(&scene, false);
        assert_eq!(metas.len(), 4);
        assert_eq!(metas[0].id, "s-a");
        assert_eq!(metas[0].group, "g-a");
        assert_eq!(metas[1].id, "s-b");
        assert_eq!(metas[1].group, "g-b");
        assert_eq!(metas[2].id, "s-c");
        assert_eq!(metas[2].group, "g-c");
        assert_eq!(metas[3].id, "s-d");
        assert_eq!(metas[3].group, "g-d");

        let opts = make_test_options();
        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        // Verify correct nesting depth
        let idx_a_open = svg.find("<g id=\"g-a\">").unwrap();
        let idx_a_close = svg.rfind("</g>").unwrap();
        let idx_b = svg.find("<g id=\"g-b\">").unwrap();
        let idx_c = svg.find("<g id=\"g-c\">").unwrap();
        let idx_d = svg.find("<g id=\"g-d\">").unwrap();
        assert!(
            idx_a_open < idx_b && idx_b < idx_c && idx_c < idx_d,
            "groups must be nested in order a<b<c<d"
        );
        assert!(
            idx_d < idx_a_close,
            "innermost group must close before outermost"
        );
    }

    #[test]
    fn test_fill_emit_css_draw_grouped() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(30.0, 20.0),
                PathElement::LineTo(30.0, 30.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;
        opts.emit_css = Some(Preset::Draw);
        opts.output = OutputFormat::SvgPretty;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(svg.contains("<style>"), "should contain style tag");
        assert!(
            svg.contains("@keyframes sc-draw"),
            "should contain keyframes"
        );
        assert!(
            svg.contains("stroke-dasharray: 100;"),
            "should contain stroke-dasharray"
        );
        assert!(
            svg.contains("pathLength=\"100\""),
            "should contain pathLength"
        );
        assert!(
            svg.contains("animation-delay: 0ms;"),
            "should have delay for first node"
        );
        assert!(
            svg.contains("animation-delay: 100ms;"),
            "should have delay for second node"
        );
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "should preserve original fill"
        );
        assert!(
            svg.contains("fill=\"#0000ff\""),
            "should preserve original fill"
        );
    }

    #[test]
    fn test_fill_emit_css_draw_flat_grouping() {
        let curve1 = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curve2 = Curve {
            segments: vec![
                PathElement::MoveTo(20.0, 20.0),
                PathElement::LineTo(30.0, 20.0),
                PathElement::LineTo(30.0, 30.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve1, curve2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;
        opts.grouping = Grouping::Flat;
        opts.emit_css = Some(Preset::Draw);
        opts.output = OutputFormat::SvgPretty;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(svg.contains("<style>"), "should contain style tag");
        assert!(
            svg.contains("@keyframes sc-draw"),
            "should contain keyframes"
        );
        assert!(
            svg.contains("stroke-dasharray: 100;"),
            "should contain stroke-dasharray"
        );
        assert!(
            svg.contains("pathLength=\"100\""),
            "should contain pathLength"
        );
        assert!(
            svg.contains("animation-delay: 0ms;"),
            "should have delay for first node"
        );
        assert!(
            svg.contains("animation-delay: 100ms;"),
            "should have delay for second node"
        );
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "should preserve original fill"
        );
        assert!(
            svg.contains("fill=\"#0000ff\""),
            "should preserve original fill"
        );
    }

    #[test]
    fn test_fill_emit_css_none_unchanged() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];
        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        let mut opts = make_test_options();
        opts.id_style = IdStyle::Sequential;

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(!svg.contains("<style"), "should NOT contain style tag");
        assert!(!svg.contains("@keyframes"), "should NOT contain keyframes");
        assert!(
            !svg.contains("stroke-dasharray"),
            "should NOT contain stroke-dasharray in CSS"
        );
        assert!(
            !svg.contains("pathLength=\"100\""),
            "should NOT contain pathLength"
        );
        // Should NOT have an added stroke attribute
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "should contain original fill"
        );
    }

    #[test]
    fn test_fill_emit_css_draw_no_ids_is_noop() {
        let curve = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![curve],
        };
        let fills = vec![Fill::Solid(Rgb { r: 255, g: 0, b: 0 })];
        let scene = build_scene_graph(&curves, &IdStyle::None, &TOrigin::Baked, &fills, false);

        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;
        opts.emit_css = Some(Preset::Draw);

        let res = emit_svg(&scene, 100, 100, &opts, None);
        let svg = &res.svg;

        assert!(
            !svg.contains("<style"),
            "should NOT contain style tag when no ids present"
        );
        assert!(!svg.contains("@keyframes"), "should NOT contain keyframes");
    }

    #[test]
    fn test_build_scene_graph_drops_zero_area_curve() {
        // A normal triangle with area 50.0
        let normal = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // A degenerate curve with area 0.0 (single point + ClosePath)
        let degenerate = Curve {
            segments: vec![PathElement::MoveTo(5.0, 5.0), PathElement::ClosePath],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![normal, degenerate],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        assert_eq!(
            scene.groups.len(),
            1,
            "only the normal curve should survive"
        );
        // The surviving node should be the non-degenerate one (index 0, "s-0")
        assert_eq!(scene.groups[0].nodes[0].id, "s-0");
    }

    #[test]
    fn test_build_scene_graph_keeps_tiny_but_real_area() {
        // A small 2x2 square with area 4.0 (well above epsilon)
        let tiny = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(2.0, 0.0),
                PathElement::LineTo(2.0, 2.0),
                PathElement::LineTo(0.0, 2.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        // A normal triangle with area 50.0
        let normal = Curve {
            segments: vec![
                PathElement::MoveTo(10.0, 10.0),
                PathElement::LineTo(20.0, 10.0),
                PathElement::LineTo(20.0, 20.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![tiny, normal],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        assert_eq!(
            scene.groups.len(),
            2,
            "both tiny-but-real and normal shapes should survive"
        );
    }

    #[test]
    fn test_build_scene_graph_all_degenerate_produces_empty_scene() {
        let deg1 = Curve {
            segments: vec![PathElement::MoveTo(1.0, 1.0), PathElement::ClosePath],
            primitive: None,
        };
        let deg2 = Curve {
            segments: vec![
                PathElement::MoveTo(2.0, 2.0),
                PathElement::LineTo(3.0, 3.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let curves = CurveSet {
            curves: vec![deg1, deg2],
        };
        let fills = vec![
            Fill::Solid(Rgb { r: 255, g: 0, b: 0 }),
            Fill::Solid(Rgb { r: 0, g: 0, b: 255 }),
        ];

        let scene = build_scene_graph(
            &curves,
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &fills,
            false,
        );

        assert!(
            scene.groups.is_empty(),
            "all-degenerate input should produce empty scene"
        );
    }

    #[test]
    fn test_stroke_gradient_emits_defs_and_url_reference() {
        let gradient = Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        };
        let node = Node {
            id: "test-node".to_string(),
            fill: None,
            stroke: Some(Stroke {
                color: Rgb { r: 0, g: 0, b: 0 },
                width: 2.0,
                paint: Some(gradient),
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(100.0, 100.0),
            ]),
        };
        let group = Group {
            id: "g-test-node".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let opts = make_test_options();
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        assert!(svg.contains("<defs>"), "should contain defs block");
        assert!(
            svg.contains(
                "<linearGradient id=\"strokegrad-test-node\" gradientUnits=\"userSpaceOnUse\""
            ),
            "should contain linearGradient with strokegrad- id"
        );
        assert!(
            svg.contains("stroke=\"url(#strokegrad-test-node)\""),
            "should reference gradient via url"
        );

        // Both substrings existing is not enough -- they could name different
        // ids and the reference would dangle. Pull the id out of each and
        // compare the extracted values.
        let defs_id = svg
            .split("<linearGradient id=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("no linearGradient id in output");
        let ref_id = svg
            .split("stroke=\"url(#")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .expect("no url(#..) stroke reference in output");
        assert_eq!(
            defs_id, ref_id,
            "stroke references #{ref_id} but defs declares #{defs_id}"
        );
    }

    #[test]
    fn test_stroke_solid_paint_emits_no_defs() {
        let node = Node {
            id: "solid-node".to_string(),
            fill: None,
            stroke: Some(Stroke {
                color: Rgb { r: 255, g: 0, b: 0 },
                width: 1.0,
                paint: None,
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
            ]),
        };
        let group = Group {
            id: "g-solid-node".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let opts = make_test_options();
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        assert!(
            !svg.contains("<defs>"),
            "should not contain defs for solid paint"
        );
        assert!(!svg.contains("url(#"), "should not contain url() reference");
        assert!(
            svg.contains("stroke=\"#ff0000\""),
            "should contain flat stroke color"
        );
    }

    #[test]
    fn test_stroke_gradient_falls_back_to_flat_when_ids_disabled() {
        let gradient = Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 255, b: 0 },
                },
            ],
        };
        let node = Node {
            id: "".to_string(),
            fill: None,
            stroke: Some(Stroke {
                color: Rgb {
                    r: 128,
                    g: 128,
                    b: 128,
                },
                width: 3.0,
                paint: Some(gradient),
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(10.0, 10.0),
            ]),
        };
        let group = Group {
            id: "".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        assert!(
            !svg.contains("<defs>"),
            "should not emit defs when ids disabled"
        );
        assert!(
            !svg.contains("url(#"),
            "should not emit url() reference when ids disabled"
        );
        assert!(
            svg.contains("stroke=\"#808080\""),
            "should fall back to flat hex color when id is empty"
        );
    }

    #[test]
    fn test_fill_and_stroke_gradients_get_distinct_ids() {
        let fill_gradient = Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 50.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        };
        let stroke_gradient = Fill::RadialGradient {
            cx: 50.0,
            cy: 50.0,
            r: 50.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 0, g: 255, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb {
                        r: 255,
                        g: 255,
                        b: 0,
                    },
                },
            ],
        };
        let node = Node {
            id: "dual-node".to_string(),
            fill: Some(fill_gradient),
            stroke: Some(Stroke {
                color: Rgb { r: 0, g: 0, b: 0 },
                width: 2.0,
                paint: Some(stroke_gradient),
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 50.0,
                cy: 50.0,
                r: 30.0,
            }),
        };
        let node_none = Node {
            id: "none-node".to_string(),
            fill: None,
            stroke: Some(Stroke {
                color: Rgb { r: 0, g: 0, b: 0 },
                width: 1.0,
                paint: None,
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 10.0,
                cy: 10.0,
                r: 5.0,
            }),
        };
        let group = Group {
            id: "g-dual-node".to_string(),
            nodes: vec![node, node_none],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let opts = make_test_options();
        // Use emit_stroke_svg which has been updated for stroke gradient paint support.
        // Both fill and stroke gradients are collected for defs even in stroke mode.
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        assert!(
            svg.contains("id=\"grad-dual-node\""),
            "should contain fill gradient def"
        );
        assert!(
            svg.contains("id=\"strokegrad-dual-node\""),
            "should contain stroke gradient def"
        );
        assert!(
            svg.contains("stroke=\"url(#strokegrad-dual-node)\""),
            "stroke should reference strokegrad- id"
        );
        assert!(
            svg.contains("fill=\"url(#grad-dual-node)\""),
            "stroke mode sets fill attribute for nodes with fill gradient"
        );
        assert!(
            svg.contains("fill=\"none\""),
            "stroke mode sets fill=\"none\" for nodes whose fill is None"
        );
    }

    #[test]
    fn test_stroke_scene_honors_solid_fill() {
        let node_filled = Node {
            id: "filled-node".to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 10.0,
                cy: 10.0,
                r: 5.0,
            }),
        };
        let node_stroked = Node {
            id: "stroked-node".to_string(),
            fill: None,
            stroke: Some(Stroke {
                color: Rgb { r: 0, g: 0, b: 0 },
                width: 1.0,
                paint: None,
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 20.0,
                cy: 20.0,
                r: 5.0,
            }),
        };
        let group = Group {
            id: "g1".to_string(),
            nodes: vec![node_filled, node_stroked],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let opts = make_test_options();
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        assert!(svg.contains("fill=\"#ff0000\""));
        assert!(svg.contains("fill=\"none\""));
        assert!(!svg.contains("<defs>"));
        assert!(!svg.contains("<linearGradient"));
    }

    #[test]
    fn test_stroke_scene_honors_linear_gradient_fill_with_id() {
        let grad = Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        };
        let node = Node {
            id: "grad-node".to_string(),
            fill: Some(grad),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 10.0,
                cy: 10.0,
                r: 5.0,
            }),
        };
        let group = Group {
            id: "g1".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let opts = make_test_options();
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        let def_id_prefix = "<linearGradient id=\"";
        let def_start = svg
            .find(def_id_prefix)
            .expect("should contain linearGradient def");
        let id_start = def_start + def_id_prefix.len();
        let id_end = svg[id_start..].find('"').expect("closing quote") + id_start;
        let def_id = &svg[id_start..id_end];

        let ref_prefix = "fill=\"url(#";
        let ref_start = svg.find(ref_prefix).expect("should contain url reference");
        let ref_id_start = ref_start + ref_prefix.len();
        let ref_id_end = svg[ref_id_start..].find(')').expect("closing paren") + ref_id_start;
        let ref_id = &svg[ref_id_start..ref_id_end];

        assert_eq!(def_id, ref_id);
        assert_eq!(def_id, "grad-grad-node");
        assert!(svg.contains("gradientUnits=\"userSpaceOnUse\""));
    }

    #[test]
    fn test_stroke_scene_linear_gradient_fill_empty_id_fallback() {
        let grad = Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb {
                        r: 128,
                        g: 64,
                        b: 32,
                    },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        };
        let node = Node {
            id: "".to_string(),
            fill: Some(grad),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 10.0,
                cy: 10.0,
                r: 5.0,
            }),
        };
        let group = Group {
            id: "".to_string(),
            nodes: vec![node],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let mut opts = make_test_options();
        opts.id_style = IdStyle::None;
        let res = emit_stroke_svg(&scene, 100, 100, &opts);
        let svg = &res.svg;

        assert!(!svg.contains("<linearGradient"));
        assert!(!svg.contains("url(#"));
        assert!(svg.contains("fill=\"#804020\""));
    }

    #[test]
    fn test_stroke_scene_fill_serialization_determinism() {
        let grad = Fill::LinearGradient {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 0.0,
            stops: vec![
                GradientStop {
                    offset: 0.0,
                    color: Rgb { r: 255, g: 0, b: 0 },
                },
                GradientStop {
                    offset: 1.0,
                    color: Rgb { r: 0, g: 0, b: 255 },
                },
            ],
        };
        let node1 = Node {
            id: "node-1".to_string(),
            fill: Some(grad),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Circle {
                cx: 10.0,
                cy: 10.0,
                r: 5.0,
            }),
        };
        let node2 = Node {
            id: "node-2".to_string(),
            fill: Some(Fill::Solid(Rgb {
                r: 10,
                g: 20,
                b: 30,
            })),
            stroke: Some(Stroke {
                color: Rgb { r: 0, g: 0, b: 0 },
                width: 1.5,
                paint: None,
            }),
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Primitive(Primitive::Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
                rx: None,
                ry: None,
            }),
        };
        let group = Group {
            id: "g-main".to_string(),
            nodes: vec![node1, node2],
            groups: vec![],
        };
        let scene = SceneGraph {
            groups: vec![group],
        };
        let opts = make_test_options();

        let res1 = emit_stroke_svg(&scene, 100, 100, &opts);
        let res2 = emit_stroke_svg(&scene, 100, 100, &opts);

        assert_eq!(res1.svg, res2.svg);
    }

    // ── Stable IDs through the scene graph (#7) ────────────────────────

    fn tri(x: f64, y: f64) -> Curve {
        Curve {
            segments: vec![
                PathElement::MoveTo(x, y),
                PathElement::LineTo(x + 10.0, y),
                PathElement::CurveTo(x + 10.0, y + 5.0, x + 5.0, y + 10.0, x, y + 10.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        }
    }

    fn ids_of(scene: &SceneGraph) -> Vec<String> {
        scene
            .groups
            .iter()
            .flat_map(|g| g.nodes.iter().map(|n| n.id.clone()))
            .collect()
    }

    fn hash_scene(curves: Vec<Curve>, fills: Vec<Fill>) -> SceneGraph {
        build_scene_graph(
            &CurveSet { curves },
            &IdStyle::Hash,
            &TOrigin::Baked,
            &fills,
            false,
        )
    }

    fn red() -> Fill {
        Fill::Solid(Rgb { r: 255, g: 0, b: 0 })
    }

    fn blue() -> Fill {
        Fill::Solid(Rgb { r: 0, g: 0, b: 255 })
    }

    /// Inserting a shape in front of another must not rename the one that
    /// did not change. Before #7 the ID hashed the paint-array index, so
    /// every shape after the insertion point was renamed.
    #[test]
    fn inserting_a_shape_does_not_rename_the_others() {
        let before = hash_scene(vec![tri(0.0, 0.0), tri(40.0, 40.0)], vec![red(), blue()]);
        let after = hash_scene(
            vec![tri(80.0, 80.0), tri(0.0, 0.0), tri(40.0, 40.0)],
            vec![Fill::Solid(Rgb { r: 0, g: 255, b: 0 }), red(), blue()],
        );

        let before_ids = ids_of(&before);
        let after_ids = ids_of(&after);

        for id in &before_ids {
            assert!(
                after_ids.contains(id),
                "ID {id} disappeared after an unrelated shape was inserted \
                 ahead of it; before={before_ids:?} after={after_ids:?}"
            );
        }
    }

    /// Reordering two shapes must swap their positions, not their names.
    #[test]
    fn reordering_shapes_does_not_rename_them() {
        let forward = ids_of(&hash_scene(
            vec![tri(0.0, 0.0), tri(40.0, 40.0)],
            vec![red(), blue()],
        ));
        let reversed = ids_of(&hash_scene(
            vec![tri(40.0, 40.0), tri(0.0, 0.0)],
            vec![blue(), red()],
        ));

        let mut a = forward.clone();
        let mut b = reversed.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b, "reordering changed the set of IDs");
    }

    /// Changing only a control point is a real geometry change and must
    /// produce a new ID — endpoints alone are not the identity.
    #[test]
    fn changing_only_a_control_point_changes_the_id() {
        let base = tri(0.0, 0.0);
        let mut bent = base.clone();
        bent.segments[2] = PathElement::CurveTo(20.0, 1.0, 5.0, 10.0, 0.0, 10.0);

        let a = ids_of(&hash_scene(vec![base], vec![red()]));
        let b = ids_of(&hash_scene(vec![bent], vec![red()]));
        assert_ne!(a, b, "a moved control point must change the ID");
    }

    #[test]
    fn changing_only_the_fill_changes_the_id() {
        let a = ids_of(&hash_scene(vec![tri(0.0, 0.0)], vec![red()]));
        let b = ids_of(&hash_scene(vec![tri(0.0, 0.0)], vec![blue()]));
        assert_ne!(a, b, "a recoloured shape must change the ID");
    }

    // ── Exact path geometry reaches the scene graph (#8) ───────────────

    /// A shape whose curves bulge well past their endpoints must report a
    /// bbox that covers the bulge. The endpoint hull would report a flat
    /// box of zero height.
    #[test]
    fn metadata_bbox_covers_curve_extrema() {
        let lens = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::CurveTo(0.0, 40.0, 100.0, 40.0, 100.0, 0.0),
                PathElement::CurveTo(100.0, -40.0, 0.0, -40.0, 0.0, 0.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let scene = build_scene_graph(
            &CurveSet {
                curves: vec![lens],
            },
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[red()],
            false,
        );

        let metas = build_node_metas(&scene, false);
        let node = &metas[0];
        assert!(
            node.bbox.y_max > 20.0 && node.bbox.y_min < -20.0,
            "bbox must cover the bulge, got {:?}",
            node.bbox
        );
        assert!(
            node.area > 1000.0,
            "a lens encloses real area, got {}",
            node.area
        );
    }

    /// A curved shape whose endpoints are collinear has zero *polygon*
    /// area, so it used to be dropped as degenerate even though it
    /// encloses ink.
    #[test]
    fn a_bulging_shape_is_not_filtered_as_degenerate() {
        let lens = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 50.0),
                PathElement::CurveTo(0.0, 90.0, 100.0, 90.0, 100.0, 50.0),
                PathElement::CurveTo(100.0, 10.0, 0.0, 10.0, 0.0, 50.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let scene = build_scene_graph(
            &CurveSet {
                curves: vec![lens],
            },
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[red()],
            false,
        );
        assert_eq!(
            scene.groups.len(),
            1,
            "a curved shape with collinear endpoints must survive"
        );
    }

    /// Two subpaths must be measured as a ring and its hole, not joined
    /// end-to-end into one nonsense polygon.
    #[test]
    fn a_hole_is_subtracted_from_reported_area() {
        let ring = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::LineTo(100.0, 0.0),
                PathElement::LineTo(100.0, 100.0),
                PathElement::LineTo(0.0, 100.0),
                PathElement::ClosePath,
                PathElement::MoveTo(25.0, 25.0),
                PathElement::LineTo(75.0, 25.0),
                PathElement::LineTo(75.0, 75.0),
                PathElement::LineTo(25.0, 75.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let scene = build_scene_graph(
            &CurveSet {
                curves: vec![ring],
            },
            &IdStyle::Sequential,
            &TOrigin::Baked,
            &[red()],
            false,
        );

        let area = build_node_metas(&scene, false)[0].area;
        assert!(
            (area - 7500.0).abs() < 1.0,
            "expected 10000 - 2500 = 7500, got {area}"
        );
    }

    /// The centroid transform origin must come from the real enclosed
    /// area, so a curved shape's origin sits inside its ink.
    #[test]
    fn centroid_origin_uses_exact_area() {
        // A quarter-disc-ish wedge: endpoints alone would put the centroid
        // at the chord midpoint, not inside the filled region.
        let wedge = Curve {
            segments: vec![
                PathElement::MoveTo(0.0, 0.0),
                PathElement::CurveTo(0.0, 80.0, 80.0, 80.0, 80.0, 0.0),
                PathElement::ClosePath,
            ],
            primitive: None,
        };
        let scene = build_scene_graph(
            &CurveSet {
                curves: vec![wedge],
            },
            &IdStyle::Sequential,
            &TOrigin::Centroid,
            &[red()],
            false,
        );

        let t = &scene.groups[0].nodes[0].transform;
        assert!(
            t.translate_y > 5.0,
            "the centroid must sit inside the bulge, not on the chord \
             (y = 0); got {}",
            t.translate_y
        );
        assert!(
            (t.translate_x - 40.0).abs() < 1.0,
            "the wedge is symmetric about x = 40; got {}",
            t.translate_x
        );
    }

    /// Two identical shapes are legitimately the same content; they collide
    /// and are separated deterministically rather than by position.
    #[test]
    fn duplicate_identical_shapes_get_deterministic_suffixes() {
        let ids = ids_of(&hash_scene(
            vec![tri(0.0, 0.0), tri(0.0, 0.0), tri(0.0, 0.0)],
            vec![red(), red(), red()],
        ));

        assert_eq!(ids.len(), 3);
        assert_eq!(ids[1], format!("{}-2", ids[0]));
        assert_eq!(ids[2], format!("{}-3", ids[0]));
    }
