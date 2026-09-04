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

/// Max allowed `pixels / (pi * maxDT^2)` for a component to be routed to fill.
/// A solid disc scores ~1.0, a square 1.27, a triangle 1.65; stroke outlines
/// (whose area grows with length, not width) measure >= 7.9 on the reference
/// sheet. 2.5 sits inside that measured gap.
pub const ROUTE_FILL_COMPACTNESS_MAX: f64 = 2.5;

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
    /// Components whose max inscribed full width (2 x max DT) is >= this value AND
    /// whose area is compact relative to that width (see
    /// `ROUTE_FILL_COMPACTNESS_MAX`) are routed to `filled_mask` instead of being
    /// skeleton-traced. None disables routing.
    pub route_fill_above: Option<f64>,
    /// Merge junction nodes sitting closer together than the local stroke width into their centroid
    /// before chain building.
    pub repair_junctions: bool,
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
            repair_junctions: false,
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

                    // Width alone cannot classify at supersampled resolutions: a
                    // junction blob on a long stroke outline reaches the same max
                    // inscribed width as a small solid glyph (measured on the
                    // reference sheet at 4x: stroke junctions 22-27px vs glyphs
                    // 26-50px — the ranges interleave). Compactness separates them:
                    // pixels / (pi * maxDT^2) is ~1.0 for a disc, 1.29 for a
                    // square, 1.65 for a triangle, but >= 7.9 for every stroke
                    // outline. The 2.5 cutoff sits in that 1.65..7.9 gap.
                    let compact = comp_pixels.len() as f64
                        <= ROUTE_FILL_COMPACTNESS_MAX * std::f64::consts::PI * max_dist * max_dist;
                    if 2.0 * max_dist >= t && compact {
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

    if opts.repair_junctions {
        graphs = graphs
            .into_iter()
            .map(|g| graph::merge_close_junctions(g, &dist, image.width))
            .collect();
    }

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
#[path = "lib_tests.rs"]
mod tests;
