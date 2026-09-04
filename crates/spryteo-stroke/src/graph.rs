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

/// Merges junction nodes sitting closer together than the local stroke width into their centroid.
pub fn merge_close_junctions(graph: StrokeGraph, dist: &[f64], width: u32) -> StrokeGraph {
    let n_nodes = graph.nodes.len();
    if n_nodes == 0 {
        return graph;
    }

    // a. degree[node] = number of incident edge endpoints over graph.edges (loop edge from==to counts 2)
    let mut degrees = vec![0usize; n_nodes];
    for edge in &graph.edges {
        degrees[edge.from] += 1;
        degrees[edge.to] += 1;
    }

    let is_junction: Vec<bool> = degrees.iter().map(|&d| d >= 3).collect();

    // b. Candidate edge = (from != to) AND both endpoints are junctions AND
    //    (edge.pixels.len() as f64 - 1.0) < 2.0 * f64::max(d_from, d_to)
    let mut is_candidate_edge = vec![false; graph.edges.len()];
    let mut candidate_count = 0;

    let w_usize = width as usize;

    for (edge_idx, edge) in graph.edges.iter().enumerate() {
        if edge.from != edge.to && is_junction[edge.from] && is_junction[edge.to] {
            let n_from = &graph.nodes[edge.from];
            let n_to = &graph.nodes[edge.to];
            let d_from = dist[(n_from.y as usize) * w_usize + n_from.x as usize];
            let d_to = dist[(n_to.y as usize) * w_usize + n_to.x as usize];
            let path_len = edge.pixels.len() as f64 - 1.0;
            if path_len < 2.0 * f64::max(d_from, d_to) {
                is_candidate_edge[edge_idx] = true;
                candidate_count += 1;
            }
        }
    }

    // g. Early return if zero candidate edges
    if candidate_count == 0 {
        return graph;
    }

    // c. Union-find over node indices using candidate edges only
    struct UnionFind {
        parent: Vec<usize>,
    }
    impl UnionFind {
        fn new(n: usize) -> Self {
            Self {
                parent: (0..n).collect(),
            }
        }
        fn find(&mut self, i: usize) -> usize {
            let mut root = i;
            while root != self.parent[root] {
                root = self.parent[root];
            }
            let mut curr = i;
            while curr != root {
                let next = self.parent[curr];
                self.parent[curr] = root;
                curr = next;
            }
            root
        }
        fn union(&mut self, i: usize, j: usize) {
            let r_i = self.find(i);
            let r_j = self.find(j);
            if r_i != r_j {
                if r_i < r_j {
                    self.parent[r_j] = r_i;
                } else {
                    self.parent[r_i] = r_j;
                }
            }
        }
    }

    let mut uf = UnionFind::new(n_nodes);
    for (edge_idx, edge) in graph.edges.iter().enumerate() {
        if is_candidate_edge[edge_idx] {
            uf.union(edge.from, edge.to);
        }
    }

    // d. Centroid coords for clusters with >= 2 members
    let mut cluster_members: Vec<Vec<usize>> = vec![Vec::new(); n_nodes];
    for i in 0..n_nodes {
        let root = uf.find(i);
        cluster_members[root].push(i);
    }

    let mut centroids = std::collections::HashMap::new();
    for (root, members) in cluster_members.iter().enumerate() {
        if members.len() >= 2 {
            let sum_x: f64 = members.iter().map(|&idx| graph.nodes[idx].x as f64).sum();
            let sum_y: f64 = members.iter().map(|&idx| graph.nodes[idx].y as f64).sum();
            let count = members.len() as f64;
            let cx = (sum_x / count).round() as i32;
            let cy = (sum_y / count).round() as i32;
            centroids.insert(root, (cx, cy));
        }
    }

    // e. Rebuild nodes: keep original node order; cluster represented at position of lowest-index member
    let mut retained_nodes = Vec::new();
    let mut retained_orig_roots = Vec::new();

    for i in 0..n_nodes {
        let root = uf.find(i);
        if i == root {
            let (nx, ny) = if let Some(&(cx, cy)) = centroids.get(&root) {
                (cx, cy)
            } else {
                (graph.nodes[i].x, graph.nodes[i].y)
            };
            retained_nodes.push(GraphNode { x: nx, y: ny });
            retained_orig_roots.push(root);
        }
    }

    // Re-sort node list row-major (y, then x)
    let mut retained_indices: Vec<usize> = (0..retained_nodes.len()).collect();
    retained_indices.sort_by(|&a, &b| {
        let na = &retained_nodes[a];
        let nb = &retained_nodes[b];
        if na.y != nb.y {
            na.y.cmp(&nb.y)
        } else {
            na.x.cmp(&nb.x)
        }
    });

    let mut new_nodes = Vec::with_capacity(retained_nodes.len());
    let mut temp_to_new = vec![0usize; retained_nodes.len()];
    for (new_idx, &temp_idx) in retained_indices.iter().enumerate() {
        new_nodes.push(retained_nodes[temp_idx].clone());
        temp_to_new[temp_idx] = new_idx;
    }

    let mut root_to_temp = vec![0usize; n_nodes];
    for (temp_idx, &root) in retained_orig_roots.iter().enumerate() {
        root_to_temp[root] = temp_idx;
    }

    let mut old_to_new = vec![0usize; n_nodes];
    for (old_idx, slot) in old_to_new.iter_mut().enumerate() {
        let root = uf.find(old_idx);
        let temp_idx = root_to_temp[root];
        *slot = temp_to_new[temp_idx];
    }

    // f. Rebuild edges in original edge order
    let mut new_edges = Vec::new();
    for (edge_idx, edge) in graph.edges.iter().enumerate() {
        if is_candidate_edge[edge_idx] {
            continue;
        }

        let mut new_from = old_to_new[edge.from];
        let mut new_to = old_to_new[edge.to];
        let mut pixels = edge.pixels.clone();

        let from_coord = (new_nodes[new_from].x, new_nodes[new_from].y);
        let to_coord = (new_nodes[new_to].x, new_nodes[new_to].y);

        if pixels.first() != Some(&from_coord) {
            pixels.insert(0, from_coord);
        }
        if pixels.last() != Some(&to_coord) {
            pixels.push(to_coord);
        }
        pixels.dedup();

        if new_from > new_to {
            std::mem::swap(&mut new_from, &mut new_to);
            pixels.reverse();
        }

        new_edges.push(GraphEdge {
            from: new_from,
            to: new_to,
            pixels,
        });
    }

    StrokeGraph {
        nodes: new_nodes,
        edges: new_edges,
    }
}
#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
