use crate::graph::StrokeGraph;
use std::collections::BTreeMap;

/// Represents a traversed edge in the path: (from_node_idx, to_node_idx, edge_index, is_reverse)
pub type TraversedEdge = (usize, usize, usize, bool);

/// Traverses the stroke graph to find Eulerian paths (Hierholzer) or fallback greedy walks.
pub fn traverse_graph(graph: &StrokeGraph) -> Vec<Vec<TraversedEdge>> {
    let num_nodes = graph.nodes.len();
    let num_edges = graph.edges.len();

    if num_edges == 0 {
        return Vec::new();
    }

    // Build adjacency list: node_idx -> Vec<(neighbor_node_idx, edge_idx, is_reverse)>
    let mut adj: BTreeMap<usize, Vec<(usize, usize, bool)>> = BTreeMap::new();
    for (edge_idx, edge) in graph.edges.iter().enumerate() {
        adj.entry(edge.from)
            .or_default()
            .push((edge.to, edge_idx, false));
        adj.entry(edge.to)
            .or_default()
            .push((edge.from, edge_idx, true));
    }

    // Sort adjacency lists for determinism
    for neighbors in adj.values_mut() {
        neighbors.sort_by(|a, b| {
            if a.0 != b.0 {
                a.0.cmp(&b.0)
            } else {
                a.1.cmp(&b.1)
            }
        });
    }

    // Count odd-degree nodes
    let mut odd_nodes = Vec::new();
    for u in 0..num_nodes {
        let degree = adj.get(&u).map_or(0, |n| n.len());
        if degree % 2 == 1 {
            odd_nodes.push(u);
        }
    }

    if odd_nodes.is_empty() || odd_nodes.len() == 2 {
        // Case 1: Eulerian path or circuit exists!
        let mut used_edges = vec![false; num_edges + 1]; // +1 for the virtual edge
        let start_node;
        let mut virtual_edge = None;

        if odd_nodes.len() == 2 {
            // Add a virtual edge between the two odd-degree nodes to make the graph Eulerian
            let u = odd_nodes[0];
            let v = odd_nodes[1];
            let v_edge_idx = num_edges;

            adj.entry(u).or_default().push((v, v_edge_idx, false));
            adj.entry(v).or_default().push((u, v_edge_idx, true));

            if let Some(n) = adj.get_mut(&u) {
                n.sort_by_key(|x| (x.0, x.1));
            }
            if let Some(n) = adj.get_mut(&v) {
                n.sort_by_key(|x| (x.0, x.1));
            }

            virtual_edge = Some((u, v, v_edge_idx));
            start_node = u;
        } else {
            // Circuit: start at the first node that has edges
            start_node = *adj.keys().next().unwrap_or(&0);
        }

        // Trace cycles helper
        let trace_cycle = |curr: usize,
                           adj_map: &BTreeMap<usize, Vec<(usize, usize, bool)>>,
                           used: &mut [bool]| {
            let mut cycle = Vec::new();
            let mut u = curr;
            loop {
                let mut next_edge = None;
                if let Some(neighbors) = adj_map.get(&u) {
                    for &(v, edge_idx, is_reverse) in neighbors {
                        if !used[edge_idx] {
                            next_edge = Some((v, edge_idx, is_reverse));
                            break;
                        }
                    }
                }
                if let Some((v, edge_idx, is_reverse)) = next_edge {
                    used[edge_idx] = true;
                    cycle.push((u, v, edge_idx, is_reverse));
                    u = v;
                } else {
                    break;
                }
            }
            cycle
        };

        let mut circuit: Vec<(usize, usize, usize, bool)> = Vec::new();
        let initial_cycle = trace_cycle(start_node, &adj, &mut used_edges);
        circuit.extend(initial_cycle);

        loop {
            let mut splice_pos = None;
            for (i, edge) in circuit.iter().enumerate() {
                let u = edge.0;
                let has_unused = adj.get(&u).is_some_and(|neighbors| {
                    neighbors
                        .iter()
                        .any(|&(_, edge_idx, _)| !used_edges[edge_idx])
                });
                if has_unused {
                    splice_pos = Some((i, u));
                    break;
                }
            }
            if splice_pos.is_none() && !circuit.is_empty() {
                let u = circuit.last().unwrap().1;
                let has_unused = adj.get(&u).is_some_and(|neighbors| {
                    neighbors
                        .iter()
                        .any(|&(_, edge_idx, _)| !used_edges[edge_idx])
                });
                if has_unused {
                    splice_pos = Some((circuit.len(), u));
                }
            }

            if let Some((idx, u)) = splice_pos {
                let sub_cycle = trace_cycle(u, &adj, &mut used_edges);
                circuit.splice(idx..idx, sub_cycle);
            } else {
                break;
            }
        }

        if let Some((_, _, v_edge_idx)) = virtual_edge {
            // Find the virtual edge and rotate/split
            if let Some(pos) = circuit
                .iter()
                .position(|&(_, _, edge_idx, _)| edge_idx == v_edge_idx)
            {
                let mut path = Vec::new();
                path.extend(circuit.iter().skip(pos + 1).copied());
                path.extend(circuit.iter().take(pos).copied());
                vec![path]
            } else {
                vec![circuit]
            }
        } else {
            vec![circuit]
        }
    } else {
        // Case 2: Greedy odd-node pairing fallback.
        // We emit the graph's edges as SEPARATE paths, one path per "run"
        // between nodes, via repeated greedy walks that consume edges.
        let mut used_edges = vec![false; num_edges];
        let mut paths = Vec::new();

        loop {
            let mut start_node = None;

            // 1. Prefer starting at a node with an odd number of unused incident edges.
            for u in 0..num_nodes {
                let unused_count = adj.get(&u).map_or(0, |neighbors| {
                    neighbors
                        .iter()
                        .filter(|&&(_, edge_idx, _)| !used_edges[edge_idx])
                        .count()
                });
                if unused_count % 2 == 1 {
                    start_node = Some(u);
                    break;
                }
            }

            // 2. Otherwise start at any node with unused edges.
            if start_node.is_none() {
                for u in 0..num_nodes {
                    let unused_count = adj.get(&u).map_or(0, |neighbors| {
                        neighbors
                            .iter()
                            .filter(|&&(_, edge_idx, _)| !used_edges[edge_idx])
                            .count()
                    });
                    if unused_count > 0 {
                        start_node = Some(u);
                        break;
                    }
                }
            }

            let Some(mut curr) = start_node else {
                break; // No more unused edges
            };

            let mut run = Vec::new();
            loop {
                let mut next_edge = None;
                if let Some(neighbors) = adj.get(&curr) {
                    for &(v, edge_idx, is_reverse) in neighbors {
                        if !used_edges[edge_idx] {
                            next_edge = Some((v, edge_idx, is_reverse));
                            break;
                        }
                    }
                }

                if let Some((v, edge_idx, is_reverse)) = next_edge {
                    used_edges[edge_idx] = true;
                    run.push((curr, v, edge_idx, is_reverse));
                    curr = v;
                } else {
                    break;
                }
            }

            if !run.is_empty() {
                paths.push(run);
            }
        }

        paths
    }
}
