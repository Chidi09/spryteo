use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use resvg::usvg::{Options, Tree};
use spryteo_cli::{run_convert, run_convert_stroke};
use spryteo_core::{ConvertOptions, Mode};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Budget {
    bytes: usize,
    nodes: usize,
}

fn compute_ssim(img1: &[f32], img2: &[f32], width: usize, height: usize) -> f32 {
    let c1 = (0.01 * 255.0) * (0.01 * 255.0);
    let c2 = (0.03 * 255.0) * (0.03 * 255.0);

    let mut ssim_sum = 0.0;
    let mut num_windows = 0;

    let step = 8;
    for y in (0..=height.saturating_sub(8)).step_by(step) {
        for x in (0..=width.saturating_sub(8)).step_by(step) {
            let mut mu_x = 0.0;
            let mut mu_y = 0.0;
            for dy in 0..8 {
                for dx in 0..8 {
                    let idx = (y + dy) * width + (x + dx);
                    mu_x += img1[idx];
                    mu_y += img2[idx];
                }
            }
            mu_x /= 64.0;
            mu_y /= 64.0;

            let mut sigma_x_sq = 0.0;
            let mut sigma_y_sq = 0.0;
            let mut sigma_xy = 0.0;

            for dy in 0..8 {
                for dx in 0..8 {
                    let idx = (y + dy) * width + (x + dx);
                    let vx = img1[idx] - mu_x;
                    let vy = img2[idx] - mu_y;
                    sigma_x_sq += vx * vx;
                    sigma_y_sq += vy * vy;
                    sigma_xy += vx * vy;
                }
            }
            sigma_x_sq /= 64.0;
            sigma_y_sq /= 64.0;
            sigma_xy /= 64.0;

            let numerator = (2.0 * mu_x * mu_y + c1) * (2.0 * sigma_xy + c2);
            let denominator = (mu_x * mu_x + mu_y * mu_y + c1) * (sigma_x_sq + sigma_y_sq + c2);
            let ssim = numerator / denominator;

            ssim_sum += ssim;
            num_windows += 1;
        }
    }

    if num_windows == 0 {
        1.0
    } else {
        ssim_sum / (num_windows as f32)
    }
}

#[test]
fn test_corpus_quality_harness() {
    let update_goldens = env::var("UPDATE_GOLDENS").unwrap_or_default() == "1";
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/corpus");
    let expected_dir = corpus_dir.join("expected");

    // "real" holds user-supplied evaluation images (png/gif, converted in
    // Auto mode). Its SSIM gate starts looser than the synthetic categories;
    // raise it as the engine improves rather than treating 0.80 as the goal.
    // "stroke" exercises the centerline pipeline (run_convert_stroke), which the other
    // categories never touch -- they all go through run_convert's fill pipeline.
    let categories = ["icons", "pixel-art", "line-art", "photos", "real", "stroke"];

    let mut current_budgets: BTreeMap<String, Budget> = BTreeMap::new();
    let budgets_path = expected_dir.join("budgets.json");
    let old_budgets: BTreeMap<String, Budget> = if budgets_path.exists() && !update_goldens {
        let content = fs::read_to_string(&budgets_path).expect("failed to read budgets.json");
        serde_json::from_str(&content).expect("failed to parse budgets.json")
    } else {
        BTreeMap::new()
    };

    for category in categories {
        let cat_dir = corpus_dir.join(category);
        if !cat_dir.exists() {
            continue;
        }

        let mut entries: Vec<_> = fs::read_dir(&cat_dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "png" || ext == "gif")
            })
            .collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let png_path = entry.path();
            let name = png_path.file_stem().unwrap().to_str().unwrap();
            let key = format!("{}/{}", category, name);

            let png_bytes = fs::read(&png_path).expect("failed to read fixture");
            let input_img = image::load_from_memory(&png_bytes)
                .expect("failed to decode input fixture")
                .to_rgba8();
            let width = input_img.width() as usize;
            let height = input_img.height() as usize;

            let mode = match category {
                "icons" => Mode::Icon,
                "pixel-art" => Mode::PixelArt,
                "line-art" => Mode::LineArt,
                "photos" => Mode::Photo,
                _ => Mode::Auto,
            };

            let is_stroke = category == "stroke";
            let opts = ConvertOptions {
                mode,
                stroke: is_stroke,
                ..ConvertOptions::default()
            };

            // 1. Convert. run_convert does not dispatch on opts.stroke -- only
            //    run_pipeline_with_bytes does -- so the stroke path is selected here.
            let convert = |bytes: &[u8]| {
                if is_stroke {
                    run_convert_stroke(bytes, &opts)
                } else {
                    run_convert(bytes, &opts)
                }
            };

            let res = convert(&png_bytes).expect("convert failed");

            // 2. DETERMINISM: convert twice; identical SVG
            let res2 = convert(&png_bytes).expect("second convert failed");
            assert_eq!(res.svg, res2.svg, "DETERMINISM failed for {}", key);

            let golden_path = expected_dir.join(category).join(format!("{}.svg", name));

            if update_goldens {
                fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
                fs::write(&golden_path, &res.svg).expect("failed to write golden svg");
                current_budgets.insert(
                    key.clone(),
                    Budget {
                        bytes: res.svg.len(),
                        nodes: res.meta.stats.node_count,
                    },
                );
            } else {
                // 3. GOLDENS: compare SVG byte-for-byte
                let golden_svg = fs::read_to_string(&golden_path)
                    .unwrap_or_else(|_| panic!("golden file not found: {:?}", golden_path));
                assert_eq!(res.svg, golden_svg, "GOLDEN match failed for {}", key);

                let old_budget = old_budgets
                    .get(&key)
                    .unwrap_or_else(|| panic!("no budget recorded for {}", key));
                let max_bytes = (old_budget.bytes as f64 * 1.10) as usize;
                let max_nodes = old_budget.nodes + 2;

                assert!(
                    res.svg.len() <= max_bytes,
                    "Budget failure for {}: SVG bytes {} exceeded max budget of {} (1.10x of {})",
                    key,
                    res.svg.len(),
                    max_bytes,
                    old_budget.bytes
                );
                assert!(
                    res.meta.stats.node_count <= max_nodes,
                    "Budget failure for {}: SVG nodes {} exceeded max budget of {} ({} + 2)",
                    key,
                    res.meta.stats.node_count,
                    max_nodes,
                    old_budget.nodes
                );
            }

            // 4. RENDER-BACK SSIM
            let opt = Options::default();
            let rtree = Tree::from_str(&res.svg, &opt).unwrap_or_else(|e| {
                panic!(
                    "failed to parse generated SVG for {}: {:?}\nSVG content: {:?}",
                    key, e, res.svg
                )
            });

            let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32).unwrap();
            resvg::render(
                &rtree,
                tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );

            let mut input_luma = vec![0.0f32; width * height];
            for y in 0..height {
                for x in 0..width {
                    let pixel = input_img.get_pixel(x as u32, y as u32);
                    let r = pixel[0] as f32;
                    let g = pixel[1] as f32;
                    let b = pixel[2] as f32;
                    let a = pixel[3] as f32;

                    let alpha_frac = a / 255.0;
                    let r_blend = r * alpha_frac + 255.0 * (1.0 - alpha_frac);
                    let g_blend = g * alpha_frac + 255.0 * (1.0 - alpha_frac);
                    let b_blend = b * alpha_frac + 255.0 * (1.0 - alpha_frac);

                    let mut luma = 0.299 * r_blend + 0.587 * g_blend + 0.114 * b_blend;
                    if category == "line-art" || is_stroke {
                        luma = if luma >= 128.0 { 255.0 } else { 0.0 };
                    }
                    input_luma[y * width + x] = luma;
                }
            }

            let pixmap_data = pixmap.data();
            let mut output_luma = vec![0.0f32; width * height];
            for y in 0..height {
                for x in 0..width {
                    let idx = (y * width + x) * 4;
                    let r = pixmap_data[idx] as f32;
                    let g = pixmap_data[idx + 1] as f32;
                    let b = pixmap_data[idx + 2] as f32;
                    let a = pixmap_data[idx + 3] as f32;

                    let r_blend = r + (255.0 - a);
                    let g_blend = g + (255.0 - a);
                    let b_blend = b + (255.0 - a);

                    let luma = 0.299 * r_blend + 0.587 * g_blend + 0.114 * b_blend;
                    output_luma[y * width + x] = luma;
                }
            }

            let ssim = compute_ssim(&input_luma, &output_luma, width, height);

            let mut total_mae = 0.0f64;
            for y in 0..height {
                for x in 0..width {
                    let pixel = input_img.get_pixel(x as u32, y as u32);
                    let r_in_orig = pixel[0] as f32;
                    let g_in_orig = pixel[1] as f32;
                    let b_in_orig = pixel[2] as f32;
                    let a_in_orig = pixel[3] as f32;

                    let alpha_frac = a_in_orig / 255.0;
                    let r_in = r_in_orig * alpha_frac + 255.0 * (1.0 - alpha_frac);
                    let g_in = g_in_orig * alpha_frac + 255.0 * (1.0 - alpha_frac);
                    let b_in = b_in_orig * alpha_frac + 255.0 * (1.0 - alpha_frac);

                    let idx = (y * width + x) * 4;
                    let r_out_orig = pixmap_data[idx] as f32;
                    let g_out_orig = pixmap_data[idx + 1] as f32;
                    let b_out_orig = pixmap_data[idx + 2] as f32;
                    let a_out_orig = pixmap_data[idx + 3] as f32;

                    let r_out = r_out_orig + (255.0 - a_out_orig);
                    let g_out = g_out_orig + (255.0 - a_out_orig);
                    let b_out = b_out_orig + (255.0 - a_out_orig);

                    let dr = (r_in - r_out).abs();
                    let dg = (g_in - g_out).abs();
                    let db = (b_in - b_out).abs();

                    total_mae += ((dr + dg + db) / 3.0) as f64;
                }
            }
            let mae = total_mae / (width * height) as f64;

            let gate = match category {
                "icons" => 0.92,
                "pixel-art" => 0.92,
                "photos" => 0.85,
                "line-art" => 0.90,
                "real" => 0.80,
                // Centerline tracing. Lowest observed is cross_256 at 0.9293 -- the
                // degree-4 junction, where Zhang-Suen thinning distorts most. Cap
                // extension and junction welding should raise these, not lower them.
                "stroke" => 0.92,
                _ => 0.0,
            };

            eprintln!("SSIM for {}: {:.4} (gate: {:.2})", key, ssim, gate);
            eprintln!("MAE for {}: {:.4}", key, mae);
            assert!(
                ssim >= gate,
                "SSIM gate failure for {}: got {:.4}, expected >= {:.2}\nSVG content: {:?}",
                key,
                ssim,
                gate,
                res.svg
            );
        }
    }

    if update_goldens {
        fs::create_dir_all(&expected_dir).unwrap();
        let budgets_json = serde_json::to_string_pretty(&current_budgets).unwrap();
        fs::write(&budgets_path, budgets_json).expect("failed to write budgets.json");
        println!("Goldens and budgets.json updated successfully.");
    }
}
