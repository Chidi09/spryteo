    use super::*;

    fn make_mask(width: u32, height: u32, ink: &[(u32, u32)]) -> Mask {
        let mut bits = vec![false; (width * height) as usize];
        for &(x, y) in ink {
            let idx = (y * width + x) as usize;
            bits[idx] = true;
        }
        Mask {
            width,
            height,
            bits,
        }
    }

    #[test]
    fn test_bbox_geometry() {
        let b1 = Bbox {
            x1: 10,
            y1: 10,
            x2: 20,
            y2: 20,
        };
        let b2 = Bbox {
            x1: 15,
            y1: 15,
            x2: 25,
            y2: 25,
        };
        let b_touch = Bbox {
            x1: 20,
            y1: 10,
            x2: 30,
            y2: 20,
        };
        let b_disjoint = Bbox {
            x1: 30,
            y1: 30,
            x2: 40,
            y2: 40,
        };

        assert_eq!(b1.width(), 10);
        assert_eq!(b1.height(), 10);
        assert_eq!(b1.area(), 100);
        assert_eq!(b1.center(), (15.0, 15.0));

        assert!(b1.intersects(&b2));
        assert!(b2.intersects(&b1));
        assert!(!b1.intersects(&b_touch));
        assert!(!b1.intersects(&b_disjoint));

        assert_eq!(b1.intersection_area(&b2), 25);
        assert_eq!(b1.intersection_area(&b_touch), 0);
        assert_eq!(b1.intersection_area(&b_disjoint), 0);

        let u = b1.union(&b2);
        assert_eq!(
            u,
            Bbox {
                x1: 10,
                y1: 10,
                x2: 25,
                y2: 25
            }
        );

        assert!(b1.contains_point(10, 10));
        assert!(b1.contains_point(19, 19));
        assert!(!b1.contains_point(20, 20));
        assert!(!b1.contains_point(9, 10));
    }

    #[test]
    fn test_dilate_radius_zero_is_identity() {
        let mask = make_mask(5, 5, &[(2, 2)]);
        let d = dilate(&mask, 0);
        assert_eq!(d.bits, mask.bits);
    }

    #[test]
    fn test_dilate_expands_single_pixel() {
        let mask_center = make_mask(5, 5, &[(2, 2)]);
        let d_center = dilate(&mask_center, 1);
        assert_eq!(d_center.count(), 9);

        let mask_corner = make_mask(5, 5, &[(0, 0)]);
        let d_corner = dilate(&mask_corner, 1);
        assert_eq!(d_corner.count(), 4);
        assert!(d_corner.get(0, 0));
        assert!(d_corner.get(1, 0));
        assert!(d_corner.get(0, 1));
        assert!(d_corner.get(1, 1));
    }

    #[test]
    fn test_find_clusters_separates_disjoint_blobs() {
        // Two 4x4 squares far apart
        let mut ink = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                ink.push((x, y));
                ink.push((x + 20, y + 20));
            }
        }
        let mask = make_mask(30, 30, &ink);
        let cfg = ClusterConfig {
            dilate: 1,
            min_pixels: 10,
        };
        let clusters = find_clusters(&mask, &cfg);
        assert_eq!(clusters.len(), 2);

        assert_eq!(
            clusters[0].bbox,
            Bbox {
                x1: 0,
                y1: 0,
                x2: 4,
                y2: 4
            }
        );
        assert_eq!(clusters[0].pixels, 16);

        assert_eq!(
            clusters[1].bbox,
            Bbox {
                x1: 20,
                y1: 20,
                x2: 24,
                y2: 24
            }
        );
        assert_eq!(clusters[1].pixels, 16);
    }

    #[test]
    fn test_find_clusters_merges_nearby_strokes_via_dilation() {
        // Two 2x2 squares separated by 2 pixels: (0,0)-(1,1) and (4,0)-(5,1)
        let mut ink = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                ink.push((x, y));
                ink.push((x + 4, y));
            }
        }
        let mask = make_mask(10, 10, &ink);

        let cfg_no_dilate = ClusterConfig {
            dilate: 0,
            min_pixels: 1,
        };
        let clusters_no_dilate = find_clusters(&mask, &cfg_no_dilate);
        assert_eq!(clusters_no_dilate.len(), 2);

        let cfg_dilate = ClusterConfig {
            dilate: 3,
            min_pixels: 1,
        };
        let clusters_dilate = find_clusters(&mask, &cfg_dilate);
        assert_eq!(clusters_dilate.len(), 1);
        assert_eq!(
            clusters_dilate[0].bbox,
            Bbox {
                x1: 0,
                y1: 0,
                x2: 6,
                y2: 2
            }
        );
        assert_eq!(clusters_dilate[0].pixels, 8);
    }

    #[test]
    fn test_find_clusters_drops_noise() {
        let mut ink = vec![(0, 0)]; // 1 pixel speck
        for y in 10..14 {
            for x in 10..14 {
                ink.push((x, y)); // 16 pixels square
            }
        }
        let mask = make_mask(20, 20, &ink);
        let cfg = ClusterConfig {
            dilate: 1,
            min_pixels: 12,
        };
        let clusters = find_clusters(&mask, &cfg);
        assert_eq!(clusters.len(), 1);
        assert_eq!(
            clusters[0].bbox,
            Bbox {
                x1: 10,
                y1: 10,
                x2: 14,
                y2: 14
            }
        );
        assert_eq!(clusters[0].pixels, 16);
    }

    #[test]
    fn test_find_clusters_is_deterministic() {
        let mut ink = Vec::new();
        for y in (0..30).step_by(5) {
            for x in (0..30).step_by(5) {
                ink.push((x, y));
            }
        }
        let mask = make_mask(35, 35, &ink);
        let cfg = ClusterConfig {
            dilate: 1,
            min_pixels: 1,
        };

        let run1 = find_clusters(&mask, &cfg);
        let run2 = find_clusters(&mask, &cfg);

        assert_eq!(run1.len(), run2.len());
        for (c1, c2) in run1.iter().zip(run2.iter()) {
            assert_eq!(c1.bbox, c2.bbox);
            assert_eq!(c1.pixels, c2.pixels);
        }
    }

    #[test]
    fn test_cluster_large_region_does_not_stack_overflow() {
        let mut ink = Vec::new();
        for y in 0..400 {
            for x in 0..400 {
                ink.push((x, y));
            }
        }
        let mask = make_mask(400, 400, &ink);
        let cfg = ClusterConfig {
            dilate: 0,
            min_pixels: 1,
        };
        let clusters = find_clusters(&mask, &cfg);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].pixels, 160000);
    }

    #[test]
    fn test_zero_size_mask_does_not_panic() {
        let mask = Mask {
            width: 0,
            height: 0,
            bits: vec![],
        };
        let cfg = ClusterConfig::default();
        let dilated = dilate(&mask, 2);
        assert_eq!(dilated.width, 0);
        let clusters = find_clusters(&mask, &cfg);
        assert!(clusters.is_empty());
    }
