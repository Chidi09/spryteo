    use super::*;

    #[test]
    fn test_extends_horizontal_chain_outward() {
        let w = 30;
        let h = 10;
        let mut chain = vec![
            (10.0, 5.0),
            (11.0, 5.0),
            (12.0, 5.0),
            (13.0, 5.0),
            (14.0, 5.0),
            (15.0, 5.0),
        ];
        let dist = vec![2.0; (w * h) as usize];
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 2);
        assert!(
            chain[0].0 < 10.0,
            "Start x should be < 10.0, got {}",
            chain[0].0
        );
        assert!(
            chain[5].0 > 15.0,
            "End x should be > 15.0, got {}",
            chain[5].0
        );
        assert!(chain[5].0 - chain[0].0 > 5.0);
    }

    #[test]
    fn test_extends_diagonal_chain_along_its_own_axis() {
        let w = 30;
        let h = 30;
        let mut chain = vec![
            (10.0, 10.0),
            (11.0, 11.0),
            (12.0, 12.0),
            (13.0, 13.0),
            (14.0, 14.0),
            (15.0, 15.0),
        ];
        let dist = vec![2.0; (w * h) as usize];
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 2);

        let dx_start = chain[0].0 - 10.0;
        let dy_start = chain[0].1 - 10.0;
        let angle_start_deg = (dy_start.atan2(dx_start) * 180.0 / std::f64::consts::PI).abs();
        assert!(
            (dx_start - dy_start).abs() < 0.1,
            "Start extension should be diagonal, got dx={}, dy={}",
            dx_start,
            dy_start
        );
        assert!(angle_start_deg > 125.0 && angle_start_deg < 145.0);

        let dx_end = chain[5].0 - 15.0;
        let dy_end = chain[5].1 - 15.0;
        assert!(
            (dx_end - dy_end).abs() < 0.1,
            "End extension should be diagonal, got dx={}, dy={}",
            dx_end,
            dy_end
        );
    }

    #[test]
    fn test_does_not_extend_closed_chain() {
        let w = 30;
        let h = 30;
        let mut chain = vec![
            (10.0, 10.0),
            (15.0, 10.0),
            (15.0, 15.0),
            (10.0, 15.0),
            (10.0, 10.0),
        ];
        let orig_chain = chain.clone();
        let dist = vec![2.0; (w * h) as usize];
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 0);
        assert_eq!(chain, orig_chain);
    }

    #[test]
    fn test_does_not_extend_short_chain() {
        let w = 30;
        let h = 10;
        let mut chain = vec![(10.0, 5.0), (11.0, 5.0), (12.0, 5.0)];
        let orig_chain = chain.clone();
        let dist = vec![2.0; (w * h) as usize];
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 0);
        assert_eq!(chain, orig_chain);
    }

    #[test]
    fn test_stops_at_ink_boundary() {
        let w = 30;
        let h = 10;
        let mut chain = vec![
            (10.0, 5.0),
            (11.0, 5.0),
            (12.0, 5.0),
            (13.0, 5.0),
            (14.0, 5.0),
            (15.0, 5.0),
        ];
        let dist = vec![5.0; (w * h) as usize];
        let mut ink = vec![false; (w * h) as usize];

        // Ink only present for x in 9..=16
        for y in 0..h {
            for x in 9..=16 {
                ink[(y * w + x) as usize] = true;
            }
        }

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 2);
        assert!(
            (10.0 - chain[0].0) <= 1.5,
            "Start point should stop at ink boundary, moved to {}",
            chain[0].0
        );
        assert!(
            (chain[5].0 - 15.0) <= 1.5,
            "End point should stop at ink boundary, moved to {}",
            chain[5].0
        );
    }

    #[test]
    fn test_does_not_move_when_immediately_off_ink() {
        let w = 30;
        let h = 10;
        let mut chain = vec![
            (10.0, 5.0),
            (11.0, 5.0),
            (12.0, 5.0),
            (13.0, 5.0),
            (14.0, 5.0),
            (15.0, 5.0),
        ];
        let mut dist = vec![2.0; (w * h) as usize];
        let mut ink = vec![false; (w * h) as usize];

        for y in 0..h {
            for x in 10..=15 {
                ink[(y * w + x) as usize] = true;
            }
        }
        ink[(5 * w + 10) as usize] = false;
        // Also set end endpoint DT below threshold so end endpoint doesn't move
        dist[(5 * w + 15) as usize] = 0.2;

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 0);
        assert_eq!(chain[0], (10.0, 5.0));
        assert_eq!(chain[5], (15.0, 5.0));
    }

    #[test]
    fn test_zero_radius_does_not_move() {
        let w = 30;
        let h = 10;
        let mut chain = vec![
            (10.0, 5.0),
            (11.0, 5.0),
            (12.0, 5.0),
            (13.0, 5.0),
            (14.0, 5.0),
            (15.0, 5.0),
        ];
        let dist = vec![0.4; (w * h) as usize];
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 0);
        assert_eq!(chain[0], (10.0, 5.0));
        assert_eq!(chain[5], (15.0, 5.0));
    }

    #[test]
    fn test_out_of_bounds_coordinates_do_not_panic() {
        let w = 10;
        let h = 10;
        let mut chain = vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)];
        let dist = vec![2.0; (w * h) as usize];
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert!(chain[0].0 >= 0.0);
        assert!(chain[0].1 >= 0.0);
        assert!(moved <= 2);
    }

    #[test]
    fn test_return_count_reflects_endpoints_moved() {
        let w = 30;
        let h = 10;
        let mut chain = vec![
            (10.0, 5.0),
            (11.0, 5.0),
            (12.0, 5.0),
            (13.0, 5.0),
            (14.0, 5.0),
            (15.0, 5.0),
        ];
        let mut dist = vec![2.0; (w * h) as usize];
        dist[(5 * w + 15) as usize] = 0.2;
        let ink = vec![true; (w * h) as usize];

        let moved = extend_endpoints(&mut chain, &dist, &ink, w, h);
        assert_eq!(moved, 1);
        assert!(chain[0].0 < 10.0);
        assert_eq!(chain[5], (15.0, 5.0));
    }
