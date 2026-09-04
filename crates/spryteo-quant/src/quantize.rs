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

use spryteo_core::{
    CancelToken, ClassifiedInput, ColorSpec, Layer, LayerStack, Layering, Mode, Rgb, SpryteoError,
};

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
    quantize_cancellable(input, colors, layering, seed, &CancelToken::none())
        .expect("quantize with an inert CancelToken cannot be cancelled")
}

/// [`quantize`] with cooperative cancellation (ROADMAP §3.1).
///
/// The token is polled inside the per-pixel Lab conversion, the k-means
/// assignment loop, the blend-dissolve passes, and palette assignment --
/// the four places that dominate quantization time on a megapixel input.
pub fn quantize_cancellable(
    input: &ClassifiedInput,
    colors: &ColorSpec,
    layering: &Layering,
    seed: u64,
    cancel: &CancelToken,
) -> Result<LayerStack, SpryteoError> {
    let pixels = &input.image.pixels;
    let width = input.image.width;
    let height = input.image.height;
    let total = (width * height) as usize;

    // Collect non-fully-transparent pixel info
    let mut pixel_data: Vec<PixelInfo> = Vec::new();
    for i in 0..total {
        cancel.check_at(i)?;
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
        return Ok(LayerStack { layers: vec![] });
    }

    // Line-art (outline mode): binarize into exactly ink + paper. Thin
    // anti-aliased strokes are majority-paper in every pixel, so color-space
    // clustering (and blend dissolution) erodes them; a luminance threshold
    // is the faithful model for ink drawings. Palette overrides still win,
    // and images that fail the duotone validation (a colourful drawing the
    // classifier mistook for line-art) fall through to k-means below.
    if matches!(input.mode, Mode::LineArt) && !matches!(colors, ColorSpec::Palette(_)) {
        if let Some(mut stack) = binarize_line_art(width, height, &pixel_data) {
            if let Layering::Stacked = layering {
                apply_stacked_layering(&mut stack);
            }
            return Ok(stack);
        }
    }

    // Determine target count
    let target = match colors {
        ColorSpec::Palette(pal) => {
            let pal_lab: Vec<Lab> = pal.iter().map(color::srgb_to_lab).collect();
            let mut stack = quantize_to_palette(pixels, width, height, &pal_lab, pal, cancel)?;
            if let Layering::Stacked = layering {
                apply_stacked_layering(&mut stack);
            }
            return Ok(stack);
        }
        ColorSpec::N(n) => *n as usize,
        ColorSpec::Auto => auto_color_count(&pixel_data, cancel)?,
    };

    if target == 0 {
        return Ok(LayerStack { layers: vec![] });
    }

    let is_flat = is_flat_art(&pixel_data);

    // Check exact-histogram shortcut
    let mut clusters = if let Some(unique) = unique_rgb_up_to(&pixel_data, target) {
        get_exact_clusters(pixels, width, height, &unique)
    } else {
        // k-means clustering
        let assignments = kmeans_pp(&pixel_data, target, seed, cancel)?;
        get_kmeans_clusters(width, height, &pixel_data, &assignments, target)
    };

    dissolve_blend_clusters(&mut clusters, &pixel_data, total, is_flat, cancel)?;

    let mut surviving_clusters: Vec<Cluster> = clusters.into_iter().filter(|c| c.active).collect();

    // Sort by first-appearance scan order
    cancel.check()?;
    let mut order: Vec<usize> = (0..surviving_clusters.len()).collect();
    order.sort_by_key(|&idx| {
        let mut first = total;
        for i in 0..total {
            if surviving_clusters[idx].mask[i] != 0 {
                first = i;
                break;
            }
        }
        first
    });

    let layers: Vec<Layer> = order
        .into_iter()
        .enumerate()
        .map(|(z, idx)| Layer {
            mask: std::mem::take(&mut surviving_clusters[idx].mask),
            color: surviving_clusters[idx].color,
            z_order: z,
        })
        .collect();

    let mut stack = LayerStack { layers };

    if let Layering::Stacked = layering {
        apply_stacked_layering(&mut stack);
    }
    Ok(stack)
}

/// Luminance half-window over which ink/paper coverage ramps linearly around
/// the Otsu threshold, so marching squares (iso 127.5) lands stroke edges at
/// subpixel positions instead of the pixel grid. Wider blurs edge placement;
/// narrower reverts toward hard staircase boundaries.
const LINE_ART_SOFT_WINDOW: f64 = 24.0;

/// Sauvola window radius (window = 2r+1 px) and parameters k / R. The
/// standard document-binarization values: k = 0.2 biases the threshold
/// ~20% below the local mean in flat regions (so clean paper never trips),
/// R = 128 normalizes the local standard deviation.
const SAUVOLA_RADIUS: usize = 15;
const SAUVOLA_K: f64 = 0.2;
const SAUVOLA_R: f64 = 128.0;

/// Per-pixel Sauvola thresholds via integral images (O(n)):
/// t(x,y) = mean * (1 + k * (std / R - 1)).
fn sauvola_thresholds(lumas: &[f64], width: usize, height: usize) -> Vec<f64> {
    let w1 = width + 1;
    let h1 = height + 1;
    let mut integral = vec![0.0f64; w1 * h1];
    let mut integral_sq = vec![0.0f64; w1 * h1];
    for y in 0..height {
        let mut row = 0.0;
        let mut row_sq = 0.0;
        for x in 0..width {
            let v = lumas[y * width + x];
            row += v;
            row_sq += v * v;
            integral[(y + 1) * w1 + (x + 1)] = integral[y * w1 + (x + 1)] + row;
            integral_sq[(y + 1) * w1 + (x + 1)] = integral_sq[y * w1 + (x + 1)] + row_sq;
        }
    }

    let mut thresholds = vec![0.0f64; width * height];
    for y in 0..height {
        let y0 = y.saturating_sub(SAUVOLA_RADIUS);
        let y1 = (y + SAUVOLA_RADIUS + 1).min(height);
        for x in 0..width {
            let x0 = x.saturating_sub(SAUVOLA_RADIUS);
            let x1 = (x + SAUVOLA_RADIUS + 1).min(width);
            let count = ((x1 - x0) * (y1 - y0)) as f64;
            let sum = integral[y1 * w1 + x1] - integral[y0 * w1 + x1] - integral[y1 * w1 + x0]
                + integral[y0 * w1 + x0];
            let sum_sq =
                integral_sq[y1 * w1 + x1] - integral_sq[y0 * w1 + x1] - integral_sq[y1 * w1 + x0]
                    + integral_sq[y0 * w1 + x0];
            let mean = sum / count;
            let var = (sum_sq / count - mean * mean).max(0.0);
            thresholds[y * width + x] = mean * (1.0 + SAUVOLA_K * (var.sqrt() / SAUVOLA_R - 1.0));
        }
    }
    thresholds
}

/// Fraction of pixels allowed to sit far off the ink↔paper axis in Lab
/// before an image is judged not-actually-duotone and binarization is
/// abandoned (falling back to the k-means path). Genuine line art —
/// including anti-aliased strokes, whose blend pixels lie ON the axis —
/// measures 0.0 here; multi-colour illustrations that merely have a low
/// ink ratio (the classifier's only line-art heuristic) measure 10%+.
const DUOTONE_MAX_OFF_AXIS_FRACTION: f64 = 0.02;
/// Lab distance from the ink↔paper segment beyond which a pixel counts
/// as off-axis for the duotone check.
const DUOTONE_OFF_AXIS_DIST: f64 = 15.0;

/// Binarize a line-art image into exactly two layers: paper (bottom) and
/// ink (top). A pixel is ink when it is dark by EITHER the global Otsu
/// threshold (solid regions) or the Sauvola local threshold (faint thin
/// strokes), both over Rec.601 luminance.
///
/// Returns `None` when the image is not genuinely two-tone (see
/// [`DUOTONE_MAX_OFF_AXIS_FRACTION`]) so the caller can fall back to
/// full colour quantization instead of destroying a colourful image.
fn binarize_line_art(width: u32, height: u32, pixel_data: &[PixelInfo]) -> Option<LayerStack> {
    let total = (width * height) as usize;

    let luma_of = |rgb: &Rgb| 0.299 * rgb.r as f64 + 0.587 * rgb.g as f64 + 0.114 * rgb.b as f64;

    let mut hist = [0u64; 256];
    for p in pixel_data {
        hist[luma_of(&p.rgb) as usize] += 1;
    }

    // Otsu: maximize between-class variance; deterministic tie-break on the
    // lowest threshold.
    let n = pixel_data.len() as f64;
    let total_sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(v, &c)| v as f64 * c as f64)
        .sum();
    let mut best_t = 127usize;
    let mut best_var = -1.0f64;
    let mut w0 = 0.0f64;
    let mut sum0 = 0.0f64;
    for (t, &count) in hist.iter().enumerate() {
        w0 += count as f64;
        sum0 += t as f64 * count as f64;
        let w1 = n - w0;
        if w0 == 0.0 || w1 == 0.0 {
            continue;
        }
        let mu0 = sum0 / w0;
        let mu1 = (total_sum - sum0) / w1;
        let var = w0 * w1 * (mu0 - mu1) * (mu0 - mu1);
        if var > best_var {
            best_var = var;
            best_t = t;
        }
    }
    let threshold = best_t as f64;

    // Duotone validation: binarization is only faithful when every pixel
    // colour lies near the Lab segment between the ink mean and the paper
    // mean (anti-aliased blends lie on it by construction). A colourful
    // illustration misclassified as line-art has whole regions far off
    // that axis — bail out so the k-means path handles it.
    {
        let mut dark_sum = [0u64; 3];
        let mut dark_count = 0u64;
        let mut light_sum = [0u64; 3];
        let mut light_count = 0u64;
        for p in pixel_data {
            let (sum, count) = if luma_of(&p.rgb) <= threshold {
                (&mut dark_sum, &mut dark_count)
            } else {
                (&mut light_sum, &mut light_count)
            };
            sum[0] += p.rgb.r as u64;
            sum[1] += p.rgb.g as u64;
            sum[2] += p.rgb.b as u64;
            *count += 1;
        }
        if dark_count == 0 || light_count == 0 {
            return None;
        }
        let mean_rgb = |sum: &[u64; 3], count: u64| Rgb {
            r: (sum[0] as f64 / count as f64).round() as u8,
            g: (sum[1] as f64 / count as f64).round() as u8,
            b: (sum[2] as f64 / count as f64).round() as u8,
        };
        let axis_a = color::srgb_to_lab(&mean_rgb(&dark_sum, dark_count));
        let axis_b = color::srgb_to_lab(&mean_rgb(&light_sum, light_count));
        let ab = (
            axis_b.l - axis_a.l,
            axis_b.a - axis_a.a,
            axis_b.b - axis_a.b,
        );
        let ab_len_sq = ab.0 * ab.0 + ab.1 * ab.1 + ab.2 * ab.2;
        let mut off_axis = 0u64;
        for p in pixel_data {
            let lab = color::srgb_to_lab(&p.rgb);
            let ap = (lab.l - axis_a.l, lab.a - axis_a.a, lab.b - axis_a.b);
            let t = if ab_len_sq > 1e-12 {
                ((ap.0 * ab.0 + ap.1 * ab.1 + ap.2 * ab.2) / ab_len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let proj = Lab {
                l: axis_a.l + t * ab.0,
                a: axis_a.a + t * ab.1,
                b: axis_a.b + t * ab.2,
            };
            if color::lab_distance_sq(&lab, &proj) > DUOTONE_OFF_AXIS_DIST * DUOTONE_OFF_AXIS_DIST {
                off_axis += 1;
            }
        }
        if off_axis as f64 > pixel_data.len() as f64 * DUOTONE_MAX_OFF_AXIS_FRACTION {
            return None;
        }
    }

    // Full-resolution luminance grid (transparent pixels count as paper) for
    // the Sauvola local threshold. Global Otsu alone misses faint strokes —
    // thin anti-aliased lines whose pixels never reach the dark mode — while
    // Sauvola alone hollows out solid ink regions larger than its window, so
    // a pixel is ink when EITHER threshold says so.
    let mut lumas = vec![255.0f64; total];
    for p in pixel_data {
        lumas[p.pixel_index] = luma_of(&p.rgb);
    }
    let local_thresholds = sauvola_thresholds(&lumas, width as usize, height as usize);

    let ramp = |l: f64, t: f64| -> f64 {
        if l <= t - LINE_ART_SOFT_WINDOW {
            255.0
        } else if l >= t + LINE_ART_SOFT_WINDOW {
            0.0
        } else {
            (t + LINE_ART_SOFT_WINDOW - l) / (2.0 * LINE_ART_SOFT_WINDOW) * 255.0
        }
    };

    let mut ink_mask = vec![0u8; total];
    let mut paper_mask = vec![0u8; total];
    let mut ink_sum = [0u64; 3];
    let mut ink_count = 0u64;
    let mut paper_sum = [0u64; 3];
    let mut paper_count = 0u64;

    for p in pixel_data {
        let l = luma_of(&p.rgb);
        // Cap the local threshold: Sauvola may rescue faint strokes the
        // global split missed, but an uncapped local threshold near strong
        // edges swallows anti-aliasing halos and bridges nearby strokes
        // into merged blobs.
        let t_local = local_thresholds[p.pixel_index].min(threshold + 2.0 * LINE_ART_SOFT_WINDOW);
        let ink_cov = ramp(l, threshold).max(ramp(l, t_local)).round() as u8;
        ink_mask[p.pixel_index] = ink_cov;
        paper_mask[p.pixel_index] = 255 - ink_cov;

        let side = if l <= threshold || l <= t_local {
            ink_count += 1;
            &mut ink_sum
        } else {
            paper_count += 1;
            &mut paper_sum
        };
        side[0] += p.rgb.r as u64;
        side[1] += p.rgb.g as u64;
        side[2] += p.rgb.b as u64;
    }

    let mean_color = |sum: &[u64; 3], count: u64| -> Rgb {
        if count == 0 {
            return Rgb { r: 0, g: 0, b: 0 };
        }
        Rgb {
            r: ((sum[0] as f64 / count as f64).round()) as u8,
            g: ((sum[1] as f64 / count as f64).round()) as u8,
            b: ((sum[2] as f64 / count as f64).round()) as u8,
        }
    };

    Some(LayerStack {
        layers: vec![
            Layer {
                mask: paper_mask,
                color: mean_color(&paper_sum, paper_count),
                z_order: 0,
            },
            Layer {
                mask: ink_mask,
                color: mean_color(&ink_sum, ink_count),
                z_order: 1,
            },
        ],
    })
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

/// The maximum population fraction of total assigned pixels for a cluster to
/// qualify as a blend of two larger clusters. If a cluster's population
/// fraction is equal to or greater than this, it is considered too large/significant
/// to be dissolved as a minor blend (anti-aliasing halo layer).
const BLEND_MAX_FRACTION: f64 = 0.10;

/// The maximum CIELAB distance from a candidate blend cluster's centroid to
/// the line segment connecting the centroids of two larger clusters. A lower
/// distance ensures that only clusters that lie very close to the interpolation
/// line are dissolved, while a higher distance is more permissive of noisy/curved blends.
const BLEND_LINE_DIST: f64 = 4.0;

/// The maximum CIELAB distance below which a smaller cluster is merged fully
/// into a larger cluster. This is set around the "just-noticeable difference"
/// (JND) threshold of 2.3 Lab units to deduplicate visually indistinguishable
/// color layers.
const DEDUPE_DIST: f64 = 2.3;

/// The elevated CIELAB deduplication distance used for flat-colour artwork.
/// Near-identical shades of a flat colour (such as anti-aliased edge variants or
/// slight compression artifacts) collapse into the dominant flat colour layer.
const FLAT_ART_DEDUPE_DIST: f64 = 10.0;

#[derive(Debug, Clone)]
struct Cluster {
    index: usize,
    centroid: Lab,
    color: Rgb,
    mask: Vec<u8>,
    active: bool,
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
fn auto_color_count(data: &[PixelInfo], cancel: &CancelToken) -> Result<usize, SpryteoError> {
    // The elbow scan only *estimates a colour count* -- it runs k-means
    // up to 15 times (k = 2..=16), so it pays the clustering cost many
    // times over. A small evenly-strided sample estimates the same k
    // (the WSSE curve's shape is what matters, not its absolute values)
    // at a fraction of the cost; deterministic because the stride is.
    const ELBOW_SAMPLE_MAX: usize = 16_384;
    if data.len() > ELBOW_SAMPLE_MAX {
        let stride = data.len().div_ceil(ELBOW_SAMPLE_MAX);
        let sample: Vec<PixelInfo> = data.iter().step_by(stride).cloned().collect();
        return auto_color_count(&sample, cancel);
    }

    let n = data.len();
    if n <= MIN_AUTO_COLORS {
        return Ok(n);
    }
    let k_max = MAX_AUTO_COLORS.min(n);

    let mut prev_error = f64::MAX;

    for k in MIN_AUTO_COLORS..=k_max {
        let assignments = kmeans_pp(data, k, SEED_AUTO, cancel)?;
        let error = within_cluster_error(data, &assignments, k);

        if k > MIN_AUTO_COLORS && prev_error.is_finite() && error.is_finite() {
            let reduction = (prev_error - error) / prev_error;
            if reduction < 0.05 {
                return Ok(k - 1);
            }
        }

        prev_error = error;
    }

    Ok(8)
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
fn kmeans_pp(
    data: &[PixelInfo],
    k: usize,
    seed: u64,
    cancel: &CancelToken,
) -> Result<Vec<usize>, SpryteoError> {
    let n = data.len();
    if n == 0 {
        return Ok(vec![]);
    }
    let k = k.min(n);

    // Large inputs: train on a deterministic stride sample, then assign
    // every point to the nearest trained centroid in one pass.
    if n > KMEANS_SAMPLE_MAX {
        let stride = n.div_ceil(KMEANS_SAMPLE_MAX);
        let sample: Vec<PixelInfo> = data.iter().step_by(stride).cloned().collect();
        let sample_assignments = kmeans_pp(&sample, k, seed, cancel)?;

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
            cancel.check_at(i)?;
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
        return Ok(assignments);
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
        cancel.check()?;
        // Assign each point to nearest centroid
        let mut changed = false;
        for (i, pi) in data.iter().enumerate() {
            cancel.check_at(i)?;
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

    Ok(assignments)
}

// ── LayerStack builders and Dissolution ──────────────────────────────────────

fn get_exact_clusters(pixels: &[u8], width: u32, height: u32, unique: &[Rgb]) -> Vec<Cluster> {
    let total = (width * height) as usize;
    let k = unique.len();
    let mut clusters = Vec::with_capacity(k);
    for (ci, &color) in unique.iter().enumerate().take(k) {
        let centroid = color::srgb_to_lab(&color);
        clusters.push(Cluster {
            index: ci,
            centroid,
            color,
            mask: vec![0u8; total],
            active: true,
        });
    }

    for i in 0..total {
        let idx = i * 4;
        let a = pixels[idx + 3];
        if a == 0 {
            continue;
        }
        let c = Rgb {
            r: pixels[idx],
            g: pixels[idx + 1],
            b: pixels[idx + 2],
        };
        if let Some(ci) = unique.iter().position(|u| *u == c) {
            clusters[ci].mask[i] = 255;
        }
    }
    clusters
}

fn get_kmeans_clusters(
    width: u32,
    height: u32,
    data: &[PixelInfo],
    assignments: &[usize],
    k: usize,
) -> Vec<Cluster> {
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

    let mut clusters = Vec::with_capacity(k);
    for c in 0..k {
        let centroid = if counts[c] > 0 {
            let n = counts[c] as f64;
            Lab {
                l: lab_sums[c].0 / n,
                a: lab_sums[c].1 / n,
                b: lab_sums[c].2 / n,
            }
        } else {
            color::srgb_to_lab(&Rgb { r: 0, g: 0, b: 0 })
        };
        let color = color::lab_to_srgb(&centroid);
        clusters.push(Cluster {
            index: c,
            centroid,
            color,
            mask: std::mem::take(&mut masks[c]),
            active: true,
        });
    }
    clusters
}

/// Deterministically detects whether the pixel data represents flat-colour artwork
/// suffering from colour fragmentation (near-identical shades of a flat colour).
///
/// # Rationale & Heuristic
/// Flat-colour artwork consists of a small number of dominant, uniform color regions
/// (typically 2 to 6 colors), with edge transitions composed of thin anti-aliasing
/// blend pixels. In CIELAB space:
/// 1. A small set of dominant centroids covers the great majority of pixels (`COVER >= 0.80`).
/// 2. The image possesses sufficient resolution/complexity (`MIN_HIST_BINS >= 60`) to exhibit multi-layer color fragmentation.
/// 3. Colour fragmentation occurs when two or more distinct dominant centroids sit very close
///    to each other in CIELAB space (between `DEDUPE_DIST` (2.3) and `FLAT_ART_DEDUPE_DIST` (10.0)),
///    causing a single flat colour region to fragment into multiple near-identical layers.
///
/// In contrast:
/// - Natural photographs and smooth gradients have wide, continuous color variations across
///   Lab space (`COVER < 0.80`).
/// - Clean vector icons with well-separated palette colours have no near-identical dominant centroids.
///
/// # Constants
/// - `HIST_BIN_SIZE = 6.0`: Coarse bin width in Lab space to group nearby pixels.
/// - `TOP_K_DOMINANT = 6`: Number of top dominant color bins considered.
/// - `NEAR_LAB_DIST = 2.5`: Maximum Lab distance for a pixel to count as covered by a dominant centroid.
/// - `MIN_COVERAGE_FRACTION = 0.80`: Minimum pixel fraction (80%) required to classify as flat art.
/// - `MIN_HIST_BINS = 60`: Minimum coarse histogram bins required for flat art classification.
fn is_flat_art(pixel_data: &[PixelInfo]) -> bool {
    const HIST_BIN_SIZE: f64 = 6.0;
    const TOP_K_DOMINANT: usize = 6;
    const NEAR_LAB_DIST: f64 = 2.5;
    const NEAR_LAB_DIST_SQ: f64 = NEAR_LAB_DIST * NEAR_LAB_DIST;
    const MIN_COVERAGE_FRACTION: f64 = 0.80;
    const MIN_HIST_BINS: usize = 60;

    if pixel_data.is_empty() {
        return false;
    }

    struct BinEntry {
        key: (i32, i32, i32),
        count: usize,
        sum_l: f64,
        sum_a: f64,
        sum_b: f64,
    }

    let mut bins: Vec<BinEntry> = Vec::new();

    for p in pixel_data {
        let bin_key = (
            (p.lab.l / HIST_BIN_SIZE).floor() as i32,
            (p.lab.a / HIST_BIN_SIZE).floor() as i32,
            (p.lab.b / HIST_BIN_SIZE).floor() as i32,
        );

        match bins.binary_search_by_key(&bin_key, |b| b.key) {
            Ok(idx) => {
                bins[idx].count += 1;
                bins[idx].sum_l += p.lab.l;
                bins[idx].sum_a += p.lab.a;
                bins[idx].sum_b += p.lab.b;
            }
            Err(idx) => {
                bins.insert(
                    idx,
                    BinEntry {
                        key: bin_key,
                        count: 1,
                        sum_l: p.lab.l,
                        sum_a: p.lab.a,
                        sum_b: p.lab.b,
                    },
                );
            }
        }
    }

    if bins.len() < MIN_HIST_BINS {
        return false;
    }

    // Sort bins deterministically by count descending, breaking ties by bin key
    bins.sort_by(|b1, b2| b2.count.cmp(&b1.count).then_with(|| b1.key.cmp(&b2.key)));

    let k = TOP_K_DOMINANT.min(bins.len());
    let raw_centroids: Vec<(Lab, usize)> = bins[..k]
        .iter()
        .map(|b| {
            (
                Lab {
                    l: b.sum_l / b.count as f64,
                    a: b.sum_a / b.count as f64,
                    b: b.sum_b / b.count as f64,
                },
                b.count,
            )
        })
        .collect();

    // Merge coarse bins within DEDUPE_DIST (2.3)
    let mut merged_centroids: Vec<(Lab, usize)> = Vec::new();
    for (lab, count) in raw_centroids {
        if let Some(existing) = merged_centroids
            .iter_mut()
            .find(|(m_lab, _)| color::lab_distance_sq(&lab, m_lab).sqrt() <= DEDUPE_DIST)
        {
            let total_count = existing.1 + count;
            existing.0 = Lab {
                l: (existing.0.l * existing.1 as f64 + lab.l * count as f64) / total_count as f64,
                a: (existing.0.a * existing.1 as f64 + lab.a * count as f64) / total_count as f64,
                b: (existing.0.b * existing.1 as f64 + lab.b * count as f64) / total_count as f64,
            };
            existing.1 = total_count;
        } else {
            merged_centroids.push((lab, count));
        }
    }

    let dominant_labs: Vec<Lab> = merged_centroids.into_iter().map(|(lab, _)| lab).collect();

    let near_count = pixel_data
        .iter()
        .filter(|p| {
            dominant_labs
                .iter()
                .any(|c| color::lab_distance_sq(&p.lab, c) <= NEAR_LAB_DIST_SQ)
        })
        .count();

    let cover = near_count as f64 / pixel_data.len() as f64;
    if cover < MIN_COVERAGE_FRACTION {
        return false;
    }

    for i in 0..dominant_labs.len() {
        for j in (i + 1)..dominant_labs.len() {
            let dist = color::lab_distance_sq(&dominant_labs[i], &dominant_labs[j]).sqrt();
            if dist > DEDUPE_DIST && dist < FLAT_ART_DEDUPE_DIST {
                return true;
            }
        }
    }

    false
}

#[allow(clippy::needless_range_loop)]
fn dissolve_blend_clusters(
    clusters: &mut [Cluster],
    pixel_data: &[PixelInfo],
    total: usize,
    is_flat: bool,
    cancel: &CancelToken,
) -> Result<(), SpryteoError> {
    let total_assigned_pixels = pixel_data.len();
    if total_assigned_pixels == 0 {
        return Ok(());
    }

    let dedupe_dist = if is_flat {
        FLAT_ART_DEDUPE_DIST
    } else {
        DEDUPE_DIST
    };

    // Lookup table for pixel Lab colors
    let mut pixel_labs = vec![None; total];
    for pi in pixel_data {
        pixel_labs[pi.pixel_index] = Some(pi.lab);
    }

    for _iter in 0..32 {
        cancel.check()?;
        // 1. Compute populations of active clusters
        let mut active_indices: Vec<usize> = (0..clusters.len())
            .filter(|&idx| clusters[idx].active)
            .collect();

        if active_indices.len() <= 1 {
            break;
        }

        let mut populations = vec![0; clusters.len()];
        for &idx in &active_indices {
            populations[idx] = clusters[idx].mask.iter().filter(|&&v| v == 255).count();
        }

        // Sort active indices from smallest population to largest
        // Tie-break: lower cluster index first
        active_indices.sort_by(|&i1, &i2| {
            let p1 = populations[i1];
            let p2 = populations[i2];
            if p1 != p2 {
                p1.cmp(&p2)
            } else {
                clusters[i1].index.cmp(&clusters[i2].index)
            }
        });

        let mut changed = false;

        // Process from smallest population to largest
        for i in 0..active_indices.len() {
            let c_idx = active_indices[i];
            if !clusters[c_idx].active {
                continue;
            }

            let c_pop = populations[c_idx];
            let c_lab = clusters[c_idx].centroid;

            // d. Deduplication: if c's centroid is within dedupe_dist of a LARGER cluster a, merge fully into a
            let mut best_merge_idx = None;
            let mut min_merge_dist = f64::MAX;

            for &a_idx in active_indices.iter().skip(i + 1) {
                if !clusters[a_idx].active {
                    continue;
                }
                let a_lab = clusters[a_idx].centroid;
                let dist = color::lab_distance_sq(&c_lab, &a_lab).sqrt();
                if dist < dedupe_dist && dist < min_merge_dist {
                    min_merge_dist = dist;
                    best_merge_idx = Some(a_idx);
                }
            }

            if let Some(a_idx) = best_merge_idx {
                for p in 0..total {
                    if clusters[c_idx].mask[p] > 0 {
                        clusters[a_idx].mask[p] = 255;
                        clusters[c_idx].mask[p] = 0;
                    }
                }
                clusters[c_idx].active = false;
                changed = true;
                continue;
            }

            // b. Blend dissolution: c with population fraction < BLEND_MAX_FRACTION
            let c_frac = c_pop as f64 / total_assigned_pixels as f64;
            if c_frac < BLEND_MAX_FRACTION {
                let mut best_pair = None;
                let mut min_segment_dist = f64::MAX;

                for j in (i + 1)..active_indices.len() {
                    let a_idx = active_indices[j];
                    if !clusters[a_idx].active {
                        continue;
                    }
                    let a_lab = clusters[a_idx].centroid;

                    for &b_idx in active_indices.iter().skip(j + 1) {
                        if !clusters[b_idx].active {
                            continue;
                        }
                        let b_lab = clusters[b_idx].centroid;

                        let v_l = b_lab.l - a_lab.l;
                        let v_a = b_lab.a - a_lab.a;
                        let v_b = b_lab.b - a_lab.b;
                        let len_sq = v_l * v_l + v_a * v_a + v_b * v_b;

                        if len_sq > 1e-9 {
                            let t = ((c_lab.l - a_lab.l) * v_l
                                + (c_lab.a - a_lab.a) * v_a
                                + (c_lab.b - a_lab.b) * v_b)
                                / len_sq;

                            if t > 0.10 && t < 0.90 {
                                let p_l = a_lab.l + t * v_l;
                                let p_a = a_lab.a + t * v_a;
                                let p_b = a_lab.b + t * v_b;

                                let dist = ((c_lab.l - p_l).powi(2)
                                    + (c_lab.a - p_a).powi(2)
                                    + (c_lab.b - p_b).powi(2))
                                .sqrt();

                                if dist < BLEND_LINE_DIST && dist < min_segment_dist {
                                    min_segment_dist = dist;
                                    best_pair = Some((a_idx, b_idx));
                                }
                            }
                        }
                    }
                }

                if let Some((a_idx, b_idx)) = best_pair {
                    let a_lab = clusters[a_idx].centroid;
                    let b_lab = clusters[b_idx].centroid;

                    let v_l = b_lab.l - a_lab.l;
                    let v_a = b_lab.a - a_lab.a;
                    let v_b = b_lab.b - a_lab.b;
                    let len_sq = v_l * v_l + v_a * v_a + v_b * v_b;

                    for p in 0..total {
                        if clusters[c_idx].mask[p] > 0 {
                            let p_lab = pixel_labs[p].unwrap();
                            let t_p = if len_sq > 1e-9 {
                                (((p_lab.l - a_lab.l) * v_l
                                    + (p_lab.a - a_lab.a) * v_a
                                    + (p_lab.b - a_lab.b) * v_b)
                                    / len_sq)
                                    .clamp(0.0, 1.0)
                            } else {
                                0.0
                            };

                            let mask_val = clusters[c_idx].mask[p] as f64;
                            let val_a = ((1.0 - t_p) * mask_val).round() as u8;
                            let val_b = (t_p * mask_val).round() as u8;

                            clusters[a_idx].mask[p] = clusters[a_idx].mask[p].saturating_add(val_a);
                            clusters[b_idx].mask[p] = clusters[b_idx].mask[p].saturating_add(val_b);
                            clusters[c_idx].mask[p] = 0;
                        }
                    }

                    clusters[c_idx].active = false;
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }
    Ok(())
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
    cancel: &CancelToken,
) -> Result<LayerStack, SpryteoError> {
    let total = (width * height) as usize;
    let k = palette_lab.len();
    let mut masks = vec![vec![0u8; total]; k];

    for i in 0..total {
        cancel.check_at(i)?;
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

    Ok(LayerStack { layers })
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
#[path = "quantize_tests.rs"]
mod tests;
