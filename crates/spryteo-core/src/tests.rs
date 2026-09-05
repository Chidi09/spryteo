use crate::ir::*;
use crate::options::*;

#[test]
fn default_options() {
    let opts = ConvertOptions::default();

    assert!(matches!(opts.mode, Mode::Auto));
    assert!(!opts.stroke);
    assert!(matches!(opts.colors, ColorSpec::Auto));
    assert!(matches!(opts.layering, Layering::Stacked));
    assert!((opts.tolerance - 0.5).abs() < f32::EPSILON);
    assert!((opts.smoothness - 1.0).abs() < f32::EPSILON);
    assert_eq!(opts.turdsize, 2);
    assert!(matches!(opts.gradients, Tri::Auto));
    assert!(matches!(opts.grouping, Grouping::Component));
    assert!(matches!(opts.id_style, IdStyle::Hash));
    assert!(matches!(opts.transform_origin, TOrigin::Centroid));
    assert_eq!(opts.precision, 2);
    assert!(opts.max_trace_dimension.is_none());
    assert!(matches!(opts.background, Background::Keep));
    assert!(matches!(opts.alpha_mode, AlphaMode::Keep));
    assert!(!opts.arcs);
    assert!(!opts.current_color);
    assert!(matches!(opts.output, OutputFormat::Svg));
    assert!(opts.emit_css.is_none());
    assert_eq!(opts.max_pixels, 16_000_000);
    assert_eq!(opts.max_input_bytes, 8_000_000);
    assert!(opts.timeout_ms.is_none());
}

#[test]
fn convert_options_serde_round_trip() {
    let opts = ConvertOptions {
        mode: Mode::Icon,
        stroke: true,
        colors: ColorSpec::N(8),
        layering: Layering::Cutout,
        tolerance: 1.0,
        smoothness: 0.5,
        turdsize: 4,
        gradients: Tri::Off,
        grouping: Grouping::Flat,
        id_style: IdStyle::Sequential,
        transform_origin: TOrigin::Baked,
        precision: 1,
        max_trace_dimension: Some(1024),
        background: Background::Drop,
        alpha_mode: AlphaMode::Matte(Rgb {
            r: 255,
            g: 255,
            b: 255,
        }),
        arcs: true,
        current_color: true,
        output: OutputFormat::SvgPretty,
        emit_css: Some(Preset::Draw),
        max_pixels: 4_000_000,
        max_input_bytes: 2_000_000,
        timeout_ms: Some(30_000),
    };

    let json = serde_json::to_string(&opts).unwrap();
    let deserialized: ConvertOptions = serde_json::from_str(&json).unwrap();

    // Can't use assert_eq! directly because ColorSpec doesn't impl PartialEq
    // (Palette variant with Vec<Rgb>).  Check fields individually.
    assert!(matches!(deserialized.mode, Mode::Icon));
    assert!(deserialized.stroke);
    assert!(matches!(deserialized.colors, ColorSpec::N(8)));
    assert!(matches!(deserialized.layering, Layering::Cutout));
    assert!((deserialized.tolerance - 1.0).abs() < f32::EPSILON);
    assert!((deserialized.smoothness - 0.5).abs() < f32::EPSILON);
    assert_eq!(deserialized.turdsize, 4);
    assert!(matches!(deserialized.gradients, Tri::Off));
    assert!(matches!(deserialized.grouping, Grouping::Flat));
    assert!(matches!(deserialized.id_style, IdStyle::Sequential));
    assert!(matches!(deserialized.transform_origin, TOrigin::Baked));
    assert_eq!(deserialized.precision, 1);
    assert_eq!(deserialized.max_trace_dimension, Some(1024));
    assert!(matches!(deserialized.background, Background::Drop));
    assert!(matches!(
        deserialized.alpha_mode,
        AlphaMode::Matte(Rgb {
            r: 255,
            g: 255,
            b: 255
        })
    ));
    assert!(deserialized.arcs);
    assert!(deserialized.current_color);
    assert!(matches!(deserialized.output, OutputFormat::SvgPretty));
    assert!(matches!(deserialized.emit_css, Some(Preset::Draw)));
    assert_eq!(deserialized.max_pixels, 4_000_000);
    assert_eq!(deserialized.max_input_bytes, 2_000_000);
    assert_eq!(deserialized.timeout_ms, Some(30_000));
}

#[test]
fn scene_graph_serde_round_trip() {
    let sg = SceneGraph {
        groups: vec![Group {
            id: "g-root".into(),
            nodes: vec![Node {
                id: "s-abc".into(),
                fill: Some(Fill::Solid(Rgb {
                    r: 0,
                    g: 128,
                    b: 255,
                })),
                stroke: Some(Stroke {
                    color: Rgb { r: 0, g: 0, b: 0 },
                    width: 1.5,
                    paint: None,
                }),
                transform: Transform {
                    translate_x: 10.0,
                    translate_y: 20.0,
                },
                shape: Shape::Path(vec![
                    PathElement::MoveTo(0.0, 0.0),
                    PathElement::LineTo(100.0, 0.0),
                    PathElement::CurveTo(100.0, 50.0, 50.0, 100.0, 0.0, 100.0),
                    PathElement::ClosePath,
                ]),
            }],
            groups: vec![Group {
                id: "g-child".into(),
                nodes: vec![Node {
                    id: "s-def".into(),
                    fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
                    stroke: None,
                    transform: Transform {
                        translate_x: 0.0,
                        translate_y: 0.0,
                    },
                    shape: Shape::Primitive(Primitive::Circle {
                        cx: 50.0,
                        cy: 50.0,
                        r: 25.0,
                    }),
                }],
                groups: vec![],
            }],
        }],
    };

    let json = serde_json::to_string(&sg).unwrap();
    let deserialized: SceneGraph = serde_json::from_str(&json).unwrap();

    // Spot-check structure
    assert_eq!(deserialized.groups.len(), 1);
    assert_eq!(deserialized.groups[0].id, "g-root");
    assert_eq!(deserialized.groups[0].nodes.len(), 1);
    assert_eq!(deserialized.groups[0].groups.len(), 1);
    assert_eq!(deserialized.groups[0].groups[0].id, "g-child");

    // Confirm primitive survived
    let child_node = &deserialized.groups[0].groups[0].nodes[0];
    assert!(matches!(
        child_node.shape,
        Shape::Primitive(Primitive::Circle { .. })
    ));
    if let Shape::Primitive(Primitive::Circle { cx, cy, r }) = child_node.shape {
        assert!((cx - 50.0).abs() < 1e-10);
        assert!((cy - 50.0).abs() < 1e-10);
        assert!((r - 25.0).abs() < 1e-10);
    }
}

#[test]
fn meta_serde_round_trip() {
    let meta = Meta {
        nodes: vec![
            NodeMeta {
                id: "s-abc".into(),
                bbox: Bbox {
                    x_min: 0.0,
                    y_min: 0.0,
                    x_max: 100.0,
                    y_max: 100.0,
                },
                centroid: (50.0, 50.0),
                area: 10000.0,
                fill: Some(Rgb {
                    r: 0,
                    g: 128,
                    b: 255,
                }),
                group: "g-root".into(),
                z_order: 1,
                suggested_draw_order: 0,
            },
            NodeMeta {
                id: "s-def".into(),
                bbox: Bbox {
                    x_min: 25.0,
                    y_min: 25.0,
                    x_max: 75.0,
                    y_max: 75.0,
                },
                centroid: (50.0, 50.0),
                area: 1963.5,
                fill: Some(Rgb { r: 255, g: 0, b: 0 }),
                group: "g-child".into(),
                z_order: 2,
                suggested_draw_order: 1,
            },
        ],
        groups: Vec::new(),
        stats: Stats {
            node_count: 2,
            path_count: 1,
            byte_count: 4096,
        },
        current_color_applied: true,
    };

    let json = serde_json::to_string(&meta).unwrap();
    let deserialized: Meta = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.nodes.len(), 2);
    assert_eq!(deserialized.nodes[0].id, "s-abc");
    assert_eq!(deserialized.nodes[1].id, "s-def");
    assert_eq!(deserialized.stats.node_count, 2);
    assert_eq!(deserialized.stats.path_count, 1);
    assert_eq!(deserialized.stats.byte_count, 4096);
    assert!(deserialized.current_color_applied);
}

#[test]
fn meta_deserialization_defaults() {
    let json = r#"{
        "nodes": [],
        "stats": {
            "node_count": 0,
            "path_count": 0,
            "byte_count": 0
        }
    }"#;
    let deserialized: Meta = serde_json::from_str(json).unwrap();
    assert!(!deserialized.current_color_applied);
}

#[test]
fn scene_graph_deterministic_serialization() {
    let sg = SceneGraph {
        groups: vec![Group {
            id: "g-det".into(),
            nodes: vec![
                Node {
                    id: "s-1".into(),
                    fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
                    stroke: None,
                    transform: Transform {
                        translate_x: 0.0,
                        translate_y: 0.0,
                    },
                    shape: Shape::Path(vec![
                        PathElement::MoveTo(0.0, 0.0),
                        PathElement::LineTo(10.0, 0.0),
                        PathElement::LineTo(10.0, 10.0),
                        PathElement::ClosePath,
                    ]),
                },
                Node {
                    id: "s-2".into(),
                    fill: Some(Fill::Solid(Rgb { r: 0, g: 0, b: 255 })),
                    stroke: None,
                    transform: Transform {
                        translate_x: 5.0,
                        translate_y: 5.0,
                    },
                    shape: Shape::Path(vec![
                        PathElement::MoveTo(0.0, 0.0),
                        PathElement::LineTo(5.0, 0.0),
                        PathElement::LineTo(5.0, 5.0),
                        PathElement::ClosePath,
                    ]),
                },
            ],
            groups: vec![],
        }],
    };

    let json_a = serde_json::to_string(&sg).unwrap();
    let json_b = serde_json::to_string(&sg).unwrap();

    assert_eq!(json_a, json_b);

    // Also prove that serializing a constructed-via-code instance twice
    // gives identical bytes (no HashMap iteration order shenanigans).
    let bytes_a = serde_json::to_vec(&sg).unwrap();
    let bytes_b = serde_json::to_vec(&sg).unwrap();
    assert_eq!(bytes_a, bytes_b);
}

#[test]
fn test_camel_case_alias() {
    let mut default_val = serde_json::to_value(ConvertOptions::default()).unwrap();
    if let Some(obj) = default_val.as_object_mut() {
        obj.insert("currentColor".to_string(), serde_json::Value::Bool(true));
        obj.remove("current_color");
    }
    let deserialized: ConvertOptions = serde_json::from_value(default_val).unwrap();
    assert!(deserialized.current_color);
}

#[test]
fn test_stroke_paint_defaults_to_none_and_roundtrips() {
    // 1) A Stroke with paint: None should serialize to JSON without "paint" key.
    let stroke = Stroke {
        color: Rgb { r: 0, g: 0, b: 0 },
        width: 2.0,
        paint: None,
    };
    let json = serde_json::to_string(&stroke).unwrap();
    assert!(
        !json.contains("\"paint\""),
        "JSON should not contain 'paint' key: {json}"
    );

    // 2) Deserializing that JSON should produce paint: None (default works).
    let restored: Stroke = serde_json::from_str(&json).unwrap();
    assert!(restored.paint.is_none());

    // 3) A Stroke with paint: Some(LinearGradient) round-trips.
    let stops = vec![
        GradientStop {
            offset: 0.0,
            color: Rgb { r: 255, g: 0, b: 0 },
        },
        GradientStop {
            offset: 1.0,
            color: Rgb { r: 0, g: 0, b: 255 },
        },
    ];
    let gradient = Fill::LinearGradient {
        x1: 0.0,
        y1: 0.0,
        x2: 100.0,
        y2: 0.0,
        stops: stops.clone(),
    };
    let stroke2 = Stroke {
        color: Rgb { r: 0, g: 0, b: 0 },
        width: 2.0,
        paint: Some(gradient),
    };
    let json2 = serde_json::to_string(&stroke2).unwrap();
    let restored2: Stroke = serde_json::from_str(&json2).unwrap();
    match restored2.paint {
        Some(Fill::LinearGradient {
            x1,
            y1,
            x2,
            y2,
            stops: s,
        }) => {
            assert!((x1 - 0.0).abs() < f64::EPSILON);
            assert!((y1 - 0.0).abs() < f64::EPSILON);
            assert!((x2 - 100.0).abs() < f64::EPSILON);
            assert!((y2 - 0.0).abs() < f64::EPSILON);
            assert_eq!(s.len(), 2);
            assert_eq!(s[0].offset, 0.0);
            assert_eq!(s[0].color, Rgb { r: 255, g: 0, b: 0 });
            assert_eq!(s[1].offset, 1.0);
            assert_eq!(s[1].color, Rgb { r: 0, g: 0, b: 255 });
        }
        other => panic!("Expected LinearGradient, got {other:?}"),
    }
}
