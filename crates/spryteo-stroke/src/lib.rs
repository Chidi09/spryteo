//! Centerline tracing: skeletonization, stroke graph, Eulerian path traversal.
//!
//! This crate implements Phase 2's core centerline/stroke tracing pipeline,
//! working directly off a `RasterImage` (bypassing colour quantization).
//!
//! Note: Variable-width stroke splitting (splitting where width changes > 30%)
//! is deferred to a future dispatch. The uniform median-width behavior is implemented.
//! Greedy odd-degree node pairing fallback is implemented by emitting separate paths,
//! which is documented in the final report.

pub mod binarize;
pub mod graph;
pub mod prune;
pub mod skeletonize;
pub mod smooth;
pub mod traverse;

use spryteo_core::ir::{CurveSet, RasterImage};

#[derive(Debug, Clone)]
pub struct StrokeResult {
    /// The smoothed vector curves.
    pub curves: CurveSet,
    /// Parallel array of stroke widths (diameter) corresponding to each curve.
    pub widths: Vec<f64>,
    /// Equal to curves.curves.len()
    pub path_count: usize,
}

/// Helper to compute the median of a list of floats.
fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// The main entry point of the spryteo-stroke centerline tracing pipeline.
pub fn trace_stroke(image: &RasterImage, tolerance: f32) -> StrokeResult {
    let mut curves = Vec::new();
    let mut widths = Vec::new();

    if image.width == 0 || image.height == 0 {
        return StrokeResult {
            curves: CurveSet { curves },
            widths,
            path_count: 0,
        };
    }

    // 1. Binarize: Sauvola adaptive thresholding
    let ink_mask = binarize::binarize(image);

    // 2. Skeletonize & Distance Transform
    let dist = skeletonize::compute_distance_transform(&ink_mask, image.width, image.height);
    let mut skeleton = skeletonize::zhang_suen_thinning(&ink_mask, image.width, image.height);

    // 3. Prune spurs
    prune::prune_spurs(&mut skeleton, &dist, image.width, image.height);

    // 4. Build stroke graphs for each connected component
    let mut graphs = graph::build_stroke_graphs(&skeleton, image.width, image.height);

    // Sort graphs by their first node's coordinate for absolute scan-order determinism
    graphs.sort_by(|g1, g2| {
        if g1.nodes.is_empty() && g2.nodes.is_empty() {
            std::cmp::Ordering::Equal
        } else if g1.nodes.is_empty() {
            std::cmp::Ordering::Less
        } else if g2.nodes.is_empty() {
            std::cmp::Ordering::Greater
        } else {
            let n1 = &g1.nodes[0];
            let n2 = &g2.nodes[0];
            if n1.y != n2.y {
                n1.y.cmp(&n2.y)
            } else {
                n1.x.cmp(&n2.x)
            }
        }
    });

    // 5. Traverse and smooth each component
    for g in &graphs {
        let traversal_paths = traverse::traverse_graph(g);
        for path in traversal_paths {
            let mut chain_pixels = Vec::new();
            for &(_, _, edge_idx, is_reverse) in &path {
                let mut edge_p = g.edges[edge_idx].pixels.clone();
                if is_reverse {
                    edge_p.reverse();
                }
                for p in edge_p {
                    if chain_pixels.is_empty() || chain_pixels.last() != Some(&p) {
                        chain_pixels.push(p);
                    }
                }
            }

            if chain_pixels.len() < 2 {
                continue;
            }

            // 6. Compute stroke width (diameter): double the median half-width sampled along the path
            let mut sample_dists = Vec::new();
            for &(px, py) in &chain_pixels {
                let idx = (py * image.width as i32 + px) as usize;
                if idx < dist.len() {
                    sample_dists.push(dist[idx]);
                }
            }
            let stroke_width = 2.0 * median(sample_dists);

            // 7. Curve smoothing
            let chain_float: Vec<(f64, f64)> = chain_pixels
                .iter()
                .map(|&(px, py)| (px as f64, py as f64))
                .collect();

            let curve = smooth::smooth_chain(&chain_float, tolerance);
            curves.push(curve);
            widths.push(stroke_width);
        }
    }

    let path_count = curves.len();
    StrokeResult {
        curves: CurveSet { curves },
        widths,
        path_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spryteo_core::ir::{PathElement, RasterImage};

    fn make_empty_image(w: u32, h: u32) -> RasterImage {
        let pixels = vec![255; (w * h * 4) as usize];
        RasterImage {
            width: w,
            height: h,
            pixels,
        }
    }

    fn draw_pixel(image: &mut RasterImage, x: u32, y: u32) {
        if x >= image.width || y >= image.height {
            return;
        }
        let idx = ((y * image.width + x) * 4) as usize;
        image.pixels[idx] = 0;
        image.pixels[idx + 1] = 0;
        image.pixels[idx + 2] = 0;
        image.pixels[idx + 3] = 255;
    }

    fn draw_line(image: &mut RasterImage, x0: i32, y0: i32, x1: i32, y1: i32, thickness: i32) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs());
        for s in 0..=steps {
            let t = if steps == 0 {
                0.0
            } else {
                s as f64 / steps as f64
            };
            let cx = x0 as f64 + t * (x1 - x0) as f64;
            let cy = y0 as f64 + t * (y1 - y0) as f64;
            for dy in -thickness..=thickness {
                for dx in -thickness..=thickness {
                    if dx * dx + dy * dy <= thickness * thickness {
                        let px = (cx + dx as f64).round() as i32;
                        let py = (cy + dy as f64).round() as i32;
                        if px >= 0 && px < image.width as i32 && py >= 0 && py < image.height as i32
                        {
                            draw_pixel(image, px as u32, py as u32);
                        }
                    }
                }
            }
        }
    }

    fn get_endpoints(segments: &[PathElement]) -> ((f64, f64), (f64, f64)) {
        let start = match segments[0] {
            PathElement::MoveTo(x, y) => (x, y),
            _ => panic!("Expected MoveTo"),
        };
        let end = match *segments.last().unwrap() {
            PathElement::MoveTo(x, y) => (x, y),
            PathElement::LineTo(x, y) => (x, y),
            PathElement::CurveTo(_, _, _, _, x, y) => (x, y),
            PathElement::ClosePath => panic!("Did not expect ClosePath"),
        };
        (start, end)
    }

    fn check_finiteness(segments: &[PathElement]) {
        for elem in segments {
            match *elem {
                PathElement::MoveTo(x, y) => {
                    assert!(x.is_finite());
                    assert!(y.is_finite());
                }
                PathElement::LineTo(x, y) => {
                    assert!(x.is_finite());
                    assert!(y.is_finite());
                }
                PathElement::CurveTo(x1, y1, x2, y2, x3, y3) => {
                    assert!(x1.is_finite());
                    assert!(y1.is_finite());
                    assert!(x2.is_finite());
                    assert!(y2.is_finite());
                    assert!(x3.is_finite());
                    assert!(y3.is_finite());
                }
                PathElement::ClosePath => {}
            }
        }
    }

    #[test]
    fn test_straight_line() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res = trace_stroke(&img, 0.5);
        assert_eq!(res.path_count, 1);
        let segments = &res.curves.curves[0].segments;
        check_finiteness(segments);

        let (start, end) = get_endpoints(segments);
        let d1 = (start.0 - 5.0).abs()
            + (start.1 - 10.0).abs()
            + (end.0 - 25.0).abs()
            + (end.1 - 10.0).abs();
        let d2 = (start.0 - 25.0).abs()
            + (start.1 - 10.0).abs()
            + (end.0 - 5.0).abs()
            + (end.1 - 10.0).abs();
        assert!(
            d1 < 4.0 || d2 < 4.0,
            "Endpoints did not match (start: {:?}, end: {:?})",
            start,
            end
        );
    }

    #[test]
    fn test_plus_shape() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 25, 15, 0);
        draw_line(&mut img, 15, 5, 15, 25, 0);

        let res = trace_stroke(&img, 0.5);
        assert!(res.path_count >= 2, "Path count was {}", res.path_count);
        for curve in &res.curves.curves {
            check_finiteness(&curve.segments);
        }
    }

    #[test]
    fn test_tiny_dot() {
        let mut img = make_empty_image(16, 16);
        draw_pixel(&mut img, 8, 8);
        let res = trace_stroke(&img, 0.5);
        assert!(res.path_count <= 1);
        for curve in &res.curves.curves {
            check_finiteness(&curve.segments);
        }
    }

    #[test]
    fn test_stroke_width() {
        let mut img_thin = make_empty_image(32, 32);
        draw_line(&mut img_thin, 5, 16, 25, 16, 0);
        let res_thin = trace_stroke(&img_thin, 0.5);
        assert_eq!(res_thin.path_count, 1);
        let w_thin = res_thin.widths[0];

        let mut img_fat = make_empty_image(32, 32);
        draw_line(&mut img_fat, 5, 16, 25, 16, 2);
        let res_fat = trace_stroke(&img_fat, 0.5);
        assert_eq!(res_fat.path_count, 1);
        let w_fat = res_fat.widths[0];

        assert!(
            w_fat > w_thin * 1.5,
            "Fat width ({}) should be significantly larger than thin width ({})",
            w_fat,
            w_thin
        );
    }

    #[test]
    fn test_spur_pruning() {
        let mut img_short = make_empty_image(32, 32);
        draw_line(&mut img_short, 5, 10, 25, 10, 0);
        draw_pixel(&mut img_short, 15, 9);
        draw_pixel(&mut img_short, 15, 8);

        let res_short = trace_stroke(&img_short, 0.5);
        assert_eq!(res_short.path_count, 1);

        let mut img_long = make_empty_image(32, 32);
        draw_line(&mut img_long, 5, 10, 25, 10, 0);
        for y in 5..=9 {
            draw_pixel(&mut img_long, 15, y as u32);
        }

        let res_long = trace_stroke(&img_long, 0.5);
        assert!(
            res_long.path_count >= 2,
            "Long spur did not survive, path count was {}",
            res_long.path_count
        );
    }

    #[test]
    fn test_determinism() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 25, 15, 1);
        draw_line(&mut img, 15, 5, 15, 25, 1);

        let res1 = trace_stroke(&img, 0.5);
        let res2 = trace_stroke(&img, 0.5);

        let json1 = serde_json::to_vec(&res1.curves).unwrap();
        let json2 = serde_json::to_vec(&res2.curves).unwrap();
        assert_eq!(json1, json2);
        assert_eq!(res1.widths, res2.widths);
    }
}
