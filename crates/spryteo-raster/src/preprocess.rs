use spryteo_core::{CancelToken, Mode, RasterImage, SpryteoError};

/// Apply a bilateral (edge-preserving) filter to a raster image.
///
/// For each pixel, this computes a weighted average of its neighbours within a spatial window.
/// The weight of each neighbour is the product of a spatial Gaussian (based on pixel distance)
/// and a range/colour Gaussian (based on colour difference in the RGB space from the centre pixel).
///
/// This filter is applied consistently across the R, G, and B channels using a single calculated
/// weight per neighbour to avoid color shifts. The alpha channel is preserved exactly.
///
/// # Parameter Derivation
///
/// - `σ_space` is scaled linearly with the minimum dimension of the image to ensure the smoothing effect
///   remains proportional to image resolution: `σ_space = max(1.0, min(width, height) / 200.0)`.
/// - `σ_color` is fixed at `20.0` to reflect typical color transitions in the standard `0-255` range.
/// - The local search window radius is set to `2 * ceil(σ_space)` to bound the computation.
///
/// # Performance
///
/// Implemented as a *separable* bilateral filter: a horizontal 1D pass
/// followed by a vertical 1D pass, each using the same spatial/colour
/// Gaussians. This is the standard real-time approximation of the true
/// 2D bilateral filter -- O(2r) neighbours per pixel instead of O(r^2)
/// (46 vs 529 taps at the radius a 1024px photo gets). The full 2D
/// version took ~20s per megapixel (each tap paying two `exp()` calls),
/// freezing the browser tab, since WASM runs on the main thread. Both
/// Gaussian weights come from precomputed lookup tables (pixel channel
/// values are integers, so colour distances form a small discrete set).
pub fn bilateral_filter(image: &RasterImage) -> RasterImage {
    bilateral_filter_cancellable(image, &CancelToken::none())
        .expect("bilateral_filter with an inert CancelToken cannot be cancelled")
}

/// [`bilateral_filter`] with cooperative cancellation (ROADMAP §3.1).
///
/// This is the single most expensive stage on a megapixel photo (~500ms),
/// so it polls per scanline of each separable pass rather than only at the
/// stage boundary — a timeout set mid-filter takes effect within one row.
pub fn bilateral_filter_cancellable(
    image: &RasterImage,
    cancel: &CancelToken,
) -> Result<RasterImage, SpryteoError> {
    let width = image.width;
    let height = image.height;
    if width == 0 || height == 0 || image.pixels.is_empty() {
        return Ok(image.clone());
    }

    let sigma_space = ((width.min(height) as f64) / 200.0).max(1.0);
    let sigma_color = 20.0;

    let radius = (2.0 * sigma_space).ceil() as i32;

    let two_sigma_space_sq = 2.0 * sigma_space * sigma_space;
    let two_sigma_color_sq = 2.0 * sigma_color * sigma_color;

    // Precomputed Gaussian weight tables (see doc comment above).
    let spatial_lut: Vec<f64> = (-radius..=radius)
        .map(|d| (-((d * d) as f64) / two_sigma_space_sq).exp())
        .collect();
    const MAX_COLOR_DIST_SQ: usize = 3 * 255 * 255;
    let color_lut: Vec<f64> = (0..=MAX_COLOR_DIST_SQ)
        .map(|d| (-(d as f64) / two_sigma_color_sq).exp())
        .collect();

    // One 1D bilateral pass along either axis. `stride` is the pixel
    // step between neighbours on the axis (1 for horizontal, `width`
    // for vertical); `len` is the number of pixels along the axis.
    let one_d_pass = |src: &[u8], horizontal: bool| -> Result<Vec<u8>, SpryteoError> {
        let mut out = src.to_vec();
        let (outer, len) = if horizontal {
            (height as usize, width as usize)
        } else {
            (width as usize, height as usize)
        };
        for o in 0..outer {
            cancel.check()?;
            for i in 0..len {
                let pixel = if horizontal {
                    o * len + i
                } else {
                    i * (width as usize) + o
                };
                let center_idx = 4 * pixel;
                let center_r = src[center_idx] as i32;
                let center_g = src[center_idx + 1] as i32;
                let center_b = src[center_idx + 2] as i32;

                let mut sum_r = 0.0;
                let mut sum_g = 0.0;
                let mut sum_b = 0.0;
                let mut sum_w = 0.0;

                let lo = (i as i32 - radius).max(0) as usize;
                let hi = ((i as i32 + radius) as usize).min(len - 1);
                for j in lo..=hi {
                    let neighbor_pixel = if horizontal {
                        o * len + j
                    } else {
                        j * (width as usize) + o
                    };
                    let idx = 4 * neighbor_pixel;
                    let nr = src[idx] as i32;
                    let ng = src[idx + 1] as i32;
                    let nb = src[idx + 2] as i32;

                    let dr = nr - center_r;
                    let dg = ng - center_g;
                    let db = nb - center_b;
                    let d_color_sq = (dr * dr + dg * dg + db * db) as usize;

                    let w = spatial_lut[(j as i32 - i as i32 + radius) as usize]
                        * color_lut[d_color_sq];

                    sum_r += w * nr as f64;
                    sum_g += w * ng as f64;
                    sum_b += w * nb as f64;
                    sum_w += w;
                }

                if sum_w > 0.0 {
                    out[center_idx] = (sum_r / sum_w).round().clamp(0.0, 255.0) as u8;
                    out[center_idx + 1] = (sum_g / sum_w).round().clamp(0.0, 255.0) as u8;
                    out[center_idx + 2] = (sum_b / sum_w).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Ok(out)
    };

    let horizontal = one_d_pass(&image.pixels, true)?;
    let filtered_pixels = one_d_pass(&horizontal, false)?;

    Ok(RasterImage {
        width,
        height,
        pixels: filtered_pixels,
    })
}

/// Estimate the JPEG quality of an image based on 8x8 block boundary discontinuities.
///
/// This measures blockiness by comparing pixel value discontinuities specifically AT 8-pixel-grid-aligned
/// boundaries versus discontinuities WITHIN the 8x8 blocks.
///
/// It returns a monotonic quality estimate in the range `0.0` to `100.0`. A score of `100.0` represents
/// no blockiness (or a flat image), while low scores represent high blockiness (heavy compression).
pub fn estimate_jpeg_quality(image: &RasterImage) -> f64 {
    let width = image.width;
    let height = image.height;
    if width <= 1 && height <= 1 {
        return 100.0;
    }

    let mut boundary_sum = 0.0;
    let mut boundary_count = 0;
    let mut internal_sum = 0.0;
    let mut internal_count = 0;

    // Horizontal differences (discontinuities between adjacent columns)
    if width > 1 {
        for y in 0..height {
            for x in 0..width - 1 {
                let idx1 = 4 * (y * width + x) as usize;
                let idx2 = 4 * (y * width + x + 1) as usize;
                let diff = ((image.pixels[idx1] as f64 - image.pixels[idx2] as f64).abs()
                    + (image.pixels[idx1 + 1] as f64 - image.pixels[idx2 + 1] as f64).abs()
                    + (image.pixels[idx1 + 2] as f64 - image.pixels[idx2 + 2] as f64).abs())
                    / 3.0;

                if (x + 1) % 8 == 0 {
                    boundary_sum += diff;
                    boundary_count += 1;
                } else {
                    internal_sum += diff;
                    internal_count += 1;
                }
            }
        }
    }

    // Vertical differences (discontinuities between adjacent rows)
    if height > 1 {
        for x in 0..width {
            for y in 0..height - 1 {
                let idx1 = 4 * (y * width + x) as usize;
                let idx2 = 4 * ((y + 1) * width + x) as usize;
                let diff = ((image.pixels[idx1] as f64 - image.pixels[idx2] as f64).abs()
                    + (image.pixels[idx1 + 1] as f64 - image.pixels[idx2 + 1] as f64).abs()
                    + (image.pixels[idx1 + 2] as f64 - image.pixels[idx2 + 2] as f64).abs())
                    / 3.0;

                if (y + 1) % 8 == 0 {
                    boundary_sum += diff;
                    boundary_count += 1;
                } else {
                    internal_sum += diff;
                    internal_count += 1;
                }
            }
        }
    }

    if boundary_count == 0 {
        return 100.0;
    }

    let avg_boundary = boundary_sum / boundary_count as f64;
    let avg_internal = internal_sum / internal_count as f64;

    if avg_internal < 1e-5 {
        if avg_boundary > 1e-5 {
            return 0.0;
        } else {
            return 100.0;
        }
    }

    let ratio = avg_boundary / avg_internal;
    if ratio <= 1.0 {
        100.0
    } else {
        (100.0 / ratio).clamp(0.0, 100.0)
    }
}

/// Apply a JPEG deblocking pass to a raster image.
///
/// This applies a 3-tap low-pass smoothing filter `[0.25, 0.5, 0.25]` concentrated at the 8x8 block
/// boundaries. This reduces the blockiness at the boundaries without blurring the rest of the image.
/// The alpha channel is preserved exactly.
pub fn deblock(image: &RasterImage) -> RasterImage {
    let width = image.width;
    let height = image.height;
    if width <= 2 || height <= 2 {
        return image.clone();
    }

    // Step 1: Horizontal deblocking (blur horizontally adjacent columns on the 8px boundaries)
    let mut h_deblocked = image.pixels.clone();
    for y in 0..height {
        for x in 1..(width - 1) {
            if (x % 8 == 0) || ((x + 1) % 8 == 0) {
                let idx = 4 * (y * width + x) as usize;
                let idx_left = idx - 4;
                let idx_right = idx + 4;

                for c in 0..3 {
                    let val = 0.25 * image.pixels[idx_left + c] as f64
                        + 0.5 * image.pixels[idx + c] as f64
                        + 0.25 * image.pixels[idx_right + c] as f64;
                    h_deblocked[idx + c] = val.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    // Step 2: Vertical deblocking (blur vertically adjacent rows on the 8px boundaries)
    let mut final_pixels = h_deblocked.clone();
    for y in 1..(height - 1) {
        if (y % 8 == 0) || ((y + 1) % 8 == 0) {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let idx_up = idx - 4 * width as usize;
                let idx_down = idx + 4 * width as usize;

                for c in 0..3 {
                    let val = 0.25 * h_deblocked[idx_up + c] as f64
                        + 0.5 * h_deblocked[idx + c] as f64
                        + 0.25 * h_deblocked[idx_down + c] as f64;
                    final_pixels[idx + c] = val.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    RasterImage {
        width,
        height,
        pixels: final_pixels,
    }
}

/// Long-edge threshold above which photo-mode inputs are downscaled
/// before the expensive quantize/trace/fit stages (ROADMAP.md §8:
/// "2048^2 photo (auto-downscaled to 1024) < 2s"). Icon/pixel-art/
/// line-art modes are never downscaled -- fidelity at native
/// resolution matters more for those and they're cheap regardless.
const PHOTO_DOWNSCALE_THRESHOLD: u32 = 1600;
const PHOTO_DOWNSCALE_TARGET: u32 = 1024;

pub fn downscale_large_photo(image: RasterImage, mode: &Mode) -> RasterImage {
    if !matches!(mode, Mode::Photo) {
        return image;
    }
    let max_dim = image.width.max(image.height);
    if max_dim <= PHOTO_DOWNSCALE_THRESHOLD {
        return image;
    }

    let (new_width, new_height) = if image.width >= image.height {
        let new_w = PHOTO_DOWNSCALE_TARGET;
        let new_h = ((image.height as f64 * PHOTO_DOWNSCALE_TARGET as f64) / image.width as f64)
            .round() as u32;
        (new_w, new_h.max(1))
    } else {
        let new_h = PHOTO_DOWNSCALE_TARGET;
        let new_w = ((image.width as f64 * PHOTO_DOWNSCALE_TARGET as f64) / image.height as f64)
            .round() as u32;
        (new_w.max(1), new_h)
    };

    resize_rgba(image, new_width, new_height)
}

/// Resize `image` so its long edge is at most `max_dim`, preserving aspect
/// ratio. Images already within the limit are returned untouched.
///
/// This is the explicit, user-facing counterpart to
/// [`downscale_large_photo`]'s automatic photo rule (§3.1,
/// `ConvertOptions::max_trace_dimension`). The engine applies this one
/// first; because it can only ever shrink the image, an input already
/// reduced to `max_dim` is at or below the automatic rule's 1600px
/// threshold whenever `max_dim <= 1600`, so the two never fight — the
/// tighter of the two limits wins.
///
/// `max_dim == 0` is rejected upstream by option validation, but is treated
/// as "no downscale" here so this function is total.
///
/// `mode` selects the resampling filter, which matters more than it looks.
/// Lanczos3 has negative lobes: downscaling flat artwork with it rings
/// around every hard colour edge, manufacturing intermediate colours that
/// did not exist in the source. Those extra colours survive quantization as
/// their own layers, which both defeats primitive recognition and inflates
/// the output — a 256px circle that traces to a single `<circle>` at native
/// size came back as a pile of paths when Lanczos-reduced to 32px. Flat
/// modes therefore use a triangle filter, which cannot overshoot.
pub fn downscale_to_max_dimension(image: RasterImage, max_dim: u32, mode: &Mode) -> RasterImage {
    if max_dim == 0 {
        return image;
    }
    let long_edge = image.width.max(image.height);
    if long_edge <= max_dim {
        return image;
    }

    let scale = max_dim as f64 / long_edge as f64;
    let new_width = ((image.width as f64 * scale).round() as u32).max(1);
    let new_height = ((image.height as f64 * scale).round() as u32).max(1);

    let filter = match mode {
        // Photographs have no hard flat regions to ring around, and
        // Lanczos3 keeps the most detail per pixel.
        Mode::Photo => image::imageops::FilterType::Lanczos3,
        // Pixel art must never gain colours it did not have.
        Mode::PixelArt => image::imageops::FilterType::Nearest,
        Mode::Icon | Mode::LineArt | Mode::Auto => image::imageops::FilterType::Triangle,
    };

    resize_rgba_with(image, new_width, new_height, filter)
}

/// The scale factor [`downscale_to_max_dimension`] would apply, or 1.0 if it
/// would leave the image alone.
///
/// The engine uses this to scale geometry-sensitive options (tolerance in
/// pixels, turdsize in square pixels) into the reduced coordinate system, so
/// `--max-trace-dimension` changes the working resolution without silently
/// changing how aggressively the tracer simplifies or despeckles.
pub fn downscale_factor(width: u32, height: u32, max_dim: u32) -> f64 {
    if max_dim == 0 {
        return 1.0;
    }
    let long_edge = width.max(height);
    if long_edge <= max_dim {
        return 1.0;
    }
    max_dim as f64 / long_edge as f64
}

/// Lanczos3 resize of an RGBA `RasterImage`, used by the automatic photo
/// rule (which by definition only ever sees photographs).
fn resize_rgba(image: RasterImage, new_width: u32, new_height: u32) -> RasterImage {
    resize_rgba_with(
        image,
        new_width,
        new_height,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Resize an RGBA `RasterImage` with an explicit filter, compositing in
/// premultiplied alpha.
///
/// `image::imageops::resize` treats the four channels independently, which
/// is wrong for straight (un-premultiplied) alpha: the RGB stored under
/// fully transparent pixels is averaged into neighbouring visible pixels,
/// so an icon whose transparent margin happens to hold green bytes picks up
/// a green halo — and, once quantized, a whole green layer that was never
/// visible in the source. Premultiplying before the resample and dividing
/// back out afterwards weights every colour by its coverage, so invisible
/// pixels contribute nothing.
fn resize_rgba_with(
    image: RasterImage,
    new_width: u32,
    new_height: u32,
    filter: image::imageops::FilterType,
) -> RasterImage {
    let mut image = image;
    let fully_opaque = image.pixels.chunks_exact(4).all(|px| px[3] == 255);
    if !fully_opaque {
        for px in image.pixels.chunks_exact_mut(4) {
            let a = px[3] as u32;
            px[0] = ((px[0] as u32 * a + 127) / 255) as u8;
            px[1] = ((px[1] as u32 * a + 127) / 255) as u8;
            px[2] = ((px[2] as u32 * a + 127) / 255) as u8;
        }
    }

    let width = image.width;
    let height = image.height;
    let pixels = image.pixels;

    let rgba = match image::RgbaImage::from_raw(width, height, pixels) {
        Some(img) => img,
        None => {
            return RasterImage {
                width,
                height,
                pixels: Vec::new(),
            };
        }
    };

    let resized = image::imageops::resize(&rgba, new_width, new_height, filter);
    let mut pixels = resized.into_raw();

    if !fully_opaque {
        // Undo the premultiply. Zero-coverage pixels have no recoverable
        // colour, so they stay at 0 rather than inventing one.
        for px in pixels.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a == 0 {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            } else {
                px[0] = (((px[0] as u32 * 255) + a / 2) / a).min(255) as u8;
                px[1] = (((px[1] as u32 * 255) + a / 2) / a).min(255) as u8;
                px[2] = (((px[2] as u32 * 255) + a / 2) / a).min(255) as u8;
            }
        }
    }

    RasterImage {
        width: new_width,
        height: new_height,
        pixels,
    }
}

/// Orchestrates image preprocessing steps.
///
/// 1. Skips bilateral filter entirely if `mode` is `Mode::PixelArt` to keep details crisp.
/// 2. Applies `deblock` only when `was_jpeg` is true AND `estimate_jpeg_quality` is below `90.0`.
/// 3. Applies `bilateral_filter` (if not pixel art) after the deblock step.
pub fn preprocess(image: RasterImage, mode: &Mode, was_jpeg: bool) -> RasterImage {
    preprocess_cancellable(image, mode, was_jpeg, &CancelToken::none())
        .expect("preprocess with an inert CancelToken cannot be cancelled")
}

/// [`preprocess`] with cooperative cancellation (ROADMAP §3.1).
pub fn preprocess_cancellable(
    image: RasterImage,
    mode: &Mode,
    was_jpeg: bool,
    cancel: &CancelToken,
) -> Result<RasterImage, SpryteoError> {
    let mut current = image;

    cancel.check()?;
    if was_jpeg && estimate_jpeg_quality(&current) < 90.0 {
        current = deblock(&current);
    }

    if !matches!(mode, Mode::PixelArt) {
        current = bilateral_filter_cancellable(&current, cancel)?;
    }

    Ok(current)
}
#[cfg(test)]
#[path = "preprocess_tests.rs"]
mod tests;
