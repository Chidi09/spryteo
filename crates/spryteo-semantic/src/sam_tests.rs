    use super::*;

    // ── preprocess_for_sam tests ─────────────────────────────────────────

    #[test]
    fn test_preprocess_produces_correct_shape() {
        let image = RasterImage {
            width: 100,
            height: 200,
            pixels: vec![128u8; 100 * 200 * 4],
        };
        let (tensor, info) = preprocess_for_sam(&image);
        assert_eq!(tensor.len(), 3 * 1024 * 1024);
        assert_eq!(info.original_width, 100);
        assert_eq!(info.original_height, 200);
        assert!((info.scale - 5.12).abs() < 1e-5);
    }

    #[test]
    fn test_preprocess_square_image() {
        let image = RasterImage {
            width: 512,
            height: 512,
            pixels: vec![0u8; 512 * 512 * 4],
        };
        let (tensor, info) = preprocess_for_sam(&image);
        assert_eq!(tensor.len(), 3 * 1024 * 1024);
        assert!((info.scale - 2.0).abs() < 1e-5);
        assert_eq!(info.resized_width, 1024);
        assert_eq!(info.resized_height, 1024);
    }

    #[test]
    fn test_preprocess_normalization_values() {
        let image = RasterImage {
            width: 1,
            height: 1,
            pixels: vec![255, 255, 255, 255],
        };
        let (tensor, _) = preprocess_for_sam(&image);
        let r_norm = (255.0 - 123.675) / 58.395;
        let g_norm = (255.0 - 116.28) / 57.12;
        let b_norm = (255.0 - 103.53) / 57.375;
        assert!((tensor[0] - r_norm).abs() < 1e-3);
        assert!((tensor[1024 * 1024] - g_norm).abs() < 1e-3);
        assert!((tensor[2 * 1024 * 1024] - b_norm).abs() < 1e-3);
    }

    #[test]
    fn test_preprocess_padded_region_is_black_normalized() {
        let image = RasterImage {
            width: 1,
            height: 1024,
            pixels: vec![255u8; 1024 * 4],
        };
        let (tensor, _) = preprocess_for_sam(&image);
        let black_r = -123.675 / 58.395;
        let black_g = -116.28 / 57.12;
        let black_b = -103.53 / 57.375;
        assert!((tensor[1] - black_r).abs() < 1e-3);
        assert!((tensor[1024 * 1024 + 1] - black_g).abs() < 1e-3);
        assert!((tensor[2 * 1024 * 1024 + 1] - black_b).abs() < 1e-3);
    }

    // ── generate_grid tests ──────────────────────────────────────────────

    #[test]
    fn test_grid_zero_points() {
        let points = generate_grid(100, 100, 0, 1.0);
        assert!(points.is_empty());
    }

    #[test]
    fn test_grid_one_point() {
        let points = generate_grid(100, 100, 1, 1.0);
        assert_eq!(points.len(), 1);
        assert!((points[0].0 - 50.0).abs() < 1e-5);
        assert!((points[0].1 - 50.0).abs() < 1e-5);
    }

    #[test]
    fn test_grid_deterministic_order() {
        let a = generate_grid(200, 100, 4, 0.5);
        let b = generate_grid(200, 100, 4, 0.5);
        assert_eq!(a.len(), 16);
        assert_eq!(a, b);
    }

    #[test]
    fn test_grid_row_major_order() {
        let points = generate_grid(200, 100, 2, 1.0);
        assert_eq!(points.len(), 4);
        assert!((points[0].0 - 50.0).abs() < 1e-5);
        assert!((points[0].1 - 25.0).abs() < 1e-5);
        assert!((points[1].0 - 150.0).abs() < 1e-5);
        assert!((points[1].1 - 25.0).abs() < 1e-5);
        assert!((points[2].0 - 50.0).abs() < 1e-5);
        assert!((points[2].1 - 75.0).abs() < 1e-5);
    }

    // ── compute_stability_score tests ────────────────────────────────────

    #[test]
    fn test_stability_empty() {
        assert_eq!(compute_stability_score(&[]), 0.0);
    }

    #[test]
    fn test_stability_perfectly_stable() {
        let logits = vec![100.0; 100];
        assert!((compute_stability_score(&logits) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_stability_all_negative_large() {
        let logits = vec![-100.0; 100];
        assert!((compute_stability_score(&logits) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_stability_half_stable_half_unstable() {
        // 2.0: above_low=true, above_high=true → stable
        // 0.0: above_low=true, above_high=false → unstable
        let mut logits = vec![2.0f32; 50];
        logits.extend(vec![0.0f32; 50]);
        let score = compute_stability_score(&logits);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_stability_zero_at_threshold_boundary() {
        let logits = vec![0.0f32; 100];
        assert!((compute_stability_score(&logits) - 0.0).abs() < 1e-6);
    }

    // ── mask_iou tests ──────────────────────────────────────────────────

    #[test]
    fn test_mask_iou_identical() {
        let a = vec![255u8; 100];
        assert!((mask_iou(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_mask_iou_disjoint() {
        let a = vec![255u8, 255u8, 0u8, 0u8];
        let b = vec![0u8, 0u8, 255u8, 255u8];
        assert!((mask_iou(&a, &b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_mask_iou_half_overlap() {
        let a = vec![255u8, 255u8, 0u8, 0u8];
        let b = vec![255u8, 0u8, 255u8, 0u8];
        let iou = mask_iou(&a, &b);
        assert!((iou - 1.0 / 3.0).abs() < 1e-6);
    }

    // ── apply_nms tests ──────────────────────────────────────────────────

    #[test]
    fn test_nms_empty() {
        let kept = apply_nms(vec![], 0.5);
        assert!(kept.is_empty());
    }

    #[test]
    fn test_nms_single_candidate() {
        let candidates = vec![NmsCandidate {
            mask: vec![255u8; 16],
            score: 0.9,
            tie_break: (0, 0),
        }];
        let kept = apply_nms(candidates, 0.7);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn test_nms_two_identical_masks_dedup() {
        let mask = vec![255u8; 16];
        let candidates = vec![
            NmsCandidate {
                mask: mask.clone(),
                score: 0.9,
                tie_break: (0, 0),
            },
            NmsCandidate {
                mask,
                score: 0.85,
                tie_break: (0, 1),
            },
        ];
        let kept = apply_nms(candidates, 0.7);
        assert_eq!(kept.len(), 1);
        assert!((kept[0].1 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn test_nms_two_disjoint_masks_both_survive() {
        let mask_a = vec![255u8, 0, 0, 0];
        let mask_b = vec![0, 0, 0, 255];
        let candidates = vec![
            NmsCandidate {
                mask: mask_a,
                score: 0.9,
                tie_break: (0, 0),
            },
            NmsCandidate {
                mask: mask_b,
                score: 0.8,
                tie_break: (0, 1),
            },
        ];
        let kept = apply_nms(candidates, 0.7);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn test_nms_three_candidates_chain_suppression() {
        let mask_a = vec![255u8, 255, 0, 0];
        let mask_b = vec![255u8, 255, 255, 0];
        let mask_c = vec![0u8, 0, 255, 255];
        let candidates = vec![
            NmsCandidate {
                mask: mask_a,
                score: 0.9,
                tie_break: (0, 0),
            },
            NmsCandidate {
                mask: mask_b,
                score: 0.8,
                tie_break: (0, 1),
            },
            NmsCandidate {
                mask: mask_c,
                score: 0.7,
                tie_break: (0, 2),
            },
        ];
        let kept = apply_nms(candidates, 0.4);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].1 - 0.9).abs() < 1e-6);
        assert!((kept[1].1 - 0.7).abs() < 1e-6);
    }

    // ── upsample_mask tests ──────────────────────────────────────────────

    #[test]
    fn test_upsample_identity() {
        let mask = vec![255u8, 0, 0, 255];
        let info = ResizeInfo {
            original_width: 2,
            original_height: 2,
            scale: 1.0,
            resized_width: 2,
            resized_height: 2,
        };
        let up = upsample_mask(&mask, 2, 2, 2, 2, &info);
        assert_eq!(up.len(), 4);
    }

    #[test]
    fn test_upsample_larger_mask() {
        let mut mask = vec![0u8; 1024 * 1024];
        for y in 0..512 {
            for x in 0..512 {
                mask[y * 1024 + x] = 255;
            }
        }
        let info = ResizeInfo {
            original_width: 512,
            original_height: 512,
            scale: 2.0,
            resized_width: 1024,
            resized_height: 1024,
        };
        let up = upsample_mask(&mask, 1024, 1024, 512, 512, &info);
        assert_eq!(up.len(), 512 * 512);
        assert_eq!(up[0], 255);
    }

    // ── image_to_hwc_f32 tests ───────────────────────────────────────────

    #[test]
    fn test_hwc_produces_correct_shape() {
        let image = RasterImage {
            width: 30,
            height: 20,
            pixels: vec![128u8; 30 * 20 * 4],
        };
        let (data, info) = image_to_hwc_f32(&image);
        assert_eq!(data.len(), 30 * 20 * 3);
        assert_eq!(info.original_width, 30);
        assert_eq!(info.original_height, 20);
        assert!((info.scale - 1.0).abs() < 1e-6);
        assert_eq!(info.resized_width, 30);
        assert_eq!(info.resized_height, 20);
    }

    #[test]
    fn test_hwc_pixel_values_raw_rgb_no_normalization() {
        // image width=3, height=2 → 6 RGBA pixels = 24 bytes
        let mut pixels = vec![0u8; 2 * 3 * 4];
        // pixel (row=0, col=0): R=10 G=20 B=30
        pixels[0] = 10;
        pixels[1] = 20;
        pixels[2] = 30;
        pixels[3] = 255;
        // pixel (row=0, col=1): R=100 G=150 B=200
        pixels[4] = 100;
        pixels[5] = 150;
        pixels[6] = 200;
        pixels[7] = 255;
        // pixel (row=1, col=0): R=200 G=100 B=50  (stride = 3*4 = 12)
        pixels[12] = 200;
        pixels[13] = 100;
        pixels[14] = 50;
        pixels[15] = 255;
        let image = RasterImage {
            width: 3,
            height: 2,
            pixels,
        };
        let (data, _) = image_to_hwc_f32(&image);
        // HWC layout: row 0 [RGB RGB RGB], row 1 [RGB RGB RGB]
        // pixel (0,0): R=10, G=20, B=30
        assert!((data[0] - 10.0).abs() < 1e-5);
        assert!((data[1] - 20.0).abs() < 1e-5);
        assert!((data[2] - 30.0).abs() < 1e-5);
        // pixel (0,1): R=100, G=150, B=200
        assert!((data[3] - 100.0).abs() < 1e-5);
        assert!((data[4] - 150.0).abs() < 1e-5);
        assert!((data[5] - 200.0).abs() < 1e-5);
        // pixel (1,0): R=200, G=100, B=50 (row 1 starts at index 3*cols = 9)
        assert!((data[9] - 200.0).abs() < 1e-5);
        assert!((data[10] - 100.0).abs() < 1e-5);
        assert!((data[11] - 50.0).abs() < 1e-5);
    }

    #[test]
    fn test_hwc_alpha_channel_ignored() {
        let mut pixels = vec![0u8; 4];
        pixels[0] = 42;
        pixels[1] = 43;
        pixels[2] = 44;
        pixels[3] = 0; // fully transparent, should be ignored
        let image = RasterImage {
            width: 1,
            height: 1,
            pixels,
        };
        let (data, _) = image_to_hwc_f32(&image);
        assert_eq!(data.len(), 3);
        assert!((data[0] - 42.0).abs() < 1e-5);
        assert!((data[1] - 43.0).abs() < 1e-5);
        assert!((data[2] - 44.0).abs() < 1e-5);
    }

    // ── AutoMaskOptions defaults ─────────────────────────────────────────

    #[test]
    fn test_automask_options_defaults() {
        let opts = AutoMaskOptions::default();
        assert_eq!(opts.points_per_side, 32);
        assert!((opts.pred_iou_thresh - 0.88).abs() < 1e-6);
        assert!((opts.stability_score_thresh - 0.95).abs() < 1e-6);
        assert!((opts.nms_iou_thresh - 0.7).abs() < 1e-6);
    }

    // ── Integration test (requires real model files) ─────────────────────

    #[test]
    #[ignore]
    fn integration_sam_segment_everything() {
        let encoder_path = std::env::var("SPRYTEO_TEST_SAM_ENCODER").ok();
        let decoder_path = std::env::var("SPRYTEO_TEST_SAM_DECODER").ok();
        let (Some(ep), Some(dp)) = (encoder_path, decoder_path) else {
            eprintln!(
                "Skipping SAM integration test: set SPRYTEO_TEST_SAM_ENCODER and \
                 SPRYTEO_TEST_SAM_DECODER"
            );
            return;
        };

        let mut model = SamModel::load(std::path::Path::new(&ep), std::path::Path::new(&dp))
            .expect("Failed to load SAM model");

        let mut pixels = vec![0u8; 256 * 256 * 4];
        for y in 0..256 {
            for x in 0..256 {
                let dx = x as f32 - 128.0;
                let dy = y as f32 - 128.0;
                if dx * dx + dy * dy <= 64.0 * 64.0 {
                    let idx = (y * 256 + x) * 4;
                    pixels[idx] = 255;
                    pixels[idx + 1] = 255;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 255;
                }
            }
        }
        let image = RasterImage {
            width: 256,
            height: 256,
            pixels,
        };

        let embedding = model.embed(&image).expect("Encoder failed");
        let opts = AutoMaskOptions::default();
        let masks = model
            .segment_everything(&image, &embedding, &opts)
            .expect("segment_everything failed");

        assert!(!masks.is_empty(), "Expected at least one mask");
        for (i, mask) in masks.iter().enumerate() {
            assert_eq!(mask.id, format!("sam-{}", i));
            assert_eq!(mask.width, 256);
            assert_eq!(mask.height, 256);
            assert_eq!(mask.pixels.len(), 256 * 256);
        }
    }
