use spryteo_core::{Mode, RasterImage};

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
pub fn bilateral_filter(image: &RasterImage) -> RasterImage {
    let width = image.width;
    let height = image.height;
    if width == 0 || height == 0 || image.pixels.is_empty() {
        return image.clone();
    }

    let sigma_space = ((width.min(height) as f64) / 200.0).max(1.0);
    let sigma_color = 20.0;

    let radius = (2.0 * sigma_space).ceil() as i32;

    let mut filtered_pixels = image.pixels.clone();

    let two_sigma_space_sq = 2.0 * sigma_space * sigma_space;
    let two_sigma_color_sq = 2.0 * sigma_color * sigma_color;

    for y in 0..height {
        for x in 0..width {
            let center_idx = 4 * (y * width + x) as usize;
            let center_r = image.pixels[center_idx] as f64;
            let center_g = image.pixels[center_idx + 1] as f64;
            let center_b = image.pixels[center_idx + 2] as f64;

            let mut sum_r = 0.0;
            let mut sum_g = 0.0;
            let mut sum_b = 0.0;
            let mut sum_w = 0.0;

            let y_min = (y as i32 - radius).max(0);
            let y_max = (y as i32 + radius).min(height as i32 - 1);
            let x_min = (x as i32 - radius).max(0);
            let x_max = (x as i32 + radius).min(width as i32 - 1);

            for ny in y_min..=y_max {
                for nx in x_min..=x_max {
                    let neighbor_idx = 4 * (ny * width as i32 + nx) as usize;
                    let neighbor_r = image.pixels[neighbor_idx] as f64;
                    let neighbor_g = image.pixels[neighbor_idx + 1] as f64;
                    let neighbor_b = image.pixels[neighbor_idx + 2] as f64;

                    let d_space_sq =
                        ((nx - x as i32) as f64).powi(2) + ((ny - y as i32) as f64).powi(2);
                    let d_color_sq = (neighbor_r - center_r).powi(2)
                        + (neighbor_g - center_g).powi(2)
                        + (neighbor_b - center_b).powi(2);

                    let w_space = (-d_space_sq / two_sigma_space_sq).exp();
                    let w_color = (-d_color_sq / two_sigma_color_sq).exp();
                    let w = w_space * w_color;

                    sum_r += w * neighbor_r;
                    sum_g += w * neighbor_g;
                    sum_b += w * neighbor_b;
                    sum_w += w;
                }
            }

            if sum_w > 0.0 {
                filtered_pixels[center_idx] = (sum_r / sum_w).round().clamp(0.0, 255.0) as u8;
                filtered_pixels[center_idx + 1] = (sum_g / sum_w).round().clamp(0.0, 255.0) as u8;
                filtered_pixels[center_idx + 2] = (sum_b / sum_w).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    RasterImage {
        width,
        height,
        pixels: filtered_pixels,
    }
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

    let resized = image::imageops::resize(
        &rgba,
        new_width,
        new_height,
        image::imageops::FilterType::Lanczos3,
    );

    RasterImage {
        width: new_width,
        height: new_height,
        pixels: resized.into_raw(),
    }
}

/// Orchestrates image preprocessing steps.
///
/// 1. Skips bilateral filter entirely if `mode` is `Mode::PixelArt` to keep details crisp.
/// 2. Applies `deblock` only when `was_jpeg` is true AND `estimate_jpeg_quality` is below `90.0`.
/// 3. Applies `bilateral_filter` (if not pixel art) after the deblock step.
pub fn preprocess(image: RasterImage, mode: &Mode, was_jpeg: bool) -> RasterImage {
    let mut current = image;

    if was_jpeg && estimate_jpeg_quality(&current) < 90.0 {
        current = deblock(&current);
    }

    if !matches!(mode, Mode::PixelArt) {
        current = bilateral_filter(&current);
    }

    current
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deterministic pseudo-random number generator (LCG)
    struct SimpleRng {
        state: u32,
    }

    impl SimpleRng {
        fn new(seed: u32) -> Self {
            Self { state: seed }
        }

        fn next_i32(&mut self, min: i32, max: i32) -> i32 {
            self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (self.state / 65536) % 32768;
            let range = max - min + 1;
            min + (val as i32 % range)
        }
    }

    fn compute_variance(image: &RasterImage) -> f64 {
        let n = (image.width * image.height) as f64;
        if n == 0.0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for i in 0..(image.width * image.height) as usize {
            let idx = i * 4;
            let val = (image.pixels[idx] as f64
                + image.pixels[idx + 1] as f64
                + image.pixels[idx + 2] as f64)
                / 3.0;
            sum += val;
        }
        let mean = sum / n;

        let mut sum_sq_diff = 0.0;
        for i in 0..(image.width * image.height) as usize {
            let idx = i * 4;
            let val = (image.pixels[idx] as f64
                + image.pixels[idx + 1] as f64
                + image.pixels[idx + 2] as f64)
                / 3.0;
            sum_sq_diff += (val - mean).powi(2);
        }
        sum_sq_diff / n
    }

    #[test]
    fn test_bilateral_filter_reduces_noise_variance() {
        let width = 32;
        let height = 32;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut rng = SimpleRng::new(42);

        // Fill with solid color + noise
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let noise = rng.next_i32(-15, 15);
                let r = (128 + noise).clamp(0, 255) as u8;
                let g = (128 + noise).clamp(0, 255) as u8;
                let b = (128 + noise).clamp(0, 255) as u8;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = 255;
            }
        }

        let input_img = RasterImage {
            width,
            height,
            pixels,
        };
        let var_before = compute_variance(&input_img);

        let filtered_img = bilateral_filter(&input_img);
        let var_after = compute_variance(&filtered_img);

        assert!(
            var_after < var_before * 0.5,
            "Variance should be significantly reduced (before: {}, after: {})",
            var_before,
            var_after
        );
    }

    #[test]
    fn test_bilateral_filter_preserves_sharp_edges() {
        let width = 20;
        let height = 20;
        let mut pixels = vec![0u8; (width * height * 4) as usize];

        // Left half red, right half cyan
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                if x < 10 {
                    pixels[idx] = 255; // R
                    pixels[idx + 1] = 0; // G
                    pixels[idx + 2] = 0; // B
                } else {
                    pixels[idx] = 0; // R
                    pixels[idx + 1] = 255; // G
                    pixels[idx + 2] = 255; // B
                }
                pixels[idx + 3] = 255;
            }
        }

        let input_img = RasterImage {
            width,
            height,
            pixels,
        };
        let filtered_img = bilateral_filter(&input_img);

        // Check values a few pixels away from the boundary (x = 10)
        let sample_left_idx = 4 * (10 * width + 7) as usize;
        assert!(
            filtered_img.pixels[sample_left_idx] > 240,
            "Red channel should remain high on the left"
        );
        assert!(
            filtered_img.pixels[sample_left_idx + 1] < 15,
            "Green channel should remain low on the left"
        );
        assert!(
            filtered_img.pixels[sample_left_idx + 2] < 15,
            "Blue channel should remain low on the left"
        );

        let sample_right_idx = 4 * (10 * width + 12) as usize;
        assert!(
            filtered_img.pixels[sample_right_idx] < 15,
            "Red channel should remain low on the right"
        );
        assert!(
            filtered_img.pixels[sample_right_idx + 1] > 240,
            "Green channel should remain high on the right"
        );
        assert!(
            filtered_img.pixels[sample_right_idx + 2] > 240,
            "Blue channel should remain high on the right"
        );
    }

    #[test]
    fn test_preprocess_mode_skip_bilateral() {
        let width = 16;
        let height = 16;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let mut rng = SimpleRng::new(99);

        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let val = (128 + rng.next_i32(-10, 10)) as u8;
                pixels[idx] = val;
                pixels[idx + 1] = val;
                pixels[idx + 2] = val;
                pixels[idx + 3] = 255;
            }
        }

        let input_img = RasterImage {
            width,
            height,
            pixels,
        };

        // Under Mode::PixelArt, preprocess should skip filtering and return byte-identical image
        let result_pixel_art = preprocess(input_img.clone(), &Mode::PixelArt, false);
        assert_eq!(
            result_pixel_art.pixels, input_img.pixels,
            "PixelArt mode must not modify image"
        );

        // Under other modes (e.g. Mode::Photo), preprocess must modify the image
        let result_photo = preprocess(input_img.clone(), &Mode::Photo, false);
        assert_ne!(
            result_photo.pixels, input_img.pixels,
            "Photo mode should apply bilateral filter"
        );
    }

    #[test]
    fn test_estimate_jpeg_quality_blocky_vs_smooth() {
        // Construct a blocky 16x16 image (jump at boundary x=8, y=8)
        let width = 16;
        let height = 16;
        let mut blocky_pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let color_val = if x < 8 {
                    if y < 8 {
                        50
                    } else {
                        150
                    }
                } else {
                    if y < 8 {
                        100
                    } else {
                        200
                    }
                };
                blocky_pixels[idx] = color_val;
                blocky_pixels[idx + 1] = color_val;
                blocky_pixels[idx + 2] = color_val;
                blocky_pixels[idx + 3] = 255;
            }
        }
        let blocky_img = RasterImage {
            width,
            height,
            pixels: blocky_pixels,
        };

        // Construct a smooth gradient image
        let mut smooth_pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let val = (x * 4 + y * 4) as u8;
                smooth_pixels[idx] = val;
                smooth_pixels[idx + 1] = val;
                smooth_pixels[idx + 2] = val;
                smooth_pixels[idx + 3] = 255;
            }
        }
        let smooth_img = RasterImage {
            width,
            height,
            pixels: smooth_pixels,
        };

        let q_blocky = estimate_jpeg_quality(&blocky_img);
        let q_smooth = estimate_jpeg_quality(&smooth_img);

        assert!(
            q_blocky < q_smooth,
            "Blocky image quality ({}) must be lower than smooth image quality ({})",
            q_blocky,
            q_smooth
        );
        assert!(q_blocky < 90.0, "Blocky image should score below threshold");
        assert!(
            q_smooth >= 90.0,
            "Smooth image should score above threshold"
        );
    }

    #[test]
    fn test_deblock_reduces_blockiness_and_preprocesses_conditionally() {
        let width = 16;
        let height = 16;
        let mut blocky_pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = 4 * (y * width + x) as usize;
                let color_val = if x < 8 {
                    if y < 8 {
                        50
                    } else {
                        150
                    }
                } else {
                    if y < 8 {
                        100
                    } else {
                        200
                    }
                };
                blocky_pixels[idx] = color_val;
                blocky_pixels[idx + 1] = color_val;
                blocky_pixels[idx + 2] = color_val;
                blocky_pixels[idx + 3] = 255;
            }
        }
        let blocky_img = RasterImage {
            width,
            height,
            pixels: blocky_pixels,
        };

        let q_before = estimate_jpeg_quality(&blocky_img);
        let deblocked = deblock(&blocky_img);
        let q_after = estimate_jpeg_quality(&deblocked);

        assert!(
            q_after > q_before,
            "Deblocking must improve (increase) estimated quality (before: {}, after: {})",
            q_before,
            q_after
        );

        // preprocess conditional checks
        // 1. was_jpeg = false -> no-op on deblock. Since we use Mode::PixelArt, bilateral is also skipped.
        // Thus, the result should be identical to input.
        let processed_no_jpeg = preprocess(blocky_img.clone(), &Mode::PixelArt, false);
        assert_eq!(
            processed_no_jpeg.pixels, blocky_img.pixels,
            "Should be no-op when was_jpeg is false and mode is PixelArt"
        );

        // 2. was_jpeg = true -> deblock applied, so image changes.
        let processed_was_jpeg = preprocess(blocky_img.clone(), &Mode::PixelArt, true);
        assert_ne!(
            processed_was_jpeg.pixels, blocky_img.pixels,
            "Should apply deblock when was_jpeg is true and quality < 90"
        );
    }

    #[test]
    fn test_downscale_large_photo_all_cases() {
        // 1. A 2048x1024 photo-mode image downscales to 1024x512 (aspect preserved, exact long-edge target)
        let pixels_2048_1024 = vec![0u8; 2048 * 1024 * 4];
        let img_2048_1024 = RasterImage {
            width: 2048,
            height: 1024,
            pixels: pixels_2048_1024,
        };
        let res = downscale_large_photo(img_2048_1024, &Mode::Photo);
        assert_eq!(res.width, 1024);
        assert_eq!(res.height, 512);

        // 2. A 2048x2048 image downscales to 1024x1024
        let pixels_2048_2048 = vec![0u8; 2048 * 2048 * 4];
        let img_2048_2048 = RasterImage {
            width: 2048,
            height: 2048,
            pixels: pixels_2048_2048,
        };
        let res = downscale_large_photo(img_2048_2048, &Mode::Photo);
        assert_eq!(res.width, 1024);
        assert_eq!(res.height, 1024);

        // 3. A photo-mode image at exactly 1600 (the threshold) is NOT downscaled
        let pixels_1600_1000 = vec![0u8; 1600 * 1000 * 4];
        let img_1600_1000 = RasterImage {
            width: 1600,
            height: 1000,
            pixels: pixels_1600_1000.clone(),
        };
        let res = downscale_large_photo(img_1600_1000, &Mode::Photo);
        assert_eq!(res.width, 1600);
        assert_eq!(res.height, 1000);
        assert_eq!(res.pixels.len(), pixels_1600_1000.len());

        // 4. An icon-mode image at 2048x2048 is NOT downscaled
        let pixels_icon = vec![0u8; 2048 * 2048 * 4];
        let img_icon = RasterImage {
            width: 2048,
            height: 2048,
            pixels: pixels_icon.clone(),
        };
        let res = downscale_large_photo(img_icon, &Mode::Icon);
        assert_eq!(res.width, 2048);
        assert_eq!(res.height, 2048);
        assert_eq!(res.pixels.len(), pixels_icon.len());

        // 5. The function never panics on a 1x1 image
        let pixels_1x1 = vec![0u8; 4];
        let img_1x1 = RasterImage {
            width: 1,
            height: 1,
            pixels: pixels_1x1.clone(),
        };
        let res = downscale_large_photo(img_1x1, &Mode::Photo);
        assert_eq!(res.width, 1);
        assert_eq!(res.height, 1);
        assert_eq!(res.pixels.len(), pixels_1x1.len());
    }
}
