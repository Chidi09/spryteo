/// Prunes dead-end spurs from the skeleton.
///
/// A spur is a pixel chain from a degree-1 node to the nearest junction or other endpoint
/// whose length (in pixels) is less than `1.5 * local_width` at the spur's base (junction/endpoint).
/// Since `local_width` is `2 * distance_transform_value`, the threshold is `3.0 * dist[base]`.
/// Pruning is run iteratively until convergence.
pub fn prune_spurs(skeleton: &mut [bool], dist: &[f64], width: u32, height: u32) {
    let w = width as i32;
    let h = height as i32;

    let get_neighbors = |x: i32, y: i32, skel: &[bool]| -> Vec<(i32, i32)> {
        let mut raw_neighbors = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && nx < w && ny >= 0 && ny < h {
                    let n_idx = (ny * w + nx) as usize;
                    if skel[n_idx] {
                        raw_neighbors.push((nx, ny));
                    }
                }
            }
        }

        let mut neighbors = Vec::new();
        for &(nx, ny) in &raw_neighbors {
            let is_diagonal = nx != x && ny != y;
            if is_diagonal {
                let h_helper_idx = (y * w + nx) as usize;
                let v_helper_idx = (ny * w + x) as usize;
                if skel[h_helper_idx] || skel[v_helper_idx] {
                    continue;
                }
            }
            neighbors.push((nx, ny));
        }
        neighbors
    };

    loop {
        let mut deleted_any = false;
        let mut to_delete = vec![false; (width * height) as usize];

        // Find all endpoints in the current skeleton
        let mut endpoints = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) as usize;
                if skeleton[idx] {
                    let nb = get_neighbors(x, y, skeleton);
                    if nb.len() == 1 {
                        endpoints.push((x, y));
                    }
                }
            }
        }

        // Trace paths starting from each endpoint
        for &(sx, sy) in &endpoints {
            let start_idx = (sy * w + sx) as usize;
            if to_delete[start_idx] {
                continue;
            }

            let mut path = vec![(sx, sy)];
            let mut curr = (sx, sy);
            let mut prev = None;

            let base = loop {
                let (cx, cy) = curr;
                let nb = get_neighbors(cx, cy, skeleton);

                if path.len() == 1 {
                    // Start of the path
                    if !nb.is_empty() {
                        let next = nb[0];
                        path.push(next);
                        prev = Some(curr);
                        curr = next;
                    } else {
                        // Isolated 1-pixel dot
                        break curr;
                    }
                } else {
                    if nb.len() == 2 {
                        let next = if nb[0] == prev.unwrap() { nb[1] } else { nb[0] };
                        if path.contains(&next) {
                            // Cycle detected
                            break curr;
                        }
                        path.push(next);
                        prev = Some(curr);
                        curr = next;
                    } else {
                        // Reached a junction or other endpoint
                        break curr;
                    }
                }
            };

            let base_idx = (base.1 * w + base.0) as usize;
            let dist_base = dist[base_idx];
            let threshold = 3.0 * dist_base;

            let base_degree = get_neighbors(base.0, base.1, skeleton).len();
            let (is_junction, length) = if base_degree >= 3 {
                // If base is a junction, the spur does not include base itself.
                (true, path.len() - 1)
            } else {
                // If base is another endpoint, the chain includes both endpoints.
                (false, path.len())
            };
            if (length as f64) < threshold {
                let delete_len = if is_junction {
                    path.len() - 1
                } else {
                    path.len()
                };
                for &(px, py) in path.iter().take(delete_len) {
                    to_delete[(py * w + px) as usize] = true;
                }
            }
        }

        for idx in 0..skeleton.len() {
            if to_delete[idx] {
                skeleton[idx] = false;
                deleted_any = true;
            }
        }

        if !deleted_any {
            break;
        }
    }
}
