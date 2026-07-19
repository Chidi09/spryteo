//! Colour quantization (§3.4).  For icon mode:
//!
//! - **Exact histogram** if unique colours ≤ target count.
//! - **k-means++** (fixed seed, CIELAB space) otherwise.
//! - **Palette mode**: nearest-Lab assignment, no clustering.
//!
//! ## Auto colour count (elbow method)
//! Tries k = 2..16, computes within-cluster sum of squared Lab distances,
//! and picks the k where the marginal error reduction first drops below 5%.
//! If no elbow is found, returns 8.  This is deterministic because k-means
//! uses a fixed seed.
//!
//! ## Out of scope
//! The photo-mode "stacked" / "cutout" layering split described in §3.4 is
//! **not implemented** — this quantizer only handles the icon path.  That
//! split belongs in Phase 4.

use spryteo_core::{ClassifiedInput, ColorSpec, Layer, LayerStack, Layering, Rgb};

use crate::color::{self, Lab};

// ── Constants ────────────────────────────────────────────────────────────────

const MAX_AUTO_COLORS: usize = 16;
const MIN_AUTO_COLORS: usize = 2;
const KMEANS_MAX_ITER: usize = 20;
const SEED_AUTO: u64 = 42;

/// Above this many pixels, k-means trains its centroids on an
/// evenly-strided sample of this size instead of every pixel, then does a
/// single full assignment pass at the end. Iterating all pixels every
/// round is O(pixels x k x iters) -- ~240M Lab distances (several
/// seconds, worse in WASM) for a 1-megapixel photo at k=12 -- while the
/// centroids a 65K-pixel stride sample converges to are visually
/// indistinguishable. Small inputs (below the threshold) keep the exact
/// original full-data path, and the stride sample keeps the result
/// deterministic for a given seed.
const KMEANS_SAMPLE_MAX: usize = 65_536;

// ── Public API ───────────────────────────────────────────────────────────────

/// Quantize a classified input into a `LayerStack`.
///
/// - `ColorSpec::Auto`: picks colour count via elbow on within-cluster error.
/// - `ColorSpec::N(n)`: uses exactly `n` colours.
/// - `ColorSpec::Palette(palette)`: assigns each pixel to the nearest palette
///   colour in Lab space, no clustering.
pub fn quantize(
    input: &ClassifiedInput,
    colors: &ColorSpec,
    layering: &Layering,
    seed: u64,
) -> LayerStack {
    let pixels = &input.image.pixels;
    let width = input.image.width;
    let height = input.image.height;
    let total = (width * height) as usize;

    // Collect non-fully-transparent pixel info
    let mut pixel_data: Vec<PixelInfo> = Vec::new();
    for i in 0..total {
        let a = pixels[i * 4 + 3];
        if a > 0 {
            let idx = i * 4;
            let rgb = Rgb {
                r: pixels[idx],
                g: pixels[idx + 1],
                b: pixels[idx + 2],
            };
            let lab = color::srgb_to_lab(&rgb);
            pixel_data.push(PixelInfo {
                pixel_index: i,
                rgb,
                lab,
            });
        }
    }

    if pixel_data.is_empty() {
        return LayerStack { layers: vec![] };
    }

    // Determine target count
    let target = match colors {
        ColorSpec::Palette(pal) => {
            let pal_lab: Vec<Lab> = pal.iter().map(color::srgb_to_lab).collect();
            let mut stack = quantize_to_palette(pixels, width, height, &pal_lab, pal);
            if let Layering::Stacked = layering {
                apply_stacked_layering(&mut stack);
            }
            return stack;
        }
        ColorSpec::N(n) => *n as usize,
        ColorSpec::Auto => auto_color_count(&pixel_data),
    };

    if target == 0 {
        return LayerStack { layers: vec![] };
    }

    // Check exact-histogram shortcut
    let mut stack = if let Some(unique) = unique_rgb_up_to(&pixel_data, target) {
        build_exact_layers(pixels, width, height, &unique, target)
    } else {
        // k-means clustering
        let assignments = kmeans_pp(&pixel_data, target, seed);
        build_kmeans_layers(pixels, width, height, &pixel_data, &assignments, target)
    };

    if let Layering::Stacked = layering {
        apply_stacked_layering(&mut stack);
    }
    stack
}

/// Post-processing step to apply stacked layering per §3.4.
/// For each layer in z_order from bottom (0) to top, sets the layer's mask
/// to the union of its own pixels and all pixels of layers above it.
fn apply_stacked_layering(stack: &mut LayerStack) {
    let n = stack.layers.len();
    if n <= 1 {
        return;
    }
    for i in (0..n - 1).rev() {
        let (left, right) = stack.layers.split_at_mut(i + 1);
        let current_layer = &mut left[i];
        let next_layer = &right[0];
        for (c, &n_val) in current_layer.mask.iter_mut().zip(next_layer.mask.iter()) {
            if n_val > *c {
                *c = n_val;
            }
        }
    }
}

// ── Data structures ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct PixelInfo {
    pixel_index: usize,
    rgb: Rgb,
    lab: Lab,
}

/// A simple deterministic PRNG (xorshift64*) for k-means++ initialisation.
struct SeededRng {
    state: u64,
}

impl SeededRng {
    fn new(seed: u64) -> Self {
        let mut rng = Self {
            state: if seed == 0 { 1 } else { seed },
        };
        rng.next_u64();
        rng
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    #[allow(dead_code)]
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
}

// ── Auto colour count (elbow method) ─────────────────────────────────────────

/// Pick the colour count automatically using the elbow heuristic.
///
/// Tries k = MIN_AUTO_COLORS ..= MAX_AUTO_COLORS.  For each k, computes the
/// within-cluster sum of squared Lab distances (WSSE) via k-means (fixed
/// seed).  Returns the first k where the marginal reduction
/// `(WSSE_{k-1} - WSSE_k) / WSSE_{k-1}` drops below 0.05.
/// If no elbow is found, returns 8.
fn auto_color_count(data: &[PixelInfo]) -> usize {
    // The elbow scan only *estimates a colour count* -- it runs k-means
    // up to 15 times (k = 2..=16), so it pays the clustering cost many
    // times over. A small evenly-strided sample estimates the same k
    // (the WSSE curve's shape is what matters, not its absolute values)
    // at a fraction of the cost; deterministic because the stride is.
    const ELBOW_SAMPLE_MAX: usize = 16_384;
    if data.len() > ELBOW_SAMPLE_MAX {
        let stride = data.len().div_ceil(ELBOW_SAMPLE_MAX);
        let sample: Vec<PixelInfo> = data.iter().step_by(stride).cloned().collect();
        return auto_color_count(&sample);
    }

    let n = data.len();
    if n <= MIN_AUTO_COLORS {
        return n;
    }
    let k_max = MAX_AUTO_COLORS.min(n);

    let mut prev_error = f64::MAX;

    for k in MIN_AUTO_COLORS..=k_max {
        let assignments = kmeans_pp(data, k, SEED_AUTO);
        let error = within_cluster_error(data, &assignments, k);

        if k > MIN_AUTO_COLORS && prev_error.is_finite() && error.is_finite() {
            let reduction = (prev_error - error) / prev_error;
            if reduction < 0.05 {
                return k - 1;
            }
        }

        prev_error = error;
    }

    8
}

/// Compute total within-cluster sum of squared Lab distances.
fn within_cluster_error(data: &[PixelInfo], assignments: &[usize], k: usize) -> f64 {
    let mut centroids = vec![
        Lab {
            l: 0.0,
            a: 0.0,
            b: 0.0
        };
        k
    ];
    let mut counts = vec![0usize; k];

    for (pi, &cluster) in assignments.iter().enumerate() {
        centroids[cluster].l += data[pi].lab.l;
        centroids[cluster].a += data[pi].lab.a;
        centroids[cluster].b += data[pi].lab.b;
        counts[cluster] += 1;
    }

    for c in 0..k {
        if counts[c] > 0 {
            let n = counts[c] as f64;
            centroids[c].l /= n;
            centroids[c].a /= n;
            centroids[c].b /= n;
        }
    }

    let mut total = 0.0;
    for (pi, &cluster) in assignments.iter().enumerate() {
        total += color::lab_distance_sq(&data[pi].lab, &centroids[cluster]);
    }
    total
}

// ── k-means++ clustering ─────────────────────────────────────────────────────

/// Run k-means with k-means++ initialisation and the given fixed seed.
///
/// Returns a `Vec<usize>` mapping each entry in `data` to a cluster index
/// in `0..k`.  The result is fully deterministic for a given seed.
#[allow(clippy::needless_range_loop)] // indexes small (k-length) centroid vectors alongside a running best
fn kmeans_pp(data: &[PixelInfo], k: usize, seed: u64) -> Vec<usize> {
    let n = data.len();
    if n == 0 {
        return vec![];
    }
    let k = k.min(n);

    // Large inputs: train on a deterministic stride sample, then assign
    // every point to the nearest trained centroid in one pass.
    if n > KMEANS_SAMPLE_MAX {
        let stride = n.div_ceil(KMEANS_SAMPLE_MAX);
        let sample: Vec<PixelInfo> = data.iter().step_by(stride).cloned().collect();
        let sample_assignments = kmeans_pp(&sample, k, seed);

        // Recover centroids from the sample assignments
        let mut centroids = vec![
            Lab {
                l: 0.0,
                a: 0.0,
                b: 0.0
            };
            k
        ];
        let mut counts = vec![0usize; k];
        for (i, &cluster) in sample_assignments.iter().enumerate() {
            centroids[cluster].l += sample[i].lab.l;
            centroids[cluster].a += sample[i].lab.a;
            centroids[cluster].b += sample[i].lab.b;
            counts[cluster] += 1;
        }
        for c in 0..k {
            if counts[c] > 0 {
                let n_c = counts[c] as f64;
                centroids[c].l /= n_c;
                centroids[c].a /= n_c;
                centroids[c].b /= n_c;
            }
        }

        // Full assignment pass over all points
        let mut assignments = vec![0usize; n];
        for (i, pi) in data.iter().enumerate() {
            let mut best = 0usize;
            let mut best_dist = color::lab_distance_sq(&pi.lab, &centroids[0]);
            for c in 1..k {
                let d = color::lab_distance_sq(&pi.lab, &centroids[c]);
                if d < best_dist {
                    best_dist = d;
                    best = c;
                }
            }
            assignments[i] = best;
        }
        return assignments;
    }

    let mut rng = SeededRng::new(seed);

    // ── Step 1: k-means++ initialisation ──
    let mut centroids: Vec<Lab> = Vec::with_capacity(k);

    // Pick first centroid uniformly at random using the seeded RNG
    let first_idx = (rng.next_u64() as usize) % n;
    centroids.push(data[first_idx].lab);

    let mut min_dists = vec![f64::MAX; n];
    for _ in 1..k {
        // Compute squared distance to nearest existing centroid
        let mut total_weight = 0.0;
        for (i, pi) in data.iter().enumerate() {
            let d = color::lab_distance_sq(&pi.lab, centroids.last().unwrap());
            if d < min_dists[i] {
                min_dists[i] = d;
            }
            total_weight += min_dists[i];
        }

        // Pick next centroid with probability ∝ distance²
        let threshold = rng.next_f64() * total_weight;
        let mut cumulative = 0.0;
        let mut pick = n - 1;
        for (i, &d) in min_dists.iter().enumerate() {
            cumulative += d;
            if cumulative >= threshold {
                pick = i;
                break;
            }
        }
        centroids.push(data[pick].lab);
    }

    // ── Step 2: iterative refinement ──
    let mut assignments = vec![0usize; n];
    let mut new_centroids = centroids.clone();
    let mut counts = vec![0usize; k];

    for iteration in 0..KMEANS_MAX_ITER {
        // Assign each point to nearest centroid
        let mut changed = false;
        for (i, pi) in data.iter().enumerate() {
            let mut best = 0usize;
            let mut best_dist = color::lab_distance_sq(&pi.lab, &centroids[0]);
            for c in 1..k {
                let d = color::lab_distance_sq(&pi.lab, &centroids[c]);
                if d < best_dist {
                    best_dist = d;
                    best = c;
                }
            }
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }

        if !changed && iteration > 0 {
            break;
        }

        // Recompute centroids
        for c in 0..k {
            new_centroids[c] = Lab {
                l: 0.0,
                a: 0.0,
                b: 0.0,
            };
            counts[c] = 0;
        }
        for (i, &cluster) in assignments.iter().enumerate() {
            new_centroids[cluster].l += data[i].lab.l;
            new_centroids[cluster].a += data[i].lab.a;
            new_centroids[cluster].b += data[i].lab.b;
            counts[cluster] += 1;
        }
        for c in 0..k {
            if counts[c] > 0 {
                let n_c = counts[c] as f64;
                new_centroids[c].l /= n_c;
                new_centroids[c].a /= n_c;
                new_centroids[c].b /= n_c;
            } else {
                new_centroids[c] = centroids[c];
            }
        }

        std::mem::swap(&mut centroids, &mut new_centroids);
    }

    assignments
}

// ── LayerStack builders ──────────────────────────────────────────────────────

/// Build a `LayerStack` from an exact colour histogram.
///
/// Each unique colour becomes one layer.  If `unique.len() < target`, the
/// colour count is used as-is.
#[allow(clippy::needless_range_loop)] // strided flat-pixel-buffer indexing (idx = i * 4) and per-cluster mask lookup
fn build_exact_layers(
    pixels: &[u8],
    width: u32,
    height: u32,
    unique: &[Rgb],
    _target: usize,
) -> LayerStack {
    let total = (width * height) as usize;
    let k = unique.len();
    let mut masks = vec![vec![0u8; total]; k];

    for i in 0..total {
        let idx = i * 4;
        let c = Rgb {
            r: pixels[idx],
            g: pixels[idx + 1],
            b: pixels[idx + 2],
        };
        // Exclude fully transparent pixels from all masks
        let a = pixels[idx + 3];
        if a == 0 {
            continue;
        }
        // Find which unique colour this pixel matches
        if let Some(ci) = unique.iter().position(|u| *u == c) {
            masks[ci][i] = 255;
        }
    }

    // Sort by first-appearance scan order
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&ci| {
        let mut first = total;
        for i in 0..total {
            if masks[ci][i] != 0 {
                first = i;
                break;
            }
        }
        first
    });

    let layers: Vec<Layer> = order
        .into_iter()
        .enumerate()
        .map(|(z, ci)| Layer {
            mask: std::mem::take(&mut masks[ci]),
            color: unique[ci],
            z_order: z,
        })
        .collect();

    LayerStack { layers }
}

/// Build a `LayerStack` from k-means cluster assignments.
///
/// Each cluster becomes one layer.  The cluster colour is the k-means
/// centroid (mean Lab → sRGB).
#[allow(clippy::needless_range_loop)] // indexes small (k-length) colour/mask vectors
fn build_kmeans_layers(
    _pixels: &[u8],
    width: u32,
    height: u32,
    data: &[PixelInfo],
    assignments: &[usize],
    k: usize,
) -> LayerStack {
    let total = (width * height) as usize;
    let mut masks = vec![vec![0u8; total]; k];
    let mut lab_sums = vec![(0.0_f64, 0.0_f64, 0.0_f64); k];
    let mut counts = vec![0usize; k];

    for (pi, &cluster) in assignments.iter().enumerate() {
        let i = data[pi].pixel_index;
        masks[cluster][i] = 255;
        lab_sums[cluster].0 += data[pi].lab.l;
        lab_sums[cluster].1 += data[pi].lab.a;
        lab_sums[cluster].2 += data[pi].lab.b;
        counts[cluster] += 1;
    }

    // Also need to mark pixels that were excluded (alpha = 0)
    // They already have mask = 0 for all clusters, which is correct.

    // Compute cluster colours (mean Lab → sRGB)
    let mut colors: Vec<Rgb> = Vec::with_capacity(k);
    for c in 0..k {
        if counts[c] > 0 {
            let n = counts[c] as f64;
            let lab = Lab {
                l: lab_sums[c].0 / n,
                a: lab_sums[c].1 / n,
                b: lab_sums[c].2 / n,
            };
            colors.push(color::lab_to_srgb(&lab));
        } else {
            colors.push(Rgb { r: 0, g: 0, b: 0 });
        }
    }

    // Sort by first-appearance scan order
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&ci| {
        let mut first = total;
        for i in 0..total {
            if masks[ci][i] != 0 {
                first = i;
                break;
            }
        }
        first
    });

    let layers: Vec<Layer> = order
        .into_iter()
        .enumerate()
        .map(|(z, ci)| Layer {
            mask: std::mem::take(&mut masks[ci]),
            color: colors[ci],
            z_order: z,
        })
        .collect();

    LayerStack { layers }
}

/// Build a `LayerStack` by assigning each pixel to the nearest palette
/// colour in CIELAB space.  No clustering is performed.
#[allow(clippy::needless_range_loop)] // strided flat-pixel-buffer indexing (idx = i * 4) and per-cluster mask lookup
fn quantize_to_palette(
    pixels: &[u8],
    width: u32,
    height: u32,
    palette_lab: &[Lab],
    palette_rgb: &[Rgb],
) -> LayerStack {
    let total = (width * height) as usize;
    let k = palette_lab.len();
    let mut masks = vec![vec![0u8; total]; k];

    for i in 0..total {
        let a = pixels[i * 4 + 3];
        if a == 0 {
            continue;
        }
        let idx = i * 4;
        let rgb = Rgb {
            r: pixels[idx],
            g: pixels[idx + 1],
            b: pixels[idx + 2],
        };
        let lab = color::srgb_to_lab(&rgb);

        let mut best = 0usize;
        let mut best_dist = color::lab_distance_sq(&lab, &palette_lab[0]);
        for c in 1..k {
            let d = color::lab_distance_sq(&lab, &palette_lab[c]);
            if d < best_dist {
                best_dist = d;
                best = c;
            }
        }
        masks[best][i] = 255;
    }

    // Sort by first-appearance scan order
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_by_key(|&ci| {
        let mut first = total;
        for i in 0..total {
            if masks[ci][i] != 0 {
                first = i;
                break;
            }
        }
        first
    });

    let layers: Vec<Layer> = order
        .into_iter()
        .enumerate()
        .map(|(z, ci)| Layer {
            mask: std::mem::take(&mut masks[ci]),
            color: palette_rgb[ci],
            z_order: z,
        })
        .collect();

    LayerStack { layers }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Collect unique `Rgb` values from pixel data in scan order, bailing out
/// with `None` as soon as more than `cap` unique colours are seen.
///
/// The result is only ever needed when the image qualifies for the
/// exact-histogram shortcut (unique colours <= target), so there is no
/// reason to keep scanning a photo with hundreds of thousands of unique
/// colours: the previous implementation did a linear `Vec::contains` per
/// pixel, which is O(pixels x unique) -- tens of billions of comparisons
/// (~20s+) on a 1-megapixel gradient photo. The set membership test uses
/// a hash set; scan order (and therefore layer order) is preserved by the
/// separate `unique` vec, so behaviour is identical for images that
/// qualify.
fn unique_rgb_up_to(data: &[PixelInfo], cap: usize) -> Option<Vec<Rgb>> {
    let mut seen: std::collections::HashSet<(u8, u8, u8)> =
        std::collections::HashSet::with_capacity(cap + 1);
    let mut unique: Vec<Rgb> = Vec::new();
    for pi in data {
        if seen.insert((pi.rgb.r, pi.rgb.g, pi.rgb.b)) {
            if unique.len() >= cap {
                return None;
            }
            unique.push(pi.rgb);
        }
    }
    Some(unique)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spryteo_core::{ClassifiedInput, Mode, RasterImage};

    fn make_image(width: u32, height: u32, pixels: Vec<u8>) -> RasterImage {
        RasterImage {
            width,
            height,
            pixels,
        }
    }

    fn rgb_pixel(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        [r, g, b, a]
    }

    fn classified_input(img: RasterImage, mode: Mode) -> ClassifiedInput {
        ClassifiedInput {
            image: img,
            mode,
            background_color: None,
        }
    }

    // ── Auto → 2 layers for a 2-colour image ─────────────────────────────

    #[test]
    fn auto_quantize_two_colour() {
        // 4×4 checkerboard of red and white
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 0 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);

        assert_eq!(
            result.layers.len(),
            2,
            "expected 2 layers for a 2-colour image, got {}",
            result.layers.len()
        );
    }

    // ── N(1) collapses to 1 layer ─────────────────────────────────────────

    #[test]
    fn n1_collapses_to_one_layer() {
        // Multi-colour image → forced to 1 colour
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let r = (x * 64) as u8;
                let g = (y * 64) as u8;
                let b = 128u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::N(1), &Layering::Cutout, 42);

        assert_eq!(
            result.layers.len(),
            1,
            "expected 1 layer with N(1), got {}",
            result.layers.len()
        );
    }

    // ── Determinism ───────────────────────────────────────────────────────

    #[test]
    fn quantize_is_deterministic() {
        // Multi-colour image large enough to trigger k-means
        let w = 8u32;
        let h = 8u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 32) as u8).wrapping_add(10);
                let g = ((y * 32) as u8).wrapping_add(20);
                let b = ((x + y) * 16) as u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Icon);
        let seed = 12345u64;

        let r1 = quantize(&input, &ColorSpec::N(4), &Layering::Cutout, seed);
        let r2 = quantize(&input, &ColorSpec::N(4), &Layering::Cutout, seed);

        let json1 = serde_json::to_vec(&r1).unwrap();
        let json2 = serde_json::to_vec(&r2).unwrap();

        assert_eq!(
            json1, json2,
            "quantize produced different results for the same seed"
        );
    }

    // ── Palette mode ──────────────────────────────────────────────────────

    #[test]
    fn palette_assigns_to_nearest() {
        // 2-colour image (red, white) → palette of [blue, green]
        // All pixels should assign to nearest between blue and green
        let mut pixels = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                if (x + y) % 2 == 0 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(255, 255, 255, 255));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let palette = vec![Rgb { r: 0, g: 0, b: 255 }, Rgb { r: 0, g: 255, b: 0 }];
        let result = quantize(&input, &ColorSpec::Palette(palette), &Layering::Cutout, 42);

        assert_eq!(result.layers.len(), 2);
        // Both palette colours should be present
        let colors: Vec<Rgb> = result.layers.iter().map(|l| l.color).collect();
        assert!(colors.contains(&Rgb { r: 0, g: 0, b: 255 }));
        assert!(colors.contains(&Rgb { r: 0, g: 255, b: 0 }));
    }

    // ── Transparent pixels excluded from clustering ───────────────────────

    #[test]
    fn transparent_pixels_excluded() {
        // 4×4 image: 8 opaque red pixels, 8 fully transparent pixels
        let mut pixels = Vec::new();
        for _y in 0..4 {
            for x in 0..4 {
                if x < 2 {
                    pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255));
                } else {
                    pixels.extend_from_slice(&rgb_pixel(0, 0, 0, 0));
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);

        // Only red pixels are clustered → 1 layer (1 unique colour)
        assert_eq!(result.layers.len(), 1);
        // Transparent pixels should have mask = 0
        assert_eq!(result.layers[0].mask[0], 255); // pixel (0,0) red
        assert_eq!(result.layers[0].mask[2], 0); // pixel (2,0) transparent
    }

    // ── Empty image ───────────────────────────────────────────────────────

    #[test]
    fn empty_image_returns_empty() {
        let img = make_image(1, 1, vec![0, 0, 0, 0]); // fully transparent
        let input = classified_input(img, Mode::Icon);
        let result = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);
        assert_eq!(result.layers.len(), 0);
    }

    #[test]
    fn stacked_layering_covers_more_pixels_and_respects_transparency() {
        let mut pixels = Vec::new();
        // 4x4 image:
        // Row 0-1: 2 red, 2 blue
        // Row 2-3: 2 red, 2 transparent
        for y in 0..4 {
            for x in 0..4 {
                if y < 2 {
                    if x >= 2 {
                        pixels.extend_from_slice(&rgb_pixel(0, 0, 255, 255)); // blue
                    } else {
                        pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255)); // red
                    }
                } else {
                    if x >= 2 {
                        pixels.extend_from_slice(&rgb_pixel(0, 0, 0, 0)); // transparent
                    } else {
                        pixels.extend_from_slice(&rgb_pixel(255, 0, 0, 255)); // red
                    }
                }
            }
        }
        let img = make_image(4, 4, pixels);
        let input = classified_input(img, Mode::Icon);

        // Run in Cutout mode
        let cutout_res = quantize(&input, &ColorSpec::Auto, &Layering::Cutout, 42);
        assert_eq!(cutout_res.layers.len(), 2);
        let red_cutout = cutout_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 255, g: 0, b: 0 })
            .unwrap();
        let blue_cutout = cutout_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 0, g: 0, b: 255 })
            .unwrap();

        // Run in Stacked mode
        let stacked_res = quantize(&input, &ColorSpec::Auto, &Layering::Stacked, 42);
        assert_eq!(stacked_res.layers.len(), 2);
        let red_stacked = stacked_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 255, g: 0, b: 0 })
            .unwrap();
        let blue_stacked = stacked_res
            .layers
            .iter()
            .find(|l| l.color == Rgb { r: 0, g: 0, b: 255 })
            .unwrap();

        // In cutout mode:
        // Red mask should cover 8 pixels (the red ones).
        // Blue mask should cover 4 pixels (the blue ones).
        let red_cutout_count = red_cutout.mask.iter().filter(|&&v| v > 0).count();
        let blue_cutout_count = blue_cutout.mask.iter().filter(|&&v| v > 0).count();
        assert_eq!(red_cutout_count, 8);
        assert_eq!(blue_cutout_count, 4);

        // In stacked mode, Red has z_order = 0 and Blue has z_order = 1.
        // Red stacked mask should be union of Red disjoint and Blue disjoint -> 12 pixels.
        // Blue stacked mask is topmost -> 4 pixels.
        let red_stacked_count = red_stacked.mask.iter().filter(|&&v| v > 0).count();
        let blue_stacked_count = blue_stacked.mask.iter().filter(|&&v| v > 0).count();

        assert_eq!(red_stacked_count, 12);
        assert_eq!(blue_stacked_count, 4);

        // Assert the actual pixel-count difference for the bottom layer is exactly 4
        assert_eq!(red_stacked_count - red_cutout_count, 4);

        // Confirm that transparent pixels are NEVER included in any layer's mask in stacked mode either.
        let transparent_indices = vec![10, 11, 14, 15];
        for idx in transparent_indices {
            assert_eq!(red_stacked.mask[idx], 0);
            assert_eq!(blue_stacked.mask[idx], 0);
        }
    }

    #[test]
    fn stacked_quantize_is_deterministic() {
        let w = 8u32;
        let h = 8u32;
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let r = ((x * 32) as u8).wrapping_add(10);
                let g = ((y * 32) as u8).wrapping_add(20);
                let b = ((x + y) * 16) as u8;
                pixels.extend_from_slice(&rgb_pixel(r, g, b, 255));
            }
        }
        let img = make_image(w, h, pixels);
        let input = classified_input(img, Mode::Photo);
        let seed = 98765u64;

        let r1 = quantize(&input, &ColorSpec::N(4), &Layering::Stacked, seed);
        let r2 = quantize(&input, &ColorSpec::N(4), &Layering::Stacked, seed);

        let json1 = serde_json::to_vec(&r1).unwrap();
        let json2 = serde_json::to_vec(&r2).unwrap();

        assert_eq!(json1, json2);
    }
}
