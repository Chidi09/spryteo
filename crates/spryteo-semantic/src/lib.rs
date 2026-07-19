// TODO(semantic-ml): Real ONNX/MobileSAM integration is deferred future work.
// Currently, mask generation is performed by the caller, and this crate
// implements only the semantic grouping and assignment algorithms.

#![allow(clippy::needless_range_loop)]

use serde::{Deserialize, Serialize};
use spryteo_core::ir::{Contour, ContourSet, Group, LayerStack, Meta, SceneGraph};

/// A custom pixel mask representing an object/segmentation boundary,
/// typically produced by a machine learning model like SAM or FastSAM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mask {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Per-pixel coverage values (0-255).
    pub pixels: Vec<u8>,
}

/// Groups shapes in a scene graph by nested containment based on their bounding boxes.
///
/// Connected components remain under their own `<g>` element. If a shape A's bounding box
/// is fully contained within another shape B's bounding box, shape A is nested as a child
/// group under B's group.
///
/// To handle multiple containing shapes, shape A is nested under the SMALLEST-AREA shape
/// whose bounding box fully contains A's bounding box.
///
/// NOTE: This algorithm simplifies containment check to bounding box overlap (rather than
/// full point-in-polygon checks).
///
/// Rules:
/// - Paint order is strictly preserved.
/// - The function is completely deterministic.
/// - Updates the returned `Meta`'s `NodeMeta.group` field for any renested node.
pub fn group_by_containment(scene: &SceneGraph, meta: &Meta) -> (SceneGraph, Meta) {
    let num_shapes = meta.nodes.len();
    if num_shapes == 0 {
        return (scene.clone(), meta.clone());
    }

    let mut parent_indices = vec![None; num_shapes];

    for i in 0..num_shapes {
        let mut best_parent = None;
        let mut min_area = f64::INFINITY;
        let bbox_i = &meta.nodes[i].bbox;
        let area_i = meta.nodes[i].area;

        for j in 0..num_shapes {
            if j == i {
                continue;
            }
            let bbox_j = &meta.nodes[j].bbox;
            let area_j = meta.nodes[j].area;

            // Check if bbox_j fully contains bbox_i
            let contains = bbox_i.x_min >= bbox_j.x_min
                && bbox_i.x_max <= bbox_j.x_max
                && bbox_i.y_min >= bbox_j.y_min
                && bbox_i.y_max <= bbox_j.y_max;

            if contains {
                // To guarantee a forest (DAG) and avoid cycles, a shape A can only be nested
                // under shape B if B's area is strictly greater than A's area, or they have
                // equal areas and B has a smaller original index (painted first).
                let is_larger = area_j > area_i || ((area_j - area_i).abs() < 1e-9 && j < i);
                if is_larger {
                    if area_j < min_area {
                        min_area = area_j;
                        best_parent = Some(j);
                    } else if (area_j - min_area).abs() < 1e-9 {
                        // Tie-breaker: prefer parent with smaller original index
                        if let Some(prev_best) = best_parent {
                            if j < prev_best {
                                best_parent = Some(j);
                            }
                        } else {
                            best_parent = Some(j);
                        }
                    }
                }
            }
        }
        parent_indices[i] = best_parent;
    }

    // Map children indices for hierarchical reconstruction
    let mut children_indices = vec![vec![]; num_shapes];
    for j in 0..num_shapes {
        if let Some(p) = parent_indices[j] {
            children_indices[p].push(j);
        }
    }

    // Extract original flat shape groups
    let mut original_shape_groups = Vec::new();
    for i in 0..num_shapes {
        let fallback_group = Group {
            id: format!("g-s-{}", i),
            nodes: Vec::new(),
            groups: Vec::new(),
        };
        original_shape_groups.push(fallback_group);
    }

    // Helper to get index of a group
    let get_idx = |g: &Group| -> Option<usize> {
        let id = g.nodes.first()?.id.as_str();
        if id.is_empty() {
            None
        } else {
            meta.nodes.iter().position(|n| n.id == id)
        }
    };

    let mut dfs_counter = 0;
    extract_base_groups(
        &scene.groups,
        &mut original_shape_groups,
        &get_idx,
        &mut dfs_counter,
    );

    // Recursively build nested groups
    fn build_group_tree(
        i: usize,
        original_shape_groups: &[Group],
        children_indices: &[Vec<usize>],
    ) -> Group {
        let mut g = original_shape_groups[i].clone();
        g.groups = children_indices[i]
            .iter()
            .map(|&child_idx| build_group_tree(child_idx, original_shape_groups, children_indices))
            .collect();
        g
    }

    // Top-level groups are those without a parent, in original order
    let mut new_groups = Vec::new();
    for i in 0..num_shapes {
        if parent_indices[i].is_none() {
            new_groups.push(build_group_tree(
                i,
                &original_shape_groups,
                &children_indices,
            ));
        }
    }

    // Update NodeMeta.group field for renested nodes
    let mut new_meta = meta.clone();
    for i in 0..num_shapes {
        if let Some(p) = parent_indices[i] {
            new_meta.nodes[i].group = original_shape_groups[p].id.clone();
        }
    }

    (SceneGraph { groups: new_groups }, new_meta)
}

/// Groups layers in a scene graph by overlap with semantic pixel masks.
///
/// For each layer, we compute its coverage-weighted overlap with each mask and
/// assign it to the mask with the highest overlap ratio, provided the ratio is
/// at least `coverage_threshold` (e.g. 0.6).
///
/// Rules:
/// - Tie-break: lowest mask index wins.
/// - Unassigned layers retain their original grouping from the input `scene`.
/// - Every node whose layer is assigned to a mask is placed under a top-level group
///   keyed `g-mask-<mask_id>`.
/// - Original paint order (z-order) is preserved: the mask group appears at the position
///   of its first member node, and nodes within the mask group maintain their relative order.
pub fn group_by_masks(
    layer_stack: &LayerStack,
    contour_set: &ContourSet,
    scene: &SceneGraph,
    meta: &Meta,
    masks: &[Mask],
    coverage_threshold: f64,
) -> (SceneGraph, Meta) {
    let num_shapes = meta.nodes.len();
    if num_shapes == 0 {
        return (scene.clone(), meta.clone());
    }

    // 1. Compute layer node ranges
    let mut cursor = 0;
    let mut layer_node_ranges = Vec::new();
    for (layer_idx, layer_contours) in contour_set.layers.iter().enumerate() {
        let count: usize = layer_contours.iter().map(count_contour_and_children).sum();
        layer_node_ranges.push((layer_idx, cursor..cursor + count));
        cursor += count;
    }

    // 2. Map shape indices to their layer index
    let mut shape_layers = vec![0; num_shapes];
    for (layer_idx, range) in &layer_node_ranges {
        for i in range.clone() {
            if i < num_shapes {
                shape_layers[i] = *layer_idx;
            }
        }
    }

    // 3. Compute overlap ratio and mask assignments for each layer
    let num_layers = layer_stack.layers.len();
    let mut layer_masks = vec![None; num_layers];
    for l in 0..num_layers {
        let layer = &layer_stack.layers[l];
        let mut best_mask_idx = None;
        let mut best_ratio = -1.0;
        for (mask_idx, mask) in masks.iter().enumerate() {
            let len = layer.mask.len().min(mask.pixels.len());
            let mut intersection_sum = 0.0;
            let mut layer_sum = 0.0;
            for p in 0..len {
                let layer_val = layer.mask[p];
                let mask_val = mask.pixels[p];
                intersection_sum += layer_val.min(mask_val) as f64;
                layer_sum += layer_val as f64;
            }
            let ratio = if layer_sum > 0.0 {
                intersection_sum / layer_sum
            } else {
                0.0
            };
            if ratio >= coverage_threshold && ratio > best_ratio {
                best_ratio = ratio;
                best_mask_idx = Some(mask_idx);
            }
        }
        layer_masks[l] = best_mask_idx;
    }

    // 4. Map shape indices to their assigned mask index
    let mut mask_assignment = vec![None; num_shapes];
    for i in 0..num_shapes {
        let layer_idx = shape_layers[i];
        if layer_idx < num_layers {
            mask_assignment[i] = layer_masks[layer_idx];
        }
    }

    // 5. Get original parents from input scene graph
    let orig_parent = find_parents_in_scene(scene, meta);

    // 6. Determine new parent relations
    let mut new_parent = vec![None; num_shapes];
    for j in 0..num_shapes {
        if let Some(p) = orig_parent[j] {
            if mask_assignment[j].is_some() {
                // If child is assigned to a mask, it stays with the parent only if the parent
                // has the exact same mask assignment
                if mask_assignment[p] == mask_assignment[j] {
                    new_parent[j] = Some(p);
                } else {
                    new_parent[j] = None;
                }
            } else {
                // Child has no mask assignment, so it keeps its parent
                new_parent[j] = Some(p);
            }
        }
    }

    // 7. Map children indices
    let mut children_indices = vec![vec![]; num_shapes];
    for j in 0..num_shapes {
        if let Some(p) = new_parent[j] {
            children_indices[p].push(j);
        }
    }

    // 8. Extract original shape groups
    let mut original_shape_groups = Vec::new();
    for i in 0..num_shapes {
        let fallback_group = Group {
            id: format!("g-s-{}", i),
            nodes: Vec::new(),
            groups: Vec::new(),
        };
        original_shape_groups.push(fallback_group);
    }

    let get_idx = |g: &Group| -> Option<usize> {
        let id = g.nodes.first()?.id.as_str();
        if id.is_empty() {
            None
        } else {
            meta.nodes.iter().position(|n| n.id == id)
        }
    };

    let mut dfs_counter = 0;
    extract_base_groups(
        &scene.groups,
        &mut original_shape_groups,
        &get_idx,
        &mut dfs_counter,
    );

    // Helper to recursively build nested groups
    fn build_group_tree(
        i: usize,
        original_shape_groups: &[Group],
        children_indices: &[Vec<usize>],
    ) -> Group {
        let mut g = original_shape_groups[i].clone();
        g.groups = children_indices[i]
            .iter()
            .map(|&child_idx| build_group_tree(child_idx, original_shape_groups, children_indices))
            .collect();
        g
    }

    // 9. Reconstruct the new list of top-level groups
    let mut new_top_level_groups = Vec::new();
    let mut emitted_masks = vec![false; masks.len()];

    for i in 0..num_shapes {
        if let Some(mask_idx) = mask_assignment[i] {
            if !emitted_masks[mask_idx] {
                // Emit mask group
                let mask = &masks[mask_idx];
                let mut mask_child_groups = Vec::new();
                for j in 0..num_shapes {
                    if mask_assignment[j] == Some(mask_idx) && new_parent[j].is_none() {
                        let g = build_group_tree(j, &original_shape_groups, &children_indices);
                        mask_child_groups.push(g);
                    }
                }
                let mask_group = Group {
                    id: format!("g-mask-{}", mask.id),
                    nodes: Vec::new(),
                    groups: mask_child_groups,
                };
                new_top_level_groups.push(mask_group);
                emitted_masks[mask_idx] = true;
            }
        } else {
            // Unassigned shape
            if new_parent[i].is_none() {
                let g = build_group_tree(i, &original_shape_groups, &children_indices);
                new_top_level_groups.push(g);
            }
        }
    }

    // 10. Update NodeMeta.group field for reassigned nodes
    let mut new_meta = meta.clone();
    for i in 0..num_shapes {
        if let Some(mask_idx) = mask_assignment[i] {
            let mask = &masks[mask_idx];
            new_meta.nodes[i].group = format!("g-mask-{}", mask.id);
        }
    }

    (
        SceneGraph {
            groups: new_top_level_groups,
        },
        new_meta,
    )
}

fn count_contour_and_children(c: &Contour) -> usize {
    1 + c
        .children
        .iter()
        .map(count_contour_and_children)
        .sum::<usize>()
}

fn find_parents_in_scene(scene: &SceneGraph, meta: &Meta) -> Vec<Option<usize>> {
    let num_shapes = meta.nodes.len();
    let mut parents = vec![None; num_shapes];

    let get_idx = |g: &Group| -> Option<usize> {
        let id = g.nodes.first()?.id.as_str();
        if id.is_empty() {
            None
        } else {
            meta.nodes.iter().position(|n| n.id == id)
        }
    };

    fn recurse(
        groups: &[Group],
        parent_idx: Option<usize>,
        parents: &mut Vec<Option<usize>>,
        get_idx: &dyn Fn(&Group) -> Option<usize>,
        dfs_counter: &mut usize,
    ) {
        for group in groups {
            let idx = match get_idx(group) {
                Some(idx) => idx,
                None => {
                    let idx = *dfs_counter;
                    *dfs_counter += 1;
                    idx
                }
            };
            if idx < parents.len() {
                parents[idx] = parent_idx;
                recurse(&group.groups, Some(idx), parents, get_idx, dfs_counter);
            }
        }
    }

    let mut dfs_counter = 0;
    recurse(
        &scene.groups,
        None,
        &mut parents,
        &get_idx,
        &mut dfs_counter,
    );
    parents
}

fn extract_base_groups(
    groups: &[Group],
    original_shape_groups: &mut [Group],
    get_idx: &dyn Fn(&Group) -> Option<usize>,
    dfs_counter: &mut usize,
) {
    for group in groups {
        let idx = match get_idx(group) {
            Some(idx) => idx,
            None => {
                let idx = *dfs_counter;
                *dfs_counter += 1;
                idx
            }
        };
        if idx < original_shape_groups.len() {
            let mut g = group.clone();
            g.groups.clear();
            original_shape_groups[idx] = g;
            extract_base_groups(&group.groups, original_shape_groups, get_idx, dfs_counter);
        }
    }
}

#[cfg(test)]
mod tests {
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
            let count: usize = layer_contours.iter().map(count_contour_and_children).sum();
            layer_node_ranges.push((layer_idx, cursor..cursor + count));
            cursor += count;
        }

        assert_eq!(cursor, 4, "Total node count must be 4");
        assert_eq!(layer_node_ranges[0].1, 0..3);
        assert_eq!(layer_node_ranges[1].1, 3..4);
    }
}
