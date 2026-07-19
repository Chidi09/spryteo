//! Contour extraction: Suzuki-Abe border following, marching squares.

use spryteo_core::ir::{Contour, ContourSet, LayerStack};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EdgeKey {
    Horizontal { x: u32, y: u32 },
    Vertical { x: u32, y: u32 },
}

impl EdgeKey {
    fn to_coord(self, mask: &[u8], w_padded: usize) -> (f64, f64) {
        match self {
            EdgeKey::Horizontal { x, y } => {
                let v0 = mask[y as usize * w_padded + x as usize] as f64;
                let v1 = mask[y as usize * w_padded + (x + 1) as usize] as f64;
                let frac = if (v1 - v0).abs() < 1e-5 {
                    0.5
                } else {
                    (127.5 - v0) / (v1 - v0)
                };
                (x as f64 + frac, y as f64)
            }
            EdgeKey::Vertical { x, y } => {
                let v0 = mask[y as usize * w_padded + x as usize] as f64;
                let v1 = mask[(y + 1) as usize * w_padded + x as usize] as f64;
                let frac = if (v1 - v0).abs() < 1e-5 {
                    0.5
                } else {
                    (127.5 - v0) / (v1 - v0)
                };
                (x as f64, y as f64 + frac)
            }
        }
    }
}

/// Helper function to perform a point-in-polygon test (ray casting).
fn point_in_polygon(point: (f64, f64), polygon: &[(f64, f64)]) -> bool {
    let (px, py) = point;
    let mut inside = false;
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (ix, iy) = polygon[i];
        let (jx, jy) = polygon[j];
        if ((iy > py) != (jy > py)) && (px < (jx - ix) * (py - iy) / (jy - iy) + ix) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Calculates the signed area of a polygon.
/// Positive signed area represents a clockwise polygon in a y-down coordinate system.
fn polygon_area(points: &[(f64, f64)]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        area += (xj * yi) - (xi * yj);
        j = i;
    }
    area / 2.0
}

struct TempNode {
    points: Vec<(f64, f64)>,
    area: f64,
    parent: Option<usize>,
    children: Vec<usize>,
}

fn build_contour(idx: usize, nodes: &[TempNode]) -> Contour {
    let mut children = Vec::new();
    for &child_idx in &nodes[idx].children {
        children.push(build_contour(child_idx, nodes));
    }
    Contour {
        points: nodes[idx].points.clone(),
        children,
    }
}

fn sort_contour_tree(contour: &mut Contour) {
    for child in &mut contour.children {
        sort_contour_tree(child);
    }
    contour.children.sort_by(|a, b| {
        let a_pt = a.points.first().copied().unwrap_or((0.0, 0.0));
        let b_pt = b.points.first().copied().unwrap_or((0.0, 0.0));
        (a_pt.1, a_pt.0)
            .partial_cmp(&(b_pt.1, b_pt.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Extracts contours from a quantized layer.
fn extract_layer_contours(mask: &[u8], width: u32, height: u32, turdsize: u32) -> Vec<Contour> {
    if width == 0 || height == 0 || mask.len() != (width as usize * height as usize) {
        return Vec::new();
    }

    // 1. Pad the mask by 1 pixel of background (value 0) on all sides.
    let w_padded = (width + 2) as usize;
    let h_padded = (height + 2) as usize;
    let mut padded_mask = vec![0u8; w_padded * h_padded];
    for r in 0..height as usize {
        for c in 0..width as usize {
            padded_mask[(r + 1) * w_padded + (c + 1)] = mask[r * width as usize + c];
        }
    }

    // 2. Connected-component labeling (CCL) using BFS/flood-fill.
    // Group foreground (value > 127) using 8-connectivity.
    // Group background (value <= 127) using 4-connectivity.
    let size = w_padded * h_padded;
    let mut labels = vec![0usize; size];
    // Indexed by label (labels are dense, starting at 1; index 0 unused).
    // These were BTreeMaps, but the despeckle pass below looks both up
    // once per pixel, and a log-time pointer-chasing lookup per pixel is
    // measurably slow at photo sizes.
    let mut component_sizes: Vec<usize> = vec![0];
    let mut component_is_fg: Vec<bool> = vec![false];
    let mut next_label = 1;

    for y in 0..h_padded {
        for x in 0..w_padded {
            let idx = y * w_padded + x;
            if labels[idx] != 0 {
                continue;
            }
            let is_fg = padded_mask[idx] > 127;
            let lbl = next_label;
            next_label += 1;

            let mut queue = Vec::new();
            labels[idx] = lbl;
            queue.push((x, y));
            let mut count = 0;

            // Neighbour offsets in the same visit order the previous
            // per-pixel Vec-building code produced (8-connectivity scans
            // dy then dx; 4-connectivity uses the explicit list), so BFS
            // queue order -- and therefore all downstream output -- is
            // unchanged. Building a fresh Vec per visited pixel was one
            // heap allocation per pixel across the whole image, per layer.
            const NEIGHBORS_8: [(isize, isize); 8] = [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ];
            const NEIGHBORS_4: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
            let deltas: &[(isize, isize)] = if is_fg { &NEIGHBORS_8 } else { &NEIGHBORS_4 };

            let mut head = 0;
            while head < queue.len() {
                let (cx, cy) = queue[head];
                head += 1;
                count += 1;

                for &(dx, dy) in deltas {
                    let nx = cx as isize + dx;
                    let ny = cy as isize + dy;
                    if nx < 0 || nx >= w_padded as isize || ny < 0 || ny >= h_padded as isize {
                        continue;
                    }
                    let (nx, ny) = (nx as usize, ny as usize);
                    let nidx = ny * w_padded + nx;
                    if labels[nidx] == 0 {
                        let n_is_fg = padded_mask[nidx] > 127;
                        if n_is_fg == is_fg {
                            labels[nidx] = lbl;
                            queue.push((nx, ny));
                        }
                    }
                }
            }

            debug_assert_eq!(component_sizes.len(), lbl);
            component_sizes.push(count);
            component_is_fg.push(is_fg);
        }
    }

    // Identify the external background component label (touches 0,0)
    let external_bg_label = labels[0];

    // 3. Despeckle: drop components with pixel area < turdsize
    for y in 0..h_padded {
        for x in 0..w_padded {
            let idx = y * w_padded + x;
            let lbl = labels[idx];
            if lbl == external_bg_label {
                continue;
            }
            let is_fg = component_is_fg[lbl];
            let comp_size = component_sizes[lbl];
            if comp_size < turdsize as usize {
                if is_fg {
                    padded_mask[idx] = 0;
                } else {
                    padded_mask[idx] = 255;
                }
            }
        }
    }

    // 4. Marching Squares over the updated padded mask at iso-level 127.5.
    let mut adj: std::collections::BTreeMap<EdgeKey, Vec<EdgeKey>> =
        std::collections::BTreeMap::new();
    for y in 0..(h_padded - 1) {
        for x in 0..(w_padded - 1) {
            let tl = padded_mask[y * w_padded + x] > 127;
            let tr = padded_mask[y * w_padded + x + 1] > 127;
            let br = padded_mask[(y + 1) * w_padded + x + 1] > 127;
            let bl = padded_mask[(y + 1) * w_padded + x] > 127;

            let state = (tl as u8) | ((tr as u8) << 1) | ((br as u8) << 2) | ((bl as u8) << 3);

            let top = EdgeKey::Horizontal {
                x: x as u32,
                y: y as u32,
            };
            let right = EdgeKey::Vertical {
                x: (x + 1) as u32,
                y: y as u32,
            };
            let bottom = EdgeKey::Horizontal {
                x: x as u32,
                y: (y + 1) as u32,
            };
            let left = EdgeKey::Vertical {
                x: x as u32,
                y: y as u32,
            };

            let mut cell_segments = Vec::new();
            match state {
                1 | 14 => {
                    cell_segments.push((top, left));
                }
                2 | 13 => {
                    cell_segments.push((top, right));
                }
                4 | 11 => {
                    cell_segments.push((right, bottom));
                }
                8 | 7 => {
                    cell_segments.push((left, bottom));
                }
                3 | 12 => {
                    cell_segments.push((left, right));
                }
                6 | 9 => {
                    cell_segments.push((top, bottom));
                }
                5 => {
                    // Saddle: TL & BR active, TR & BL inactive
                    let v_tl = padded_mask[y * w_padded + x] as f64;
                    let v_tr = padded_mask[y * w_padded + x + 1] as f64;
                    let v_br = padded_mask[(y + 1) * w_padded + x + 1] as f64;
                    let v_bl = padded_mask[(y + 1) * w_padded + x] as f64;
                    let avg = (v_tl + v_tr + v_br + v_bl) / 4.0;
                    if avg <= 127.5 {
                        // Minority is active (inside): connect active corners TL & BR
                        cell_segments.push((top, right));
                        cell_segments.push((left, bottom));
                    } else {
                        // Minority is inactive (outside): connect inactive corners TR & BL
                        cell_segments.push((top, left));
                        cell_segments.push((right, bottom));
                    }
                }
                10 => {
                    // Saddle: TR & BL active, TL & BR inactive
                    let v_tl = padded_mask[y * w_padded + x] as f64;
                    let v_tr = padded_mask[y * w_padded + x + 1] as f64;
                    let v_br = padded_mask[(y + 1) * w_padded + x + 1] as f64;
                    let v_bl = padded_mask[(y + 1) * w_padded + x] as f64;
                    let avg = (v_tl + v_tr + v_br + v_bl) / 4.0;
                    if avg <= 127.5 {
                        // Minority is active (inside): connect active corners TR & BL
                        cell_segments.push((top, left));
                        cell_segments.push((right, bottom));
                    } else {
                        // Minority is inactive (outside): connect inactive corners TL & BR
                        cell_segments.push((top, right));
                        cell_segments.push((left, bottom));
                    }
                }
                _ => {}
            }

            for (u, v) in cell_segments {
                adj.entry(u).or_default().push(v);
                adj.entry(v).or_default().push(u);
            }
        }
    }

    // 5. Trace loops from the segment graph.
    let mut visited = std::collections::BTreeSet::new();
    let mut loops = Vec::new();

    for &start_vertex in adj.keys() {
        if visited.contains(&start_vertex) {
            continue;
        }

        let mut loop_vertices = Vec::new();
        let current = start_vertex;
        loop_vertices.push(current);
        visited.insert(current);

        let neighbors = &adj[&current];
        if neighbors.len() < 2 {
            continue;
        }
        let mut next = if neighbors[0] < neighbors[1] {
            neighbors[0]
        } else {
            neighbors[1]
        };

        let mut broken = false;
        while next != start_vertex {
            loop_vertices.push(next);
            visited.insert(next);

            let n_list = &adj[&next];
            if n_list.len() < 2 {
                broken = true;
                break;
            }
            let prev = loop_vertices[loop_vertices.len() - 2];
            let nxt = if n_list[0] == prev {
                n_list[1]
            } else {
                n_list[0]
            };
            next = nxt;
        }

        if !broken && loop_vertices.len() >= 3 {
            loops.push(loop_vertices);
        }
    }

    let mut all_contours = Vec::new();

    // Map each loop back to original coordinates and orient it correctly
    for loop_vertices in loops {
        let mut points = Vec::new();
        for vertex in &loop_vertices {
            let (px, py) = vertex.to_coord(&padded_mask, w_padded);
            points.push((px - 1.0, py - 1.0));
        }

        // Determine if this loop is foreground or background
        let start_vertex = loop_vertices[0];
        let g_active = match start_vertex {
            EdgeKey::Horizontal { x, y } => {
                if padded_mask[y as usize * w_padded + x as usize] > 127 {
                    (x, y)
                } else {
                    (x + 1, y)
                }
            }
            EdgeKey::Vertical { x, y } => {
                if padded_mask[y as usize * w_padded + x as usize] > 127 {
                    (x, y)
                } else {
                    (x, y + 1)
                }
            }
        };

        let g_active_orig = (g_active.0 as f64 - 1.0, g_active.1 as f64 - 1.0);
        let is_fg_contour = point_in_polygon(g_active_orig, &points);

        let area = polygon_area(&points);
        if is_fg_contour {
            // Foreground contour -> Clockwise (signed area > 0)
            if area < 0.0 {
                points.reverse();
            }
        } else {
            // Hole contour -> Counter-clockwise (signed area < 0)
            if area > 0.0 {
                points.reverse();
            }
        }

        // Rotate vertices to start at topmost-leftmost vertex to ensure determinism
        if !points.is_empty() {
            let mut min_idx = 0;
            let mut min_val = (points[0].1, points[0].0);
            for (idx, &pt) in points.iter().enumerate().skip(1) {
                let val = (pt.1, pt.0);
                if val < min_val {
                    min_val = val;
                    min_idx = idx;
                }
            }
            points.rotate_left(min_idx);
        }

        all_contours.push(points);
    }

    // 6. Build the parent-child hierarchy tree.
    let mut nodes: Vec<TempNode> = all_contours
        .into_iter()
        .map(|points| {
            let area = polygon_area(&points).abs();
            TempNode {
                points,
                area,
                parent: None,
                children: Vec::new(),
            }
        })
        .collect();

    for i in 0..nodes.len() {
        let mut best_parent = None;
        let mut min_area = f64::MAX;
        let p_test = nodes[i].points[0];
        for (j, node_j) in nodes.iter().enumerate() {
            if i == j {
                continue;
            }
            if point_in_polygon(p_test, &node_j.points) && node_j.area < min_area {
                min_area = node_j.area;
                best_parent = Some(j);
            }
        }
        nodes[i].parent = best_parent;
    }

    for i in 0..nodes.len() {
        if let Some(p) = nodes[i].parent {
            nodes[p].children.push(i);
        }
    }

    let mut top_level = Vec::new();
    for i in 0..nodes.len() {
        if nodes[i].parent.is_none() {
            top_level.push(build_contour(i, &nodes));
        }
    }

    // Sort the top-level list and recursive children to ensure absolute determinism.
    for c in &mut top_level {
        sort_contour_tree(c);
    }
    top_level.sort_by(|a, b| {
        let a_pt = a.points.first().copied().unwrap_or((0.0, 0.0));
        let b_pt = b.points.first().copied().unwrap_or((0.0, 0.0));
        (a_pt.1, a_pt.0)
            .partial_cmp(&(b_pt.1, b_pt.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    top_level
}

/// Main entry point for Phase 1 contour extraction.
///
/// Processes each layer in the input `LayerStack` and extracts subpixel coordinates
/// of closed contours using Marching Squares with linear interpolation, despeckling,
/// and hierarchical parent-child hole nesting.
pub fn extract_contours(layers: &LayerStack, width: u32, height: u32, turdsize: u32) -> ContourSet {
    let mut result_layers = Vec::new();

    for layer in &layers.layers {
        let layer_contours = extract_layer_contours(&layer.mask, width, height, turdsize);
        result_layers.push(layer_contours);
    }

    ContourSet {
        layers: result_layers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spryteo_core::ir::{Layer, Rgb};

    fn make_layer(mask: Vec<u8>) -> Layer {
        Layer {
            mask,
            color: Rgb { r: 255, g: 0, b: 0 },
            z_order: 0,
        }
    }

    #[test]
    fn test_single_solid_filled_square() {
        // A 6x6 image with a 4x4 filled square in the center (from 1,1 to 4,4 inclusive)
        let mut mask = vec![0u8; 36];
        for r in 1..=4 {
            for c in 1..=4 {
                mask[r * 6 + c] = 255;
            }
        }
        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };
        let set = extract_contours(&stack, 6, 6, 0);

        assert_eq!(set.layers.len(), 1);
        let contours = &set.layers[0];
        assert_eq!(contours.len(), 1);

        let contour = &contours[0];
        assert!(contour.children.is_empty());

        // Bounding box of the contour
        let min_x = contour
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min);
        let max_x = contour
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = contour
            .points
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min);
        let max_y = contour
            .points
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max);

        // Bounding box should roughly match the square extent (1..4)
        assert!((min_x - 0.5).abs() < 0.1);
        assert!((max_x - 4.5).abs() < 0.1);
        assert!((min_y - 0.5).abs() < 0.1);
        assert!((max_y - 4.5).abs() < 0.1);
    }

    #[test]
    fn test_soft_edge_interpolation() {
        // Create two 6x6 images. One with a hard edge, one with a soft edge.
        // Hard edge:
        // 0 0 0 0 0 0
        // 0 0 255 255 0 0
        // ...
        let mut hard_mask = vec![0u8; 36];
        hard_mask[8] = 255;
        hard_mask[9] = 255;

        // Soft edge:
        // 0 0 0 0 0 0
        // 0 0 160 255 0 0
        let mut soft_mask = vec![0u8; 36];
        soft_mask[8] = 160;
        soft_mask[9] = 255;

        let hard_stack = LayerStack {
            layers: vec![make_layer(hard_mask)],
        };
        let soft_stack = LayerStack {
            layers: vec![make_layer(soft_mask)],
        };

        let hard_set = extract_contours(&hard_stack, 6, 6, 0);
        let soft_set = extract_contours(&soft_stack, 6, 6, 0);

        let hard_pts = &hard_set.layers[0][0].points;
        let soft_pts = &soft_set.layers[0][0].points;

        assert_ne!(hard_pts, soft_pts);

        // The point on the left edge of the soft mask should be interpolated
        // and have a different subpixel X coordinate.
        // For hard mask, 0 -> 255 cross at 0.5 fraction (coordinate 1.5).
        // For soft mask, 0 -> 160 crosses at (127.5 / 160) = 0.796875 fraction (coordinate 1.796875).
        let hard_x_coords: Vec<f64> = hard_pts.iter().map(|p| p.0).collect();
        let soft_x_coords: Vec<f64> = soft_pts.iter().map(|p| p.0).collect();
        assert_ne!(hard_x_coords, soft_x_coords);
    }

    #[test]
    fn test_ring_shape_with_hole() {
        // A 7x7 image with a 5x5 filled square having a 1x1 hole in the center.
        // Filled: 1..5 inclusive, hole at 3,3.
        let mut mask = vec![0u8; 49];
        for r in 1..=5 {
            for c in 1..=5 {
                if r == 3 && c == 3 {
                    mask[r * 7 + c] = 0;
                } else {
                    mask[r * 7 + c] = 255;
                }
            }
        }

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };
        let set = extract_contours(&stack, 7, 7, 0);

        assert_eq!(set.layers[0].len(), 1);
        let parent = &set.layers[0][0];
        assert_eq!(parent.children.len(), 1);

        let hole = &parent.children[0];
        assert!(hole.children.is_empty());

        // Bounding box of hole should roughly match 3,3
        let min_x = hole
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min);
        let max_x = hole
            .points
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((min_x - 2.5).abs() < 0.1);
        assert!((max_x - 3.5).abs() < 0.1);
    }

    #[test]
    fn test_despeckle() {
        // A tiny 1-pixel speck in a 6x6 image
        let mut mask = vec![0u8; 36];
        mask[2 * 6 + 2] = 255;

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };

        // turdsize = 5 should drop it
        let set_dropped = extract_contours(&stack, 6, 6, 5);
        assert!(set_dropped.layers[0].is_empty());

        // turdsize = 0 should keep it
        let set_kept = extract_contours(&stack, 6, 6, 0);
        assert_eq!(set_kept.layers[0].len(), 1);
    }

    #[test]
    fn test_determinism() {
        // An image with two distinct components and a hole
        let mut mask = vec![0u8; 64];
        // Component 1: 3x3 square at 1,1
        for r in 1..=3 {
            for c in 1..=3 {
                mask[r * 8 + c] = 255;
            }
        }
        // Component 2: 4x4 square at 4,4 with a hole at 5,5
        for r in 4..=7 {
            for c in 4..=7 {
                if r == 5 && c == 5 {
                    mask[r * 8 + c] = 0;
                } else {
                    mask[r * 8 + c] = 255;
                }
            }
        }

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };

        let result1 = extract_contours(&stack, 8, 8, 0);
        let result2 = extract_contours(&stack, 8, 8, 0);

        let json1 = serde_json::to_string(&result1).unwrap();
        let json2 = serde_json::to_string(&result2).unwrap();

        assert_eq!(json1, json2);
    }

    #[test]
    fn test_two_separate_regions() {
        // An 8x8 image with two 2x2 squares
        let mut mask = vec![0u8; 64];
        for r in 1..=2 {
            for c in 1..=2 {
                mask[r * 8 + c] = 255;
            }
        }
        for r in 5..=6 {
            for c in 5..=6 {
                mask[r * 8 + c] = 255;
            }
        }

        let stack = LayerStack {
            layers: vec![make_layer(mask)],
        };
        let set = extract_contours(&stack, 8, 8, 0);

        assert_eq!(set.layers[0].len(), 2);
    }
}
