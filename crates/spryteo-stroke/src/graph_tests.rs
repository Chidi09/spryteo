    use super::*;

    #[test]
    fn test_merge_close_junctions_basic() {
        let width = 32;
        let height = 32;
        let dist = vec![3.0f64; (width * height) as usize];

        let nodes = vec![
            GraphNode { x: 5, y: 5 },   // 0: E1
            GraphNode { x: 5, y: 15 },  // 1: E2
            GraphNode { x: 10, y: 10 }, // 2: J1
            GraphNode { x: 12, y: 10 }, // 3: J2
            GraphNode { x: 15, y: 5 },  // 4: E3
            GraphNode { x: 15, y: 15 }, // 5: E4
        ];

        let edges = vec![
            GraphEdge {
                from: 0,
                to: 2,
                pixels: vec![(5, 5), (7, 7), (10, 10)],
            },
            GraphEdge {
                from: 1,
                to: 2,
                pixels: vec![(5, 15), (7, 13), (10, 10)],
            },
            GraphEdge {
                from: 2,
                to: 3,
                pixels: vec![(10, 10), (11, 10), (12, 10)],
            },
            GraphEdge {
                from: 3,
                to: 4,
                pixels: vec![(12, 10), (13, 7), (15, 5)],
            },
            GraphEdge {
                from: 3,
                to: 5,
                pixels: vec![(12, 10), (13, 13), (15, 15)],
            },
        ];

        let graph = StrokeGraph { nodes, edges };
        let merged = merge_close_junctions(graph, &dist, width);

        // Exactly 5 nodes after merging J1 and J2 into (11, 10)
        assert_eq!(merged.nodes.len(), 5);
        let centroid_node = merged.nodes.iter().find(|n| n.x == 11 && n.y == 10);
        assert!(
            centroid_node.is_some(),
            "Centroid node (11, 10) should exist"
        );

        // 4 long edges remaining; short edge dropped
        assert_eq!(merged.edges.len(), 4);

        // Every edge satisfies invariant
        for edge in &merged.edges {
            let from_coord = (merged.nodes[edge.from].x, merged.nodes[edge.from].y);
            let to_coord = (merged.nodes[edge.to].x, merged.nodes[edge.to].y);
            assert_eq!(*edge.pixels.first().unwrap(), from_coord);
            assert_eq!(*edge.pixels.last().unwrap(), to_coord);
            assert!(edge.from <= edge.to);
        }
    }

    #[test]
    fn test_merge_close_junctions_no_merge_thin_stroke() {
        let width = 32;
        let height = 32;
        let dist = vec![0.5f64; (width * height) as usize];

        let nodes = vec![
            GraphNode { x: 5, y: 5 },   // 0: E1
            GraphNode { x: 5, y: 15 },  // 1: E2
            GraphNode { x: 10, y: 10 }, // 2: J1
            GraphNode { x: 12, y: 10 }, // 3: J2
            GraphNode { x: 15, y: 5 },  // 4: E3
            GraphNode { x: 15, y: 15 }, // 5: E4
        ];

        let edges = vec![
            GraphEdge {
                from: 0,
                to: 2,
                pixels: vec![(5, 5), (7, 7), (10, 10)],
            },
            GraphEdge {
                from: 1,
                to: 2,
                pixels: vec![(5, 15), (7, 13), (10, 10)],
            },
            GraphEdge {
                from: 2,
                to: 3,
                pixels: vec![(10, 10), (11, 10), (12, 10)],
            },
            GraphEdge {
                from: 3,
                to: 4,
                pixels: vec![(12, 10), (13, 7), (15, 5)],
            },
            GraphEdge {
                from: 3,
                to: 5,
                pixels: vec![(12, 10), (13, 13), (15, 15)],
            },
        ];

        let graph = StrokeGraph { nodes, edges };
        let merged = merge_close_junctions(graph, &dist, width);

        // Nothing merges; nodes and edges identical
        assert_eq!(merged.nodes.len(), 6);
        assert_eq!(merged.edges.len(), 5);
        for (n1, n2) in merged.nodes.iter().zip(
            [
                GraphNode { x: 5, y: 5 },
                GraphNode { x: 5, y: 15 },
                GraphNode { x: 10, y: 10 },
                GraphNode { x: 12, y: 10 },
                GraphNode { x: 15, y: 5 },
                GraphNode { x: 15, y: 15 },
            ]
            .iter(),
        ) {
            assert_eq!(n1, n2);
        }
    }

    #[test]
    fn test_merge_close_junctions_long_loop_edge_in_same_cluster() {
        let width = 32;
        let height = 32;
        let dist = vec![1.5f64; (width * height) as usize]; // width threshold = 3.0

        let nodes = vec![
            GraphNode { x: 5, y: 5 },   // 0: E1
            GraphNode { x: 5, y: 15 },  // 1: E2
            GraphNode { x: 10, y: 10 }, // 2: J1
            GraphNode { x: 12, y: 10 }, // 3: J2
            GraphNode { x: 15, y: 5 },  // 4: E3
        ];

        let edges = vec![
            GraphEdge {
                from: 0,
                to: 2,
                pixels: vec![(5, 5), (7, 7), (10, 10)],
            },
            GraphEdge {
                from: 1,
                to: 2,
                pixels: vec![(5, 15), (7, 13), (10, 10)],
            },
            GraphEdge {
                // Short candidate edge (len 3, path 2 < 3)
                from: 2,
                to: 3,
                pixels: vec![(10, 10), (11, 10), (12, 10)],
            },
            GraphEdge {
                // Long non-candidate edge between same junctions (len 5, path 4 >= 3)
                from: 2,
                to: 3,
                pixels: vec![(10, 10), (10, 12), (11, 13), (12, 12), (12, 10)],
            },
            GraphEdge {
                from: 3,
                to: 4,
                pixels: vec![(12, 10), (13, 7), (15, 5)],
            },
        ];

        let graph = StrokeGraph { nodes, edges };
        let merged = merge_close_junctions(graph, &dist, width);

        // J1 and J2 merged into centroid (11, 10)
        assert_eq!(merged.nodes.len(), 4);

        // Check for loop edge (from == to)
        let loop_edge = merged.edges.iter().find(|e| e.from == e.to);
        assert!(
            loop_edge.is_some(),
            "Long non-candidate edge between merged junctions should become loop edge"
        );
        let le = loop_edge.unwrap();

        // Check that intermediate pixels survived
        assert!(le.pixels.contains(&(10, 12)));
        assert!(le.pixels.contains(&(11, 13)));
        assert!(le.pixels.contains(&(12, 12)));
    }

    #[test]
    fn test_merge_close_junctions_determinism() {
        let width = 32;
        let height = 32;
        let dist = vec![3.0f64; (width * height) as usize];

        let nodes = vec![
            GraphNode { x: 5, y: 5 },
            GraphNode { x: 5, y: 15 },
            GraphNode { x: 10, y: 10 },
            GraphNode { x: 12, y: 10 },
            GraphNode { x: 15, y: 5 },
            GraphNode { x: 15, y: 15 },
        ];

        let edges = vec![
            GraphEdge {
                from: 0,
                to: 2,
                pixels: vec![(5, 5), (7, 7), (10, 10)],
            },
            GraphEdge {
                from: 1,
                to: 2,
                pixels: vec![(5, 15), (7, 13), (10, 10)],
            },
            GraphEdge {
                from: 2,
                to: 3,
                pixels: vec![(10, 10), (11, 10), (12, 10)],
            },
            GraphEdge {
                from: 3,
                to: 4,
                pixels: vec![(12, 10), (13, 7), (15, 5)],
            },
            GraphEdge {
                from: 3,
                to: 5,
                pixels: vec![(12, 10), (13, 13), (15, 15)],
            },
        ];

        let graph1 = StrokeGraph {
            nodes: nodes.clone(),
            edges: edges.clone(),
        };
        let m1 = merge_close_junctions(graph1, &dist, width);

        let graph2 = StrokeGraph { nodes, edges };
        let m2 = merge_close_junctions(graph2, &dist, width);

        assert_eq!(m1.nodes, m2.nodes);
        assert_eq!(m1.edges.len(), m2.edges.len());
        for (e1, e2) in m1.edges.iter().zip(m2.edges.iter()) {
            assert_eq!(e1.from, e2.from);
            assert_eq!(e1.to, e2.to);
            assert_eq!(e1.pixels, e2.pixels);
        }

        // Re-merging the already merged graph yields identical result
        let m3 = merge_close_junctions(m1, &dist, width);
        assert_eq!(m2.nodes, m3.nodes);
        assert_eq!(m2.edges.len(), m3.edges.len());
    }
