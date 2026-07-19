use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphNode {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: usize,             // index in nodes
    pub to: usize,               // index in nodes
    pub pixels: Vec<(i32, i32)>, // starts at node[from], ends at node[to]
}

pub struct StrokeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Builds separate stroke graphs for each connected component of skeleton pixels.
/// Connected components and nodes/edges are processed in a deterministic row-major scan order.
pub fn build_stroke_graphs(skeleton: &[bool], width: u32, height: u32) -> Vec<StrokeGraph> {
    let w = width as i32;
    let h = height as i32;
    let mut visited = vec![false; (width * height) as usize];
    let mut graphs = Vec::new();

    let get_skel_neighbors = |x: i32, y: i32, skel: &[bool]| -> Vec<(i32, i32)> {
        let mut raw_neighbors = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && nx < w && ny >= 0 && ny < h {
                    let n_idx = (ny * w + nx) as usize;
                    if skel[n_idx] {
                        raw_neighbors.push((nx, ny));
                    }
                }
            }
        }

        let mut neighbors = Vec::new();
        for &(nx, ny) in &raw_neighbors {
            let is_diagonal = nx != x && ny != y;
            if is_diagonal {
                let h_helper_idx = (y * w + nx) as usize;
                let v_helper_idx = (ny * w + x) as usize;
                if skel[h_helper_idx] || skel[v_helper_idx] {
                    continue;
                }
            }
            neighbors.push((nx, ny));
        }
        neighbors
    };

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if skeleton[idx] && !visited[idx] {
                // BFS to get all pixels in this component
                let mut component = Vec::new();
                let mut queue = std::collections::VecDeque::new();
                queue.push_back((x, y));
                visited[idx] = true;

                while let Some((cx, cy)) = queue.pop_front() {
                    component.push((cx, cy));
                    for (nx, ny) in get_skel_neighbors(cx, cy, skeleton) {
                        let n_idx = (ny * w + nx) as usize;
                        if !visited[n_idx] {
                            visited[n_idx] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }

                let graph = build_graph_for_component(&component, skeleton, width, height);
                graphs.push(graph);
            }
        }
    }

    graphs
}

fn build_graph_for_component(
    component: &[(i32, i32)],
    skeleton: &[bool],
    width: u32,
    height: u32,
) -> StrokeGraph {
    let w = width as i32;
    let h = height as i32;

    let get_neighbors_in_comp = |x: i32, y: i32| -> Vec<(i32, i32)> {
        let mut raw_neighbors = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && nx < w && ny >= 0 && ny < h {
                    let n_idx = (ny * w + nx) as usize;
                    if skeleton[n_idx] && component.contains(&(nx, ny)) {
                        raw_neighbors.push((nx, ny));
                    }
                }
            }
        }

        let mut neighbors = Vec::new();
        for &(nx, ny) in &raw_neighbors {
            let is_diagonal = nx != x && ny != y;
            if is_diagonal {
                let h_helper_idx = (y * w + nx) as usize;
                let v_helper_idx = (ny * w + x) as usize;
                let h_in_skel = skeleton[h_helper_idx] && component.contains(&(nx, y));
                let v_in_skel = skeleton[v_helper_idx] && component.contains(&(x, ny));
                if h_in_skel || v_in_skel {
                    continue;
                }
            }
            neighbors.push((nx, ny));
        }
        neighbors
    };

    // Classify neighbor counts
    let mut degrees = BTreeMap::new();
    for &(cx, cy) in component {
        let nb = get_neighbors_in_comp(cx, cy);
        degrees.insert((cx, cy), nb.len());
    }

    // Nodes are endpoints (degree 1) and junctions (degree >= 3), plus degree 0 isolated points.
    let mut node_coords = Vec::new();
    for &(cx, cy) in component {
        let deg = *degrees.get(&(cx, cy)).unwrap_or(&0);
        if deg != 2 {
            node_coords.push((cx, cy));
        }
    }

    // If there are no nodes in the component (e.g. it is a closed loop),
    // we promote the top-left-most pixel to a node.
    if node_coords.is_empty() && !component.is_empty() {
        let mut best = component[0];
        for &pt in component {
            if pt.1 < best.1 || (pt.1 == best.1 && pt.0 < best.0) {
                best = pt;
            }
        }
        node_coords.push(best);
        degrees.insert(best, 2);
    }

    // Sort nodes in row-major order for determinism
    node_coords.sort_by(|a, b| {
        if a.1 != b.1 {
            a.1.cmp(&b.1)
        } else {
            a.0.cmp(&b.0)
        }
    });

    let nodes: Vec<GraphNode> = node_coords
        .iter()
        .map(|&(nx, ny)| GraphNode { x: nx, y: ny })
        .collect();

    let mut edges = Vec::new();

    for (from_idx, node_coord) in node_coords.iter().enumerate() {
        let &(sx, sy) = node_coord;

        let mut neighbors = get_neighbors_in_comp(sx, sy);
        neighbors.sort_by(|a, b| {
            if a.1 != b.1 {
                a.1.cmp(&b.1)
            } else {
                a.0.cmp(&b.0)
            }
        });

        for &nb in &neighbors {
            let nb_is_node = node_coords.contains(&nb);

            if nb_is_node {
                let to_idx = node_coords.iter().position(|&x| x == nb).unwrap();
                if from_idx < to_idx {
                    edges.push(GraphEdge {
                        from: from_idx,
                        to: to_idx,
                        pixels: vec![(sx, sy), nb],
                    });
                }
            } else {
                let mut path = vec![(sx, sy), nb];
                let mut curr = nb;
                let mut prev = (sx, sy);

                loop {
                    let c_neighbors = get_neighbors_in_comp(curr.0, curr.1);
                    if c_neighbors.len() < 2 {
                        // In case of unexpected dangling paths
                        break;
                    }
                    let next = if c_neighbors[0] == prev {
                        c_neighbors[1]
                    } else {
                        c_neighbors[0]
                    };

                    path.push(next);
                    if node_coords.contains(&next) {
                        break;
                    }

                    prev = curr;
                    curr = next;
                }

                let end_node = *path.last().unwrap();
                if let Some(to_idx) = node_coords.iter().position(|&x| x == end_node) {
                    if from_idx < to_idx {
                        edges.push(GraphEdge {
                            from: from_idx,
                            to: to_idx,
                            pixels: path,
                        });
                    } else if from_idx == to_idx {
                        // For loop edges, keep only one direction deterministically
                        let first_step = path[1];
                        let last_step = path[path.len() - 2];
                        if first_step.1 < last_step.1
                            || (first_step.1 == last_step.1 && first_step.0 < last_step.0)
                        {
                            edges.push(GraphEdge {
                                from: from_idx,
                                to: to_idx,
                                pixels: path,
                            });
                        }
                    }
                }
            }
        }
    }

    StrokeGraph { nodes, edges }
}
