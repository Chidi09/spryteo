    use super::*;
    use crate::lattice::Band;

    fn make_lattice(cols: &[(u32, u32)], rows: &[(u32, u32)]) -> Lattice {
        Lattice {
            cols: cols
                .iter()
                .map(|&(s, e)| Band { start: s, end: e })
                .collect(),
            rows: rows
                .iter()
                .map(|&(s, e)| Band { start: s, end: e })
                .collect(),
            confidence: 1.0,
        }
    }

    fn make_cluster(x1: u32, y1: u32, x2: u32, y2: u32) -> Cluster {
        let bbox = Bbox { x1, y1, x2, y2 };
        let pixels = bbox.width() * bbox.height();
        Cluster { bbox, pixels }
    }

    #[test]
    fn test_reconcile_perfect_grid() {
        // 3x2 lattice
        let lat = make_lattice(&[(0, 10), (10, 20), (20, 30)], &[(0, 10), (10, 20)]);
        let clusters = vec![
            make_cluster(1, 1, 9, 9),
            make_cluster(11, 1, 19, 9),
            make_cluster(21, 1, 29, 9),
            make_cluster(1, 11, 9, 19),
            make_cluster(11, 11, 19, 19),
            make_cluster(21, 11, 29, 19),
        ];

        let rec = reconcile(&lat, &clusters);
        assert_eq!(rec.matched, 6);
        assert!(rec.empty_cells.is_empty());
        assert!(rec.straddling.is_empty());
        assert!(rec.orphans.is_empty());
        assert!(rec.multi_cluster_cells.is_empty());
        assert_eq!(rec.agreement, 1.0);
    }

    #[test]
    fn test_reconcile_detects_straddling_cluster() {
        // 2x1 lattice
        let lat = make_lattice(&[(0, 10), (10, 20)], &[(0, 10)]);
        // Wide cluster from x=2 to x=18: area 16x8 = 128.
        // Overlap with cell 0 [0,10): 8x8 = 64 (50% >= 25%).
        // Overlap with cell 1 [10,20): 8x8 = 64 (50% >= 25%).
        let clusters = vec![make_cluster(2, 1, 18, 9)];

        let rec = reconcile(&lat, &clusters);
        assert_eq!(rec.straddling.len(), 1);
        assert_eq!(rec.straddling[0], clusters[0].bbox);
        assert!(rec.agreement < 1.0);
    }

    #[test]
    fn test_reconcile_detects_orphan() {
        let lat = make_lattice(&[(0, 10)], &[(0, 10)]);
        let orphan = make_cluster(50, 50, 60, 60);
        let expected_bbox = orphan.bbox;
        let clusters = vec![orphan];

        let rec = reconcile(&lat, &clusters);
        assert_eq!(rec.orphans.len(), 1);
        assert_eq!(rec.orphans[0], expected_bbox);
        assert_eq!(rec.matched, 0);
        assert_eq!(rec.empty_cells, vec![(0, 0)]);
    }

    #[test]
    fn test_reconcile_detects_empty_cell() {
        // 2x2 lattice
        let lat = make_lattice(&[(0, 10), (10, 20)], &[(0, 10), (10, 20)]);
        let clusters = vec![
            make_cluster(1, 1, 9, 9),   // (0,0)
            make_cluster(11, 1, 19, 9), // (1,0)
            make_cluster(1, 11, 9, 19), // (0,1)
        ];

        let rec = reconcile(&lat, &clusters);
        assert_eq!(rec.matched, 3);
        assert_eq!(rec.empty_cells, vec![(1, 1)]);
    }

    #[test]
    fn test_reconcile_multi_component_icon_is_not_an_error() {
        // 1x1 lattice
        let lat = make_lattice(&[(0, 100)], &[(0, 100)]);
        let clusters = vec![
            make_cluster(5, 5, 15, 15),
            make_cluster(25, 25, 35, 35),
            make_cluster(45, 45, 55, 55),
        ];

        let rec = reconcile(&lat, &clusters);
        assert_eq!(rec.matched, 0);
        assert!(rec.straddling.is_empty());
        assert!(rec.orphans.is_empty());
        assert_eq!(rec.multi_cluster_cells, vec![(0, 0, 3)]);
    }

    #[test]
    fn test_reconcile_empty_inputs() {
        let lat = Lattice {
            cols: vec![],
            rows: vec![],
            confidence: 0.0,
        };
        let rec = reconcile(&lat, &[]);
        assert_eq!(rec.matched, 0);
        assert_eq!(rec.agreement, 0.0);
    }
