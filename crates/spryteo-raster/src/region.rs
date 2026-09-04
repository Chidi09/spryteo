use spryteo_core::RasterImage;

pub fn crop(image: &RasterImage, x: u32, y: u32, w: u32, h: u32) -> RasterImage {
    let clamped_w = if x < image.width {
        w.min(image.width - x)
    } else {
        0
    };
    let clamped_h = if y < image.height {
        h.min(image.height - y)
    } else {
        0
    };

    if clamped_w == 0 || clamped_h == 0 {
        return RasterImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
    }

    // usize, not u32: `y * width * 4` overflows a u32 on large images
    // (a 33k-square RGBA sheet is already past u32::MAX).
    let src_stride = image.width as usize * 4;
    let dst_stride = clamped_w as usize * 4;
    let src_start = y as usize * src_stride + x as usize * 4;

    let mut pixels = vec![0u8; dst_stride * clamped_h as usize];

    for row in 0..clamped_h as usize {
        let src_begin = src_start + row * src_stride;
        let src_end = src_begin + dst_stride;
        let dst_begin = row * dst_stride;
        pixels[dst_begin..dst_begin + dst_stride]
            .copy_from_slice(&image.pixels[src_begin..src_end]);
    }

    RasterImage {
        width: clamped_w,
        height: clamped_h,
        pixels,
    }
}

pub fn upscale_bicubic(image: &RasterImage, factor: u32) -> RasterImage {
    let factor = factor.clamp(1, 8);

    if image.width == 0 || image.height == 0 {
        return RasterImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
    }

    if factor == 1 {
        return RasterImage {
            width: image.width,
            height: image.height,
            pixels: image.pixels.clone(),
        };
    }

    // RasterImage dimensions are u32, so an upscale that cannot be represented
    // is not a clamp candidate -- it has no valid answer. Return 0x0 rather than
    // wrapping the multiply and then indexing past the buffer.
    let (new_w, new_h) = match (
        image.width.checked_mul(factor),
        image.height.checked_mul(factor),
    ) {
        (Some(w), Some(h)) => (w, h),
        _ => {
            return RasterImage {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            }
        }
    };
    let factor_f = factor as f64;

    let mut pixels = vec![0u8; new_w as usize * new_h as usize * 4];

    let src_w = image.width as i32;
    let src_h = image.height as i32;

    for oy in 0..new_h {
        for ox in 0..new_w {
            let src_x = (ox as f64 + 0.5) / factor_f - 0.5;
            let src_y = (oy as f64 + 0.5) / factor_f - 0.5;

            let ix = src_x.floor() as i32;
            let iy = src_y.floor() as i32;

            let mut wx = [0.0f64; 4];
            let mut wy = [0.0f64; 4];
            for i in 0i32..4 {
                wx[i as usize] = catmull_rom_weight(((ix - 1 + i) as f64 - src_x).abs());
                wy[i as usize] = catmull_rom_weight(((iy - 1 + i) as f64 - src_y).abs());
            }

            let mut sum_r = 0.0f64;
            let mut sum_g = 0.0f64;
            let mut sum_b = 0.0f64;
            let mut sum_a = 0.0f64;
            let mut sum_weights = 0.0f64;

            for j in 0i32..4 {
                // Row offset is in BYTES, so it carries the 4-byte pixel stride
                // just as the column offset below does. Without the * 4 every
                // vertical tap reads a quarter of the way into the wrong row.
                let py = (iy - 1 + j).clamp(0, src_h - 1) as usize * image.width as usize * 4;
                for i in 0i32..4 {
                    let px = ((ix - 1 + i).clamp(0, src_w - 1) as usize) * 4;
                    let weight = wy[j as usize] * wx[i as usize];
                    sum_weights += weight;

                    let idx = py + px;
                    let a_val = image.pixels[idx + 3] as f64;
                    sum_r += weight * (image.pixels[idx] as f64 * a_val / 255.0);
                    sum_g += weight * (image.pixels[idx + 1] as f64 * a_val / 255.0);
                    sum_b += weight * (image.pixels[idx + 2] as f64 * a_val / 255.0);
                    sum_a += weight * a_val;
                }
            }

            let inv_weights = if sum_weights == 0.0 {
                0.0
            } else {
                1.0 / sum_weights
            };
            let mut out_a = sum_a * inv_weights;
            let mut out_r = sum_r * inv_weights;
            let mut out_g = sum_g * inv_weights;
            let mut out_b = sum_b * inv_weights;

            if out_a <= 0.0 {
                out_r = 0.0;
                out_g = 0.0;
                out_b = 0.0;
                out_a = 0.0;
            } else {
                let inv_a = 255.0 / out_a;
                out_r *= inv_a;
                out_g *= inv_a;
                out_b *= inv_a;
            }

            let dst_idx = (oy as usize * new_w as usize + ox as usize) * 4;
            pixels[dst_idx] = out_r.round().clamp(0.0, 255.0) as u8;
            pixels[dst_idx + 1] = out_g.round().clamp(0.0, 255.0) as u8;
            pixels[dst_idx + 2] = out_b.round().clamp(0.0, 255.0) as u8;
            pixels[dst_idx + 3] = out_a.round().clamp(0.0, 255.0) as u8;
        }
    }

    RasterImage {
        width: new_w,
        height: new_h,
        pixels,
    }
}

fn catmull_rom_weight(t: f64) -> f64 {
    if t <= 1.0 {
        (1.5 * t - 2.5) * t * t + 1.0
    } else if t < 2.0 {
        ((-0.5 * t + 2.5) * t - 4.0) * t + 2.0
    } else {
        0.0
    }
}
#[cfg(test)]
#[path = "region_tests.rs"]
mod tests;
