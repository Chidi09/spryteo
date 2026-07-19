use std::collections::VecDeque;

/// Computes the Euclidean distance from each ink pixel to the nearest background pixel.
/// If there are no background pixels in the image, treats the outer bounds of the image as background.
pub fn compute_distance_transform(ink_mask: &[bool], width: u32, height: u32) -> Vec<f64> {
    let w = width as usize;
    let h = height as usize;
    let mut dist = vec![f64::INFINITY; w * h];
    let mut nearest = vec![None; w * h];
    let mut queue = VecDeque::new();

    // Initialize with background pixels in the image
    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            if !ink_mask[idx] {
                dist[idx] = 0.0;
                nearest[idx] = Some((x as i32, y as i32));
                queue.push_back((x as i32, y as i32));
            }
        }
    }

    // Fallback if there are no background pixels: treat image boundaries as background
    if queue.is_empty() {
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                let dx1 = x + 1;
                let dx2 = w - x;
                let dy1 = y + 1;
                let dy2 = h - y;
                dist[idx] = dx1.min(dx2).min(dy1).min(dy2) as f64;
            }
        }
        return dist;
    }

    // BFS propagation to compute Euclidean distances
    let dx = [-1, 0, 1, -1, 1, -1, 0, 1];
    let dy = [-1, -1, -1, 0, 0, 1, 1, 1];

    while let Some((cx, cy)) = queue.pop_front() {
        let c_idx = (cy * w as i32 + cx) as usize;
        let Some((bx, by)) = nearest[c_idx] else {
            continue;
        };

        for i in 0..8 {
            let nx = cx + dx[i];
            let ny = cy + dy[i];

            if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                let n_idx = (ny * w as i32 + nx) as usize;
                if ink_mask[n_idx] {
                    let rx = nx as f64 - bx as f64;
                    let ry = ny as f64 - by as f64;
                    let d = (rx * rx + ry * ry).sqrt();
                    if d < dist[n_idx] {
                        dist[n_idx] = d;
                        nearest[n_idx] = Some((bx, by));
                        queue.push_back((nx, ny));
                    }
                }
            }
        }
    }

    dist
}

/// Reduces the binary ink mask to a 1-pixel-wide skeleton using the Zhang-Suen algorithm.
pub fn zhang_suen_thinning(ink_mask: &[bool], width: u32, height: u32) -> Vec<bool> {
    let w = width as i32;
    let h = height as i32;
    let mut skeleton = ink_mask.to_vec();

    let get_val = |x: i32, y: i32, grid: &[bool]| -> u8 {
        if x < 0 || x >= w || y < 0 || y >= h {
            0
        } else {
            if grid[(y * w + x) as usize] {
                1
            } else {
                0
            }
        }
    };

    loop {
        let mut changed = false;
        let mut to_delete = Vec::new();

        // Sub-iteration 1
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                if !skeleton[idx] {
                    continue;
                }

                // Clockwise neighbors: p2(N), p3(NE), p4(E), p5(SE), p6(S), p7(SW), p8(W), p9(NW)
                let p2 = get_val(x, y - 1, &skeleton);
                let p3 = get_val(x + 1, y - 1, &skeleton);
                let p4 = get_val(x + 1, y, &skeleton);
                let p5 = get_val(x + 1, y + 1, &skeleton);
                let p6 = get_val(x, y + 1, &skeleton);
                let p7 = get_val(x - 1, y + 1, &skeleton);
                let p8 = get_val(x - 1, y, &skeleton);
                let p9 = get_val(x - 1, y - 1, &skeleton);

                let b = p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9;

                // Number of 0 -> 1 transitions in sequence p2..p9, p2
                let neighbors = [p2, p3, p4, p5, p6, p7, p8, p9, p2];
                let mut a = 0;
                for i in 0..8 {
                    if neighbors[i] == 0 && neighbors[i + 1] == 1 {
                        a += 1;
                    }
                }

                if (2..=6).contains(&b) && a == 1 && p2 * p4 * p6 == 0 && p4 * p6 * p8 == 0 {
                    to_delete.push(idx);
                }
            }
        }

        if !to_delete.is_empty() {
            changed = true;
            for &idx in &to_delete {
                skeleton[idx] = false;
            }
        }

        to_delete.clear();

        // Sub-iteration 2
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                if !skeleton[idx] {
                    continue;
                }

                let p2 = get_val(x, y - 1, &skeleton);
                let p3 = get_val(x + 1, y - 1, &skeleton);
                let p4 = get_val(x + 1, y, &skeleton);
                let p5 = get_val(x + 1, y + 1, &skeleton);
                let p6 = get_val(x, y + 1, &skeleton);
                let p7 = get_val(x - 1, y + 1, &skeleton);
                let p8 = get_val(x - 1, y, &skeleton);
                let p9 = get_val(x - 1, y - 1, &skeleton);

                let b = p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9;

                let neighbors = [p2, p3, p4, p5, p6, p7, p8, p9, p2];
                let mut a = 0;
                for i in 0..8 {
                    if neighbors[i] == 0 && neighbors[i + 1] == 1 {
                        a += 1;
                    }
                }

                if (2..=6).contains(&b) && a == 1 && p2 * p4 * p8 == 0 && p2 * p6 * p8 == 0 {
                    to_delete.push(idx);
                }
            }
        }

        if !to_delete.is_empty() {
            changed = true;
            for &idx in &to_delete {
                skeleton[idx] = false;
            }
        }

        if !changed {
            break;
        }
    }

    skeleton
}
