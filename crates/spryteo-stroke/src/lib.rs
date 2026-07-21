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
pub mod caps;
pub mod graph;
pub mod prune;
pub mod skeletonize;
pub mod smooth;
pub mod traverse;

use spryteo_core::ir::{Curve, CurveSet, RasterImage};

/// Where the ink mask comes from.
#[derive(Debug, Clone)]
pub enum InkSource {
    /// Sauvola adaptive thresholding on luminance -- what `trace_stroke` uses.
    Luminance,
    /// A caller-supplied mask, one bool per pixel, row-major, length
    /// width*height. Used when the caller has better information than
    /// luminance can provide (e.g. a chroma-derived mask for coloured line art
    /// on a light background).
    Mask(Vec<bool>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComponentWidth {
    pub component: usize,    // scan-order index
    pub max_full_width: f64, // 2.0 * max distance-transform value in the component
    pub pixels: usize,
}

pub fn component_widths(mask: &[bool], width: u32, height: u32) -> Vec<ComponentWidth> {
    if width == 0 || height == 0 || mask.len() != (width as usize * height as usize) {
        return Vec::new();
    }

    let dist = skeletonize::compute_distance_transform(mask, width, height);

    let w = width as i32;
    let h = height as i32;
    let size = (width * height) as usize;
    let mut visited = vec![false; size];
    let mut result = Vec::new();
    let mut component_idx = 0;

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if mask[idx] && !visited[idx] {
                visited[idx] = true;
                let mut comp_pixel_count = 0;
                let mut max_dist = dist[idx];
                let mut queue = std::collections::VecDeque::new();
                queue.push_back((x, y));
                comp_pixel_count += 1;

                while let Some((cx, cy)) = queue.pop_front() {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = cx + dx;
                            let ny = cy + dy;
                            if nx >= 0 && nx < w && ny >= 0 && ny < h {
                                let n_idx = (ny * w + nx) as usize;
                                if mask[n_idx] && !visited[n_idx] {
                                    visited[n_idx] = true;
                                    queue.push_back((nx, ny));
                                    comp_pixel_count += 1;
                                    if dist[n_idx] > max_dist {
                                        max_dist = dist[n_idx];
                                    }
                                }
                            }
                        }
                    }
                }

                result.push(ComponentWidth {
                    component: component_idx,
                    max_full_width: 2.0 * max_dist,
                    pixels: comp_pixel_count,
                });
                component_idx += 1;
            }
        }
    }

    result
}

#[derive(Debug, Clone)]
pub struct StrokeOptions {
    pub tolerance: f32,
    pub ink: InkSource,
    /// Skip spur pruning. Pruning removes skeleton hair on large shapes but
    /// also deletes small genuine features, so the caller chooses.
    pub prune_spurs: bool,
    /// Connected components with fewer than this many ink pixels are dropped
    /// before graph building.
    pub min_component_pixels: usize,
    /// Extend open stroke endpoints outward along local tangent by distance transform radius.
    pub extend_caps: bool,
    /// Components whose max inscribed full width (2 x max DT) is >= this value are
    /// routed to `filled_mask` instead of being skeleton-traced. None disables routing.
    pub route_fill_above: Option<f64>,
}

impl Default for StrokeOptions {
    fn default() -> Self {
        Self {
            tolerance: 1.0,
            ink: InkSource::Luminance,
            prune_spurs: true,
            min_component_pixels: 0,
            extend_caps: false,
            route_fill_above: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StrokePath {
    pub curve: Curve,
    /// Diameter: 2.0 * median(half-widths), same as trace_stroke computes.
    pub width: f64,
    /// Per-chain-pixel HALF-widths straight from the distance transform, in
    /// traversal order, not collapsed. Empty only for an empty chain.
    pub width_profile: Vec<f64>,
    /// Index into the sorted component list this path came from.
    pub component: usize,
    /// True when the chain's first and last pixel coincide.
    pub closed: bool,
    /// First and last point of the chain, in pixel coordinates.
    pub endpoints: [(f64, f64); 2],
}

#[derive(Debug, Clone)]
pub struct StrokeResultEx {
    pub paths: Vec<StrokePath>,
    /// Number of components that survived `min_component_pixels`.
    pub component_count: usize,
    /// Union of ink pixels belonging to components routed to fill. Empty Vec when no
    /// routing happened (route_fill_above None or no component crossed the threshold).
    pub filled_mask: Vec<bool>,
    /// Number of components routed to fill.
    pub filled_count: usize,
}

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
    let opts = StrokeOptions {
        tolerance,
        ..StrokeOptions::default()
    };
    let ex = trace_stroke_ex(image, &opts);
    let mut curves = Vec::with_capacity(ex.paths.len());
    let mut widths = Vec::with_capacity(ex.paths.len());
    for p in ex.paths {
        curves.push(p.curve);
        widths.push(p.width);
    }
    let path_count = curves.len();
    StrokeResult {
        curves: CurveSet { curves },
        widths,
        path_count,
    }
}

pub fn trace_stroke_ex(image: &RasterImage, opts: &StrokeOptions) -> StrokeResultEx {
    let mut paths = Vec::new();

    if image.width == 0 || image.height == 0 {
        return StrokeResultEx {
            paths,
            component_count: 0,
            filled_mask: Vec::new(),
            filled_count: 0,
        };
    }

    // 1. Ink mask
    let ink_mask = match &opts.ink {
        InkSource::Luminance => binarize::binarize(image),
        InkSource::Mask(m) => {
            if m.len() != (image.width as usize * image.height as usize) {
                return StrokeResultEx {
                    paths,
                    component_count: 0,
                    filled_mask: Vec::new(),
                    filled_count: 0,
                };
            }
            m.clone()
        }
    };

    // 2. Skeletonize & Distance Transform
    let dist = skeletonize::compute_distance_transform(&ink_mask, image.width, image.height);

    let mut filled_mask = Vec::new();
    let mut filled_count = 0;

    let reduced_ink_mask;
    let tracing_mask = if let Some(t) = opts.route_fill_above {
        let w = image.width as i32;
        let h = image.height as i32;
        let size = (image.width * image.height) as usize;
        let mut visited = vec![false; size];
        let mut mask_copy = ink_mask.clone();

        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                if mask_copy[idx] && !visited[idx] {
                    visited[idx] = true;
                    let mut comp_pixels = Vec::new();
                    let mut queue = std::collections::VecDeque::new();
                    queue.push_back((x, y));
                    comp_pixels.push(idx);

                    let mut max_dist = dist[idx];

                    while let Some((cx, cy)) = queue.pop_front() {
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                if dx == 0 && dy == 0 {
                                    continue;
                                }
                                let nx = cx + dx;
                                let ny = cy + dy;
                                if nx >= 0 && nx < w && ny >= 0 && ny < h {
                                    let n_idx = (ny * w + nx) as usize;
                                    if mask_copy[n_idx] && !visited[n_idx] {
                                        visited[n_idx] = true;
                                        queue.push_back((nx, ny));
                                        comp_pixels.push(n_idx);
                                        if dist[n_idx] > max_dist {
                                            max_dist = dist[n_idx];
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if 2.0 * max_dist >= t {
                        if filled_mask.is_empty() {
                            filled_mask = vec![false; size];
                        }
                        for &px_idx in &comp_pixels {
                            filled_mask[px_idx] = true;
                            mask_copy[px_idx] = false;
                        }
                        filled_count += 1;
                    }
                }
            }
        }
        reduced_ink_mask = mask_copy;
        &reduced_ink_mask
    } else {
        &ink_mask
    };

    let mut skeleton = skeletonize::zhang_suen_thinning(tracing_mask, image.width, image.height);

    // 3. Prune spurs
    if opts.prune_spurs {
        prune::prune_spurs(&mut skeleton, &dist, image.width, image.height);
    }

    // 4. min_component_pixels filtering on INK mask
    if opts.min_component_pixels > 0 {
        let w = image.width as i32;
        let h = image.height as i32;
        let mut visited = vec![false; (image.width * image.height) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                if tracing_mask[idx] && !visited[idx] {
                    visited[idx] = true;
                    let mut comp_pixels = Vec::new();
                    let mut queue = std::collections::VecDeque::new();
                    queue.push_back((x, y));
                    comp_pixels.push(idx);

                    while let Some((cx, cy)) = queue.pop_front() {
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                if dx == 0 && dy == 0 {
                                    continue;
                                }
                                let nx = cx + dx;
                                let ny = cy + dy;
                                if nx >= 0 && nx < w && ny >= 0 && ny < h {
                                    let n_idx = (ny * w + nx) as usize;
                                    if tracing_mask[n_idx] && !visited[n_idx] {
                                        visited[n_idx] = true;
                                        queue.push_back((nx, ny));
                                        comp_pixels.push(n_idx);
                                    }
                                }
                            }
                        }
                    }

                    if comp_pixels.len() < opts.min_component_pixels {
                        for &px_idx in &comp_pixels {
                            skeleton[px_idx] = false;
                        }
                    }
                }
            }
        }
    }

    // 5. Build stroke graphs for each connected component
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

    let component_count = graphs.len();

    // 6. Traverse and smooth each component
    for (comp_idx, g) in graphs.iter().enumerate() {
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

            let mut sample_dists = Vec::new();
            for &(px, py) in &chain_pixels {
                let idx = (py * image.width as i32 + px) as usize;
                if idx < dist.len() {
                    sample_dists.push(dist[idx]);
                }
            }
            let stroke_width = 2.0 * median(sample_dists.clone());

            let mut chain_float: Vec<(f64, f64)> = chain_pixels
                .iter()
                .map(|&(px, py)| (px as f64, py as f64))
                .collect();

            if opts.extend_caps {
                caps::extend_endpoints(
                    &mut chain_float,
                    &dist,
                    tracing_mask,
                    image.width,
                    image.height,
                );
            }

            let curve = smooth::smooth_chain(&chain_float, opts.tolerance);
            let closed = chain_pixels.first() == chain_pixels.last();
            let p_first = chain_float.first().unwrap();
            let p_last = chain_float.last().unwrap();
            let endpoints = [*p_first, *p_last];

            paths.push(StrokePath {
                curve,
                width: stroke_width,
                width_profile: sample_dists,
                component: comp_idx,
                closed,
                endpoints,
            });
        }
    }

    StrokeResultEx {
        paths,
        component_count,
        filled_mask,
        filled_count,
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

    #[test]
    fn test_trace_stroke_ex_matches_trace_stroke_on_defaults() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res = trace_stroke(&img, 1.0);
        let res_ex = trace_stroke_ex(&img, &StrokeOptions::default());

        assert_eq!(res.path_count, res_ex.paths.len());
        for (w1, p2) in res.widths.iter().zip(res_ex.paths.iter()) {
            assert_eq!(*w1, p2.width);
        }
    }

    #[test]
    fn test_ink_source_mask_is_used_instead_of_luminance() {
        let w = 32;
        let h = 32;
        let pixels = vec![240; (w * h * 4) as usize];
        let img = RasterImage {
            width: w,
            height: h,
            pixels,
        };

        let res_lum = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res_lum.paths.len(), 0);

        let mut mask = vec![false; (w * h) as usize];
        for x in 5..=25 {
            mask[(10 * w + x) as usize] = true;
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            ..StrokeOptions::default()
        };
        let res_mask = trace_stroke_ex(&img, &opts);
        assert!(!res_mask.paths.is_empty());
    }

    #[test]
    fn test_ink_source_mask_wrong_length_returns_empty() {
        let img = make_empty_image(32, 32);
        let wrong_mask = vec![true; 10];
        let opts = StrokeOptions {
            ink: InkSource::Mask(wrong_mask),
            ..StrokeOptions::default()
        };
        let res = trace_stroke_ex(&img, &opts);
        assert_eq!(res.paths.len(), 0);
        assert_eq!(res.component_count, 0);
    }

    #[test]
    fn test_component_indices_are_assigned() {
        let mut img = make_empty_image(64, 64);
        draw_line(&mut img, 5, 5, 15, 5, 0);
        draw_line(&mut img, 5, 25, 15, 25, 0);
        draw_line(&mut img, 5, 45, 15, 45, 0);

        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res.component_count, 3);
        let mut components: Vec<usize> = res.paths.iter().map(|p| p.component).collect();
        components.sort_unstable();
        components.dedup();
        assert_eq!(components, vec![0, 1, 2]);
    }

    #[test]
    fn test_min_component_pixels_drops_small_components() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);
        draw_pixel(&mut img, 2, 2);
        draw_pixel(&mut img, 3, 2);

        let opts_default = StrokeOptions {
            prune_spurs: false,
            min_component_pixels: 0,
            ..StrokeOptions::default()
        };
        let res_default = trace_stroke_ex(&img, &opts_default);

        let opts_filtered = StrokeOptions {
            prune_spurs: false,
            min_component_pixels: 5,
            ..StrokeOptions::default()
        };
        let res_filtered = trace_stroke_ex(&img, &opts_filtered);

        assert!(
            res_default.component_count > res_filtered.component_count,
            "default should have more components ({}) than filtered ({})",
            res_default.component_count,
            res_filtered.component_count
        );
    }

    #[test]
    fn test_width_profile_is_not_collapsed() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 12, 15, 0);
        draw_line(&mut img, 13, 15, 25, 15, 2);

        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert!(!res.paths.is_empty());
        let path = &res.paths[0];
        assert!(path.width_profile.len() > 2);

        let min_val = path
            .width_profile
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
        let max_val = path
            .width_profile
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max_val > min_val,
            "width profile should contain distinct values"
        );

        let expected_median_width = 2.0 * median(path.width_profile.clone());
        assert!((path.width - expected_median_width).abs() < 1e-6);
    }

    #[test]
    fn test_closed_flag_for_loop() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 10, 10, 20, 10, 0);
        draw_line(&mut img, 20, 10, 20, 20, 0);
        draw_line(&mut img, 20, 20, 10, 20, 0);
        draw_line(&mut img, 10, 20, 10, 10, 0);

        let res_closed = trace_stroke_ex(&img, &StrokeOptions::default());
        assert!(
            res_closed.paths.iter().any(|p| p.closed),
            "Loop image should yield at least one closed path"
        );

        let mut img_line = make_empty_image(32, 32);
        draw_line(&mut img_line, 5, 10, 25, 10, 0);
        let res_line = trace_stroke_ex(&img_line, &StrokeOptions::default());
        assert!(
            res_line.paths.iter().all(|p| !p.closed),
            "Open line should yield no closed paths"
        );
    }

    #[test]
    fn test_endpoints_match_chain_ends() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 10, 25, 10, 0);

        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res.paths.len(), 1);
        let path = &res.paths[0];

        let (curve_start, curve_end) = get_endpoints(&path.curve.segments);
        assert_eq!(path.endpoints[0], curve_start);
        assert_eq!(path.endpoints[1], curve_end);
    }

    #[test]
    fn test_trace_stroke_ex_is_deterministic() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 15, 25, 15, 1);
        draw_line(&mut img, 15, 5, 15, 25, 1);

        let res1 = trace_stroke_ex(&img, &StrokeOptions::default());
        let res2 = trace_stroke_ex(&img, &StrokeOptions::default());

        assert_eq!(res1.paths.len(), res2.paths.len());
        assert_eq!(res1.component_count, res2.component_count);

        for (p1, p2) in res1.paths.iter().zip(res2.paths.iter()) {
            assert_eq!(p1.width, p2.width);
            assert_eq!(p1.width_profile, p2.width_profile);
            assert_eq!(p1.component, p2.component);
            assert_eq!(p1.closed, p2.closed);
            assert_eq!(p1.endpoints, p2.endpoints);
        }
    }

    #[test]
    fn test_zero_size_image_does_not_panic() {
        let img = RasterImage {
            width: 0,
            height: 0,
            pixels: vec![],
        };
        let res = trace_stroke_ex(&img, &StrokeOptions::default());
        assert_eq!(res.paths.len(), 0);
        assert_eq!(res.component_count, 0);
    }

    #[test]
    fn test_extend_caps_lengthens_open_stroke() {
        let mut img = make_empty_image(32, 32);
        draw_line(&mut img, 5, 16, 25, 16, 1);

        let opts_off = StrokeOptions {
            extend_caps: false,
            ..StrokeOptions::default()
        };
        let res_off = trace_stroke_ex(&img, &opts_off);

        let opts_on = StrokeOptions {
            extend_caps: true,
            ..StrokeOptions::default()
        };
        let res_on = trace_stroke_ex(&img, &opts_on);

        assert_eq!(res_off.paths.len(), res_on.paths.len());
        assert!(!res_off.paths.is_empty());

        let ep_off = res_off.paths[0].endpoints;
        let ep_on = res_on.paths[0].endpoints;

        let dist_off = (ep_off[0].0 - ep_off[1].0).hypot(ep_off[0].1 - ep_off[1].1);
        let dist_on = (ep_on[0].0 - ep_on[1].0).hypot(ep_on[0].1 - ep_on[1].1);

        assert!(
            dist_on > dist_off,
            "Endpoints with extend_caps=true ({}) should be further apart than extend_caps=false ({})",
            dist_on,
            dist_off
        );

        let mut img_loop = make_empty_image(32, 32);
        draw_line(&mut img_loop, 10, 10, 20, 10, 0);
        draw_line(&mut img_loop, 20, 10, 20, 20, 0);
        draw_line(&mut img_loop, 20, 20, 10, 20, 0);
        draw_line(&mut img_loop, 10, 20, 10, 10, 0);

        let res_loop_off = trace_stroke_ex(&img_loop, &opts_off);
        let res_loop_on = trace_stroke_ex(&img_loop, &opts_on);

        assert_eq!(res_loop_off.paths.len(), res_loop_on.paths.len());
        for (p_off, p_on) in res_loop_off.paths.iter().zip(res_loop_on.paths.iter()) {
            assert_eq!(p_off.closed, p_on.closed);
        }
    }

    #[test]
    fn test_trace_stroke_delegation_is_output_identical() {
        let tolerance = 0.5;

        // 1. Open line
        let mut img_line = make_empty_image(32, 32);
        draw_line(&mut img_line, 5, 10, 25, 10, 0);

        // 2. Closed loop
        let mut img_loop = make_empty_image(32, 32);
        draw_line(&mut img_loop, 10, 10, 20, 10, 0);
        draw_line(&mut img_loop, 20, 10, 20, 20, 0);
        draw_line(&mut img_loop, 20, 20, 10, 20, 0);
        draw_line(&mut img_loop, 10, 20, 10, 10, 0);

        // 3. Two disjoint strokes
        let mut img_disjoint = make_empty_image(64, 64);
        draw_line(&mut img_disjoint, 5, 5, 15, 5, 0);
        draw_line(&mut img_disjoint, 5, 25, 15, 25, 0);

        let images = vec![img_line, img_loop, img_disjoint];

        for img in images {
            let res = trace_stroke(&img, tolerance);
            let opts = StrokeOptions {
                tolerance,
                ..StrokeOptions::default()
            };
            let res_ex = trace_stroke_ex(&img, &opts);

            assert_eq!(res.path_count, res_ex.paths.len());
            assert_eq!(res.curves.curves.len(), res_ex.paths.len());
            assert_eq!(res.widths.len(), res_ex.paths.len());

            for (i, p) in res_ex.paths.iter().enumerate() {
                assert_eq!(res.widths[i], p.width);
                assert_eq!(res.curves.curves[i].segments.len(), p.curve.segments.len());
                let curve_json = serde_json::to_vec(&res.curves.curves[i]).unwrap();
                let ex_curve_json = serde_json::to_vec(&p.curve).unwrap();
                assert_eq!(curve_json, ex_curve_json);
            }
        }
    }

    #[test]
    fn test_component_widths_measures_stroke_and_blob() {
        let w = 64;
        let h = 64;
        let mut mask = vec![false; (w * h) as usize];

        // 3px-wide bar: x in 5..=7, y in 5..=25
        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
            }
        }

        // 12x12 solid square: x in 30..=41, y in 30..=41
        for y in 30..=41 {
            for x in 30..=41 {
                mask[(y * w + x) as usize] = true;
            }
        }

        let widths = component_widths(&mask, w, h);
        assert_eq!(widths.len(), 2);
        assert_eq!(widths[0].component, 0);
        assert!(
            widths[0].max_full_width < 5.0,
            "Bar max_full_width should be < 5, got {}",
            widths[0].max_full_width
        );
        assert_eq!(widths[1].component, 1);
        assert!(
            widths[1].max_full_width >= 11.0,
            "Square max_full_width should be >= 11, got {}",
            widths[1].max_full_width
        );
    }

    #[test]
    fn test_route_fill_above_diverts_blob() {
        let w = 64;
        let h = 64;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        // 3px-wide bar
        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        // 12x12 solid square
        for y in 30..=41 {
            for x in 30..=41 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(8.0),
            ..StrokeOptions::default()
        };

        let res = trace_stroke_ex(&img, &opts);

        assert_eq!(res.filled_count, 1);
        assert_eq!(res.filled_mask.len(), (w * h) as usize);

        // Check filled_mask is true for square pixels
        assert!(res.filled_mask[(35 * w + 35) as usize]);
        assert!(res.filled_mask[(30 * w + 30) as usize]);
        // Check filled_mask is false for bar pixels and background
        assert!(!res.filled_mask[(15 * w + 6) as usize]);
        assert!(!res.filled_mask[0]);

        // Assert path(s) exist for the bar, but not the square
        assert!(!res.paths.is_empty());
        for p in &res.paths {
            assert!(p.endpoints[0].1 < 30.0);
            assert!(p.endpoints[1].1 < 30.0);
        }
    }

    #[test]
    fn test_route_fill_above_none_is_unchanged() {
        let w = 64;
        let h = 64;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }
        for y in 30..=41 {
            for x in 30..=41 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts_none = StrokeOptions {
            ink: InkSource::Mask(mask.clone()),
            route_fill_above: None,
            ..StrokeOptions::default()
        };

        let opts_max = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(f64::MAX),
            ..StrokeOptions::default()
        };

        let res_none = trace_stroke_ex(&img, &opts_none);
        let res_max = trace_stroke_ex(&img, &opts_max);

        assert!(res_none.filled_mask.is_empty());
        assert_eq!(res_none.filled_count, 0);

        assert!(res_max.filled_mask.is_empty());
        assert_eq!(res_max.filled_count, 0);

        assert_eq!(res_none.paths.len(), res_max.paths.len());
        for (p1, p2) in res_none.paths.iter().zip(res_max.paths.iter()) {
            assert_eq!(p1.width, p2.width);
            assert_eq!(p1.endpoints, p2.endpoints);
        }
    }

    #[test]
    fn test_route_fill_above_all_thin_routes_nothing() {
        let w = 64;
        let h = 64;
        let mut img = make_empty_image(w, h);
        let mut mask = vec![false; (w * h) as usize];

        for y in 5..=25 {
            for x in 5..=7 {
                mask[(y * w + x) as usize] = true;
                draw_pixel(&mut img, x, y);
            }
        }

        let opts = StrokeOptions {
            ink: InkSource::Mask(mask),
            route_fill_above: Some(8.0),
            ..StrokeOptions::default()
        };

        let res = trace_stroke_ex(&img, &opts);

        assert_eq!(res.filled_count, 0);
        assert!(res.filled_mask.is_empty());
        assert!(!res.paths.is_empty());
    }
}
