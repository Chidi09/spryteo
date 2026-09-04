    use super::*;
    use spryteo_core::ir::{
        Bbox, Contour, ContourSet, Fill, Group, Layer, LayerStack, Meta, Node, NodeMeta, Rgb,
        SceneGraph, Shape, Stats, Transform,
    };

    fn make_test_node(id: &str) -> Node {
        Node {
            id: id.to_string(),
            fill: Some(Fill::Solid(Rgb { r: 255, g: 0, b: 0 })),
            stroke: None,
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
            },
            shape: Shape::Path(vec![]),
        }
    }

    fn make_test_group(id: &str, node_id: &str) -> Group {
        Group {
            id: id.to_string(),
            nodes: vec![make_test_node(node_id)],
            groups: vec![],
        }
    }

    fn make_test_node_meta(id: &str, bbox: Bbox, area: f64, z_order: usize) -> NodeMeta {
        let centroid = (
            (bbox.x_min + bbox.x_max) / 2.0,
            (bbox.y_min + bbox.y_max) / 2.0,
        );
        NodeMeta {
            id: id.to_string(),
            bbox,
            centroid,
            area,
            fill: Some(Rgb { r: 255, g: 0, b: 0 }),
            group: format!("g-{}", id),
            z_order,
            suggested_draw_order: z_order,
        }
    }

    #[test]
    fn test_group_by_containment_nested_3_levels() {
        // C inside B, B inside A
        // A (index 0): [0, 0, 30, 30], area = 900
        // B (index 1): [5, 5, 25, 25], area = 400
        // C (index 2): [10, 10, 20, 20], area = 100
        let group_a = make_test_group("g-s-0", "s-0");
        let group_b = make_test_group("g-s-1", "s-1");
        let group_c = make_test_group("g-s-2", "s-2");

        let scene = SceneGraph {
            groups: vec![group_a, group_b, group_c],
        };

        let meta_a = make_test_node_meta(
            "s-0",
            Bbox {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 30.0,
                y_max: 30.0,
            },
            900.0,
            0,
        );
        let meta_b = make_test_node_meta(
            "s-1",
            Bbox {
                x_min: 5.0,
                y_min: 5.0,
                x_max: 25.0,
                y_max: 25.0,
            },
            400.0,
            1,
        );
        let meta_c = make_test_node_meta(
            "s-2",
            Bbox {
                x_min: 10.0,
                y_min: 10.0,
                x_max: 20.0,
                y_max: 20.0,
            },
            100.0,
            2,
        );

        let meta = Meta {
            nodes: vec![meta_a, meta_b, meta_c],
            stats: Stats {
                node_count: 3,
                path_count: 3,
                byte_count: 0,
            },
            current_color_applied: false,
        };

        let (nested_scene, updated_meta) = group_by_containment(&scene, &meta);

        // Assert 1 top level group (A)
        assert_eq!(nested_scene.groups.len(), 1);
        let top_g = &nested_scene.groups[0];
        assert_eq!(top_g.id, "g-s-0");
        assert_eq!(top_g.nodes[0].id, "s-0");

        // Assert B nested inside A
        assert_eq!(top_g.groups.len(), 1);
        let mid_g = &top_g.groups[0];
        assert_eq!(mid_g.id, "g-s-1");
        assert_eq!(mid_g.nodes[0].id, "s-1");

        // Assert C nested inside B
        assert_eq!(mid_g.groups.len(), 1);
        let bot_g = &mid_g.groups[0];
        assert_eq!(bot_g.id, "g-s-2");
        assert_eq!(bot_g.nodes[0].id, "s-2");
        assert_eq!(bot_g.groups.len(), 0);

        // Check updated NodeMeta.group fields
        // s-0 stays "g-s-0" (or g-s-0 from original)
        assert_eq!(updated_meta.nodes[0].group, "g-s-0");
        // s-1 nested under A -> group becomes "g-s-0"
        assert_eq!(updated_meta.nodes[1].group, "g-s-0");
        // s-2 nested under B -> group becomes "g-s-1"
        assert_eq!(updated_meta.nodes[2].group, "g-s-1");
    }

    #[test]
    fn test_group_by_containment_disjoint() {
        let group_a = make_test_group("g-s-0", "s-0");
        let group_b = make_test_group("g-s-1", "s-1");

        let scene = SceneGraph {
            groups: vec![group_a, group_b],
        };

        let meta_a = make_test_node_meta(
            "s-0",
            Bbox {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 10.0,
                y_max: 10.0,
            },
            100.0,
            0,
        );
        let meta_b = make_test_node_meta(
            "s-1",
            Bbox {
                x_min: 20.0,
                y_min: 20.0,
                x_max: 30.0,
                y_max: 30.0,
            },
            100.0,
            1,
        );

        let meta = Meta {
            nodes: vec![meta_a, meta_b],
            stats: Stats {
                node_count: 2,
                path_count: 2,
                byte_count: 0,
            },
            current_color_applied: false,
        };

        let (nested_scene, updated_meta) = group_by_containment(&scene, &meta);

        // Both should stay top level
        assert_eq!(nested_scene.groups.len(), 2);
        assert_eq!(nested_scene.groups[0].id, "g-s-0");
        assert_eq!(nested_scene.groups[1].id, "g-s-1");

        // Groups unchanged
        assert_eq!(updated_meta.nodes[0].group, "g-s-0");
        assert_eq!(updated_meta.nodes[1].group, "g-s-1");
    }

    #[test]
    fn test_group_by_masks_basic_overlap() {
        // 2 layers, 1 contour each (no holes)
        let layer0 = Layer {
            mask: vec![255, 255, 0, 0],
            color: Rgb { r: 255, g: 0, b: 0 },
            z_order: 0,
        };
        let layer1 = Layer {
            mask: vec![0, 0, 255, 255],
            color: Rgb { r: 0, g: 0, b: 255 },
            z_order: 1,
        };
        let layer_stack = LayerStack {
            layers: vec![layer0, layer1],
        };

        let contour_set = ContourSet {
            layers: vec![
                vec![Contour {
                    points: vec![],
                    children: vec![],
                }],
                vec![Contour {
                    points: vec![],
                    children: vec![],
                }],
            ],
        };

        let group_0 = make_test_group("g-s-0", "s-0");
        let group_1 = make_test_group("g-s-1", "s-1");
        let scene = SceneGraph {
            groups: vec![group_0, group_1],
        };

        let bbox = Bbox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 2.0,
            y_max: 2.0,
        };
        let meta = Meta {
            nodes: vec![
                make_test_node_meta("s-0", bbox.clone(), 4.0, 0),
                make_test_node_meta("s-1", bbox.clone(), 4.0, 1),
            ],
            stats: Stats {
                node_count: 2,
                path_count: 2,
                byte_count: 0,
            },
            current_color_applied: false,
        };

        let mask_a = Mask {
            id: "A".to_string(),
            width: 2,
            height: 2,
            pixels: vec![255, 255, 0, 0],
        };
        let mask_b = Mask {
            id: "B".to_string(),
            width: 2,
            height: 2,
            pixels: vec![0, 0, 255, 255],
        };

        let (grouped_scene, updated_meta) = group_by_masks(
            &layer_stack,
            &contour_set,
            &scene,
            &meta,
            &[mask_a, mask_b],
            0.6,
        );

        // We expect 2 top-level groups: g-mask-A and g-mask-B
        assert_eq!(grouped_scene.groups.len(), 2);
        assert_eq!(grouped_scene.groups[0].id, "g-mask-A");
        assert_eq!(grouped_scene.groups[1].id, "g-mask-B");

        assert_eq!(grouped_scene.groups[0].groups.len(), 1);
        assert_eq!(grouped_scene.groups[0].groups[0].id, "g-s-0");

        assert_eq!(grouped_scene.groups[1].groups.len(), 1);
        assert_eq!(grouped_scene.groups[1].groups[0].id, "g-s-1");

        // Verify NodeMeta.group fields updated
        assert_eq!(updated_meta.nodes[0].group, "g-mask-A");
        assert_eq!(updated_meta.nodes[1].group, "g-mask-B");
    }

    #[test]
    fn test_group_by_masks_sub_threshold() {
        let layer = Layer {
            mask: vec![255, 255, 255, 255],
            color: Rgb { r: 255, g: 0, b: 0 },
            z_order: 0,
        };
        let layer_stack = LayerStack {
            layers: vec![layer],
        };

        let contour_set = ContourSet {
            layers: vec![vec![Contour {
                points: vec![],
                children: vec![],
            }]],
        };

        let group = make_test_group("g-s-0", "s-0");
        let scene = SceneGraph {
            groups: vec![group],
        };

        let bbox = Bbox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 2.0,
            y_max: 2.0,
        };
        let meta = Meta {
            nodes: vec![make_test_node_meta("s-0", bbox, 4.0, 0)],
            stats: Stats {
                node_count: 1,
                path_count: 1,
                byte_count: 0,
            },
            current_color_applied: false,
        };

        // Mask only has 25% overlap with the layer
        let mask = Mask {
            id: "A".to_string(),
            width: 2,
            height: 2,
            pixels: vec![255, 0, 0, 0],
        };

        let (grouped_scene, updated_meta) =
            group_by_masks(&layer_stack, &contour_set, &scene, &meta, &[mask], 0.6);

        // Should not be reassigned. Stays "g-s-0"
        assert_eq!(grouped_scene.groups.len(), 1);
        assert_eq!(grouped_scene.groups[0].id, "g-s-0");
        assert_eq!(updated_meta.nodes[0].group, "g-s-0");
    }

    #[test]
    fn test_layer_node_ranges_cursor_invariant() {
        // Layer 0: 1 outer contour + 2 child holes = 3 nodes
        // Layer 1: 1 outer contour + 0 children = 1 node
        let c_child1 = Contour {
            points: vec![],
            children: vec![],
        };
        let c_child2 = Contour {
            points: vec![],
            children: vec![],
        };
        let c_parent = Contour {
            points: vec![],
            children: vec![c_child1, c_child2],
        };
        let c_other = Contour {
            points: vec![],
            children: vec![],
        };

        let contour_set = ContourSet {
            layers: vec![vec![c_parent], vec![c_other]],
        };

        // Compute layer node ranges as in group_by_masks
        let mut cursor = 0;
        let mut layer_node_ranges = Vec::new();
        for (layer_idx, layer_contours) in contour_set.layers.iter().enumerate() {
            let count: usize = layer_contours
                .iter()
                .map(Contour::emitted_curve_count)
                .sum();
            layer_node_ranges.push((layer_idx, cursor..cursor + count));
            cursor += count;
        }

        // c_parent absorbs its two holes as subpaths of one shape, so layer 0
        // emits a single node; c_other emits the second.
        assert_eq!(cursor, 2, "Total node count must be 2");
        assert_eq!(layer_node_ranges[0].1, 0..1);
        assert_eq!(layer_node_ranges[1].1, 1..2);
    }
