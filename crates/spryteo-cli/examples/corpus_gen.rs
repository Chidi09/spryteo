use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::fs::{create_dir_all, File};
use std::path::Path;

fn save_png(img: RgbaImage, category: &str, name: &str) {
    let dir = Path::new("testdata/corpus").join(category);
    create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.png", name));
    let mut file = File::create(&path).unwrap();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut file, ImageFormat::Png)
        .unwrap();
    println!("Generated: {}", path.display());
}

fn downscale_4x(src: &RgbaImage) -> RgbaImage {
    let width = src.width() / 4;
    let height = src.height() / 4;
    let mut dst = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut a_sum = 0u32;
            for dy in 0..4 {
                for dx in 0..4 {
                    let pixel = src.get_pixel(4 * x + dx, 4 * y + dy);
                    r_sum += pixel[0] as u32;
                    g_sum += pixel[1] as u32;
                    b_sum += pixel[2] as u32;
                    a_sum += pixel[3] as u32;
                }
            }
            dst.put_pixel(
                x,
                y,
                Rgba([
                    (r_sum / 16) as u8,
                    (g_sum / 16) as u8,
                    (b_sum / 16) as u8,
                    (a_sum / 16) as u8,
                ]),
            );
        }
    }
    dst
}

fn point_in_polygon(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        if ((poly[i].1 > y) != (poly[j].1 > y))
            && (x < (poly[j].0 - poly[i].0) * (y - poly[i].1) / (poly[j].1 - poly[i].1) + poly[i].0)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn dist_to_segment(x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return ((x - x1) * (x - x1) + (y - y1) * (y - y1)).sqrt();
    }
    let t = (((x - x1) * dx + (y - y1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    ((x - proj_x) * (x - proj_x) + (y - proj_y) * (y - proj_y)).sqrt()
}

fn dist_to_polyline(x: f32, y: f32, points: &[(f32, f32)]) -> f32 {
    let mut min_dist = f32::MAX;
    for i in 0..points.len() - 1 {
        let d = dist_to_segment(
            x,
            y,
            points[i].0,
            points[i].1,
            points[i + 1].0,
            points[i + 1].1,
        );
        if d < min_dist {
            min_dist = d;
        }
    }
    min_dist
}

fn dist_to_arc(x: f32, y: f32, cx: f32, cy: f32, r: f32, start_angle: f32, end_angle: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    let dist_to_circle = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let normalize = |a: f32| -> f32 {
        let mut val = a % (2.0 * std::f32::consts::PI);
        if val < 0.0 {
            val += 2.0 * std::f32::consts::PI;
        }
        val
    };
    let angle_norm = normalize(angle);
    let start_norm = normalize(start_angle);
    let end_norm = normalize(end_angle);

    let in_arc = if start_norm <= end_norm {
        angle_norm >= start_norm && angle_norm <= end_norm
    } else {
        angle_norm >= start_norm || angle_norm <= end_norm
    };

    if in_arc {
        (dist_to_circle - r).abs()
    } else {
        let ep1_x = cx + r * start_angle.cos();
        let ep1_y = cy + r * start_angle.sin();
        let ep2_x = cx + r * end_angle.cos();
        let ep2_y = cy + r * end_angle.sin();
        let d1 = ((x - ep1_x) * (x - ep1_x) + (y - ep1_y) * (y - ep1_y)).sqrt();
        let d2 = ((x - ep2_x) * (x - ep2_x) + (y - ep2_y) * (y - ep2_y)).sqrt();
        d1.min(d2)
    }
}

// LCG random generator with fixed seed
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.state >> 16) as u32
    }
}

fn generate_icons() {
    let red = Rgba([255, 0, 0, 255]);
    let blue = Rgba([0, 0, 255, 255]);
    let green = Rgba([0, 255, 0, 255]);
    let white = Rgba([255, 255, 255, 255]);
    let light_grey = Rgba([240, 240, 240, 255]);

    // Helper for rendering icons with supersampling
    let render_icon = |size: u32, f: &dyn Fn(f32, f32) -> Rgba<u8>| -> RgbaImage {
        let canvas_size = size * 4;
        let mut canvas = RgbaImage::new(canvas_size, canvas_size);
        for y in 0..canvas_size {
            for x in 0..canvas_size {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                canvas.put_pixel(x, y, f(px, py));
            }
        }
        downscale_4x(&canvas)
    };

    // 1. circle_48
    let circle_48 = render_icon(48, &|x, y| {
        let dx = x - 96.0;
        let dy = y - 96.0;
        if dx * dx + dy * dy <= 80.0 * 80.0 {
            red
        } else {
            white
        }
    });
    save_png(circle_48, "icons", "circle_48");

    // 2. circle_256
    let circle_256 = render_icon(256, &|x, y| {
        let dx = x - 512.0;
        let dy = y - 512.0;
        if dx * dx + dy * dy <= 440.0 * 440.0 {
            red
        } else {
            white
        }
    });
    save_png(circle_256, "icons", "circle_256");

    // 3. ring_48
    let mut ring_48 = RgbaImage::new(48, 48);
    for y in 0..48 {
        for x in 0..48 {
            let in_outer = (5..42).contains(&x) && (5..42).contains(&y);
            let in_inner = (17..30).contains(&x) && (17..30).contains(&y);
            let col = if in_inner {
                light_grey
            } else if in_outer {
                blue
            } else {
                white
            };
            ring_48.put_pixel(x, y, col);
        }
    }
    save_png(ring_48, "icons", "ring_48");

    // 4. ring_256
    let mut ring_256 = RgbaImage::new(256, 256);
    for y in 0..256 {
        for x in 0..256 {
            let dx = x as f32 + 0.5 - 127.5;
            let dy = y as f32 + 0.5 - 127.5;
            let d2 = dx * dx + dy * dy;
            let col = if (55.0 * 55.0..=110.0 * 110.0).contains(&d2) {
                blue
            } else if d2 < 55.0 * 55.0 {
                light_grey
            } else {
                white
            };
            ring_256.put_pixel(x, y, col);
        }
    }
    save_png(ring_256, "icons", "ring_256");

    // 5. rounded_rect_48
    let rounded_rect_48 = render_icon(48, &|x, y| {
        let cx = 96.0;
        let cy = 96.0;
        let w = 160.0;
        let h = 120.0;
        let r = 32.0;
        let dx = (x - cx).abs() - (w / 2.0 - r);
        let dy = (y - cy).abs() - (h / 2.0 - r);
        let inside = if dx > 0.0 && dy > 0.0 {
            dx * dx + dy * dy <= r * r
        } else {
            (x - cx).abs() <= w / 2.0 && (y - cy).abs() <= h / 2.0
        };
        if inside {
            green
        } else {
            white
        }
    });
    save_png(rounded_rect_48, "icons", "rounded_rect_48");

    // 6. rounded_rect_256
    let rounded_rect_256 = render_icon(256, &|x, y| {
        let cx = 512.0;
        let cy = 512.0;
        let w = 880.0;
        let h = 640.0;
        let r = 160.0;
        let dx = (x - cx).abs() - (w / 2.0 - r);
        let dy = (y - cy).abs() - (h / 2.0 - r);
        let inside = if dx > 0.0 && dy > 0.0 {
            dx * dx + dy * dy <= r * r
        } else {
            (x - cx).abs() <= w / 2.0 && (y - cy).abs() <= h / 2.0
        };
        if inside {
            green
        } else {
            white
        }
    });
    save_png(rounded_rect_256, "icons", "rounded_rect_256");

    // 7. star_256
    let mut star_poly = Vec::new();
    for i in 0..10 {
        let angle = (i as f32) * std::f32::consts::PI / 5.0 - std::f32::consts::PI / 2.0;
        let r = if i % 2 == 0 { 440.0 } else { 180.0 };
        star_poly.push((512.0 + r * angle.cos(), 512.0 + r * angle.sin()));
    }
    let star_256 = render_icon(256, &|x, y| {
        if point_in_polygon(x, y, &star_poly) {
            Rgba([255, 215, 0, 255]) // Gold
        } else {
            white
        }
    });
    save_png(star_256, "icons", "star_256");

    // 8. chevron_48
    let chevron_poly = vec![
        (15.0, 15.0),
        (96.0, 96.0),
        (15.0, 177.0),
        (90.0, 177.0),
        (171.0, 96.0),
        (90.0, 15.0),
    ];
    let chevron_48 = render_icon(48, &|x, y| {
        if point_in_polygon(x, y, &chevron_poly) {
            Rgba([255, 128, 0, 255]) // Orange
        } else {
            white
        }
    });
    save_png(chevron_48, "icons", "chevron_48");

    // 9. overlapping_256
    let mut overlapping_256 = RgbaImage::new(256, 256);
    for y in 0..256 {
        for x in 0..256 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let in_square = (50.0..=175.0).contains(&px) && (50.0..=175.0).contains(&py);
            let dx = px - 175.0;
            let dy = py - 175.0;
            let in_circle = dx * dx + dy * dy <= 75.0 * 75.0;

            let col = if in_circle {
                Rgba([255, 255, 0, 255]) // Yellow
            } else if in_square {
                Rgba([0, 0, 255, 255]) // Blue
            } else {
                white
            };
            overlapping_256.put_pixel(x, y, col);
        }
    }
    save_png(overlapping_256, "icons", "overlapping_256");

    // 10. traffic_light_256
    let traffic_light_256 = render_icon(256, &|x, y| {
        let hcx = 512.0;
        let hcy = 512.0;
        let hw = 360.0;
        let hh = 880.0;
        let hr = 80.0;
        let hdx = (x - hcx).abs() - (hw / 2.0 - hr);
        let hdy = (y - hcy).abs() - (hh / 2.0 - hr);
        let in_housing = if hdx > 0.0 && hdy > 0.0 {
            hdx * hdx + hdy * hdy <= hr * hr
        } else {
            (x - hcx).abs() <= hw / 2.0 && (y - hcy).abs() <= hh / 2.0
        };

        let r_in = (x - 512.0) * (x - 512.0) + (y - 280.0) * (y - 280.0) <= 100.0 * 100.0;
        let y_in = (x - 512.0) * (x - 512.0) + (y - 512.0) * (y - 512.0) <= 100.0 * 100.0;
        let g_in = (x - 512.0) * (x - 512.0) + (y - 744.0) * (y - 744.0) <= 100.0 * 100.0;

        if r_in {
            Rgba([255, 0, 0, 255])
        } else if y_in {
            Rgba([255, 255, 0, 255])
        } else if g_in {
            Rgba([0, 255, 0, 255])
        } else if in_housing {
            Rgba([64, 64, 64, 255])
        } else {
            white
        }
    });
    save_png(traffic_light_256, "icons", "traffic_light_256");
}

#[allow(clippy::needless_range_loop)]
fn generate_pixel_art() {
    let black = Rgba([0, 0, 0, 255]);
    let white = Rgba([255, 255, 255, 255]);

    // 1. checkerboard_16
    let mut check_16 = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            check_16.put_pixel(x, y, white);
        }
    }
    for dy in 0..3 {
        for dx in 0..3 {
            check_16.put_pixel(2 + dx, 2 + dy, black);
            check_16.put_pixel(11 + dx, 2 + dy, black);
            check_16.put_pixel(2 + dx, 11 + dy, black);
            check_16.put_pixel(11 + dx, 11 + dy, black);
        }
    }
    save_png(check_16, "pixel-art", "checkerboard_16");

    // 2. sprite_32
    let mut sprite_32 = RgbaImage::new(32, 32);
    let alien = [
        [0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0],
        [0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0],
        [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
        [0, 1, 1, 2, 1, 1, 1, 1, 2, 1, 1, 0],
        [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        [1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1],
        [1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1],
        [0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0],
    ];
    let sprite_red = Rgba([220, 20, 60, 255]);
    let sprite_white = Rgba([255, 255, 255, 255]);
    let sprite_bg = Rgba([30, 30, 30, 255]);
    for y in 0..32 {
        for x in 0..32 {
            sprite_32.put_pixel(x, y, sprite_bg);
        }
    }
    for row in 0..8 {
        for col in 0..12 {
            let val = alien[row][col];
            let color = match val {
                1 => sprite_red,
                2 => sprite_white,
                _ => sprite_bg,
            };
            if val > 0 {
                for dy in 0..2 {
                    for dx in 0..2 {
                        sprite_32.put_pixel(
                            4 + col as u32 * 2 + dx,
                            8 + row as u32 * 2 + dy,
                            color,
                        );
                    }
                }
            }
        }
    }
    save_png(sprite_32, "pixel-art", "sprite_32");

    // 3. dither_64
    let mut dither_64 = RgbaImage::new(64, 64);
    let purple = Rgba([128, 0, 128, 255]);
    let cyan = Rgba([0, 255, 255, 255]);
    for y in 0..64 {
        let stripe = y / 8;
        let col = if stripe % 2 == 0 { purple } else { cyan };
        for x in 0..64 {
            dither_64.put_pixel(x, y, col);
        }
    }
    save_png(dither_64, "pixel-art", "dither_64");

    // 4. brick_32
    let mut brick_32 = RgbaImage::new(32, 32);
    let brick_color = Rgba([180, 60, 40, 255]);
    let mortar_color = Rgba([160, 160, 160, 255]);
    for y in 0..32 {
        let brick_row = y / 8;
        let is_odd_row = brick_row % 2 != 0;
        for x in 0..32 {
            let is_mortar_y = y % 8 == 0 || y % 8 == 7;
            let is_mortar_x = if is_odd_row {
                (x + 8) % 16 == 0 || (x + 8) % 16 == 15
            } else {
                x % 16 == 0 || x % 16 == 15
            };
            if is_mortar_y || is_mortar_x {
                brick_32.put_pixel(x, y, mortar_color);
            } else {
                brick_32.put_pixel(x, y, brick_color);
            }
        }
    }
    save_png(brick_32, "pixel-art", "brick_32");

    // 5. star_32
    let mut star_32 = RgbaImage::new(32, 32);
    let yellow = Rgba([255, 255, 0, 255]);
    let dark_blue = Rgba([10, 10, 80, 255]);
    for y in 0..32 {
        for x in 0..32 {
            star_32.put_pixel(x, y, dark_blue);
        }
    }
    // Draw central 4x4 square
    for y in 14..18 {
        for x in 14..18 {
            star_32.put_pixel(x, y, yellow);
        }
    }
    // Draw arms (2 pixels wide)
    for y in 8..14 {
        for x in 15..17 {
            star_32.put_pixel(x, y, yellow);
        }
    }
    for y in 18..24 {
        for x in 15..17 {
            star_32.put_pixel(x, y, yellow);
        }
    }
    for y in 15..17 {
        for x in 8..14 {
            star_32.put_pixel(x, y, yellow);
        }
    }
    for y in 15..17 {
        for x in 18..24 {
            star_32.put_pixel(x, y, yellow);
        }
    }
    save_png(star_32, "pixel-art", "star_32");
}

fn generate_line_art() {
    let black = Rgba([0, 0, 0, 255]);
    let white = Rgba([255, 255, 255, 255]);

    let render_line_art = |name: &str, f: &dyn Fn(f32, f32) -> f32, stroke_width_4x: f32| {
        let canvas_size = 1024;
        let mut canvas = RgbaImage::new(canvas_size, canvas_size);
        let half_w = stroke_width_4x / 2.0;
        for y in 0..canvas_size {
            for x in 0..canvas_size {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let dist = f(px, py);
                if dist <= half_w {
                    canvas.put_pixel(x, y, black);
                } else {
                    canvas.put_pixel(x, y, white);
                }
            }
        }
        let downscaled = downscale_4x(&canvas);
        save_png(downscaled, "line-art", name);
    };

    // 1. segments_256
    render_line_art(
        "segments_256",
        &|x, y| {
            let lines = vec![
                (150.0, 150.0, 874.0, 874.0),
                (150.0, 874.0, 874.0, 150.0),
                (150.0, 512.0, 874.0, 512.0),
                (512.0, 150.0, 512.0, 874.0),
            ];
            let mut min_d = f32::MAX;
            for l in lines {
                min_d = min_d.min(dist_to_segment(x, y, l.0, l.1, l.2, l.3));
            }
            min_d
        },
        12.0,
    );

    // 2. arcs_256
    render_line_art(
        "arcs_256",
        &|x, y| {
            let d1 = dist_to_arc(x, y, 512.0, 512.0, 350.0, 0.0, std::f32::consts::PI);
            let d2 = dist_to_arc(
                x,
                y,
                512.0,
                512.0,
                200.0,
                std::f32::consts::PI,
                2.0 * std::f32::consts::PI,
            );
            d1.min(d2)
        },
        10.0,
    );

    // 3. spiral_256
    let mut spiral_pts = Vec::new();
    let steps = 600;
    let max_theta = 4.0 * 2.0 * std::f32::consts::PI;
    for i in 0..=steps {
        let theta = (i as f32 / steps as f32) * max_theta;
        let r = 16.0 * theta;
        spiral_pts.push((512.0 + r * theta.cos(), 512.0 + r * theta.sin()));
    }
    render_line_art(
        "spiral_256",
        &|x, y| dist_to_polyline(x, y, &spiral_pts),
        12.0,
    );

    // 4. zigzag_256
    let mut zigzag_pts = Vec::new();
    for i in 0..9 {
        let px = 100.0 + (i as f32) * 103.0;
        let py = if i % 2 == 0 { 250.0 } else { 774.0 };
        zigzag_pts.push((px, py));
    }
    render_line_art(
        "zigzag_256",
        &|x, y| dist_to_polyline(x, y, &zigzag_pts),
        14.0,
    );

    // 5. squiggle_256
    let mut squiggle_pts = Vec::new();
    let steps = 400;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let px = 150.0 + 724.0 * t;
        let py = 512.0
            + 250.0 * (t * 6.0 * std::f32::consts::PI).sin()
            + 150.0 * (t * 2.0 * std::f32::consts::PI).cos();
        squiggle_pts.push((px, py));
    }
    render_line_art(
        "squiggle_256",
        &|x, y| dist_to_polyline(x, y, &squiggle_pts),
        10.0,
    );
}

/// Fixtures for the centerline (`--stroke`) pipeline.
///
/// These deliberately target the cases the spryteo-stroke unit tests do NOT cover:
/// multiple disjoint components in one image, genuine closed loops, and junctions of
/// degree 3 and 4 (which push `traverse_graph` off the Hierholzer path and into its
/// greedy multi-path fallback). Strokes are kept thin, since centerline tracing is
/// most fragile on thin strokes and that is the regime the icon-sheet work targets.
fn generate_stroke() {
    let black = Rgba([0, 0, 0, 255]);
    let white = Rgba([255, 255, 255, 255]);

    let render_stroke = |name: &str, f: &dyn Fn(f32, f32) -> f32, stroke_width_4x: f32| {
        let canvas_size = 1024;
        let mut canvas = RgbaImage::new(canvas_size, canvas_size);
        let half_w = stroke_width_4x / 2.0;
        for y in 0..canvas_size {
            for x in 0..canvas_size {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                if f(px, py) <= half_w {
                    canvas.put_pixel(x, y, black);
                } else {
                    canvas.put_pixel(x, y, white);
                }
            }
        }
        save_png(downscale_4x(&canvas), "stroke", name);
    };

    // 1. cross_256 -- two crossing lines. The centre is a degree-4 junction, giving 4
    //    odd-degree endpoints, which forces traverse_graph's greedy multi-path fallback.
    render_stroke(
        "cross_256",
        &|x, y| {
            dist_to_segment(x, y, 160.0, 160.0, 864.0, 864.0)
                .min(dist_to_segment(x, y, 160.0, 864.0, 864.0, 160.0))
        },
        10.0,
    );

    // 2. tee_256 -- a single degree-3 junction (3 odd nodes + the junction).
    render_stroke(
        "tee_256",
        &|x, y| {
            dist_to_segment(x, y, 160.0, 300.0, 864.0, 300.0)
                .min(dist_to_segment(x, y, 512.0, 300.0, 512.0, 880.0))
        },
        10.0,
    );

    // 3. loop_256 -- a genuine closed loop with no endpoints at all. Exercises the
    //    "promote a pixel to a node" path in build_stroke_graphs and the fact that
    //    smooth_chain never emits ClosePath.
    render_stroke(
        "loop_256",
        &|x, y| ((x - 512.0).hypot(y - 512.0) - 330.0).abs(),
        10.0,
    );

    // 4. multi_256 -- four disjoint components in one image. This is the icon-sheet
    //    case, and spryteo-stroke has no unit coverage for it at all.
    render_stroke(
        "multi_256",
        &|x, y| {
            let mut d = f32::MAX;
            for (cx, cy) in [(280.0, 280.0), (744.0, 280.0), (280.0, 744.0)] {
                d = d.min(((x - cx).hypot(y - cy) - 130.0).abs());
            }
            // a plain segment as the fourth component
            d.min(dist_to_segment(x, y, 640.0, 640.0, 850.0, 850.0))
        },
        10.0,
    );

    // 5. corner_256 -- an open polyline with sharp corners and two free ends, the
    //    clean Hierholzer case (exactly 2 odd nodes -> one continuous path).
    render_stroke(
        "corner_256",
        &|x, y| {
            dist_to_polyline(
                x,
                y,
                &[
                    (180.0, 840.0),
                    (180.0, 300.0),
                    (512.0, 180.0),
                    (844.0, 300.0),
                    (844.0, 840.0),
                ],
            )
        },
        10.0,
    );
}

fn generate_photos() {
    let size = 256;

    let render_photo = |name: &str, f: &dyn Fn(u32, u32) -> Rgba<u8>| {
        let mut img = RgbaImage::new(size, size);
        for y in 0..size {
            for x in 0..size {
                img.put_pixel(x, y, f(x, y));
            }
        }
        save_png(img, "photos", name);
    };

    // 1. gradient_h_256
    render_photo("gradient_h_256", &|x, _| {
        let r = (x as f32 / 255.0 * 255.0) as u8;
        let b = 255 - r;
        Rgba([r, 0, b, 255])
    });

    // 2. gradient_v_256
    render_photo("gradient_v_256", &|_, y| {
        let g = (y as f32 / 255.0 * 255.0) as u8;
        let r = 255 - g;
        Rgba([r, g, r, 255])
    });

    // 3. gradient_d_256
    render_photo("gradient_d_256", &|x, y| {
        let factor = (x + y) as f32 / 510.0;
        let r = (factor * 255.0) as u8;
        let g = 255;
        let b = 255 - r;
        Rgba([r, g, b, 255])
    });

    // 4. gradient_r_256
    render_photo("gradient_r_256", &|x, y| {
        let dx = x as f32 - 128.0;
        let dy = y as f32 - 128.0;
        let dist = (dx * dx + dy * dy).sqrt() / 128.0;
        let r_val = ((1.0 - dist.min(1.0)) * 255.0) as u8;
        let b_val = 255 - r_val;
        Rgba([r_val, 0, b_val, 255])
    });

    // 5. landscape_256
    render_photo("landscape_256", &|x, y| {
        let sky_r = if y < 150 {
            let t = y as f32 / 150.0;
            (t * 255.0) as u8
        } else {
            255
        };
        let sky_g = if y < 150 {
            let t = y as f32 / 150.0;
            (t * 140.0) as u8
        } else {
            140
        };
        let sky_b = if y < 150 {
            let t = y as f32 / 150.0;
            ((1.0 - t) * 128.0) as u8
        } else {
            0
        };
        let sky_color = Rgba([sky_r, sky_g, sky_b, 255]);

        let sdx = x as f32 - 128.0;
        let sdy = y as f32 - 80.0;
        let in_sun = sdx * sdx + sdy * sdy <= 30.0 * 30.0;

        let in_hill1 = y as f32 >= 160.0;
        let in_hill2 = y as f32 >= 200.0;

        if in_hill2 {
            Rgba([34, 139, 34, 255])
        } else if in_hill1 {
            Rgba([107, 142, 35, 255])
        } else if in_sun {
            Rgba([255, 223, 0, 255])
        } else {
            sky_color
        }
    });

    // 6. gradient_noise_256
    let mut lcg = Lcg::new(42);
    let mut img = RgbaImage::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let r_base = (x as f32 / 255.0 * 255.0) as i16;
            let b_base = 255 - r_base;
            let noise = (lcg.next() % 3) as i16 - 1;
            let r = (r_base + noise).clamp(0, 255) as u8;
            let b = (b_base + noise).clamp(0, 255) as u8;
            img.put_pixel(x, y, Rgba([r, 0, b, 255]));
        }
    }
    save_png(img, "photos", "gradient_noise_256");
}

fn main() {
    println!("Starting procedural corpus generation...");
    generate_icons();
    generate_pixel_art();
    generate_line_art();
    generate_stroke();
    generate_photos();
    println!("Corpus generation finished successfully!");
}
