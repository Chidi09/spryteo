use crate::chroma::Mask;

/// An axis-aligned bounding box. `x2`/`y2` are EXCLUSIVE, matching Band::end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bbox {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl Bbox {
    pub fn width(&self) -> u32 {
        self.x2.saturating_sub(self.x1)
    }

    pub fn height(&self) -> u32 {
        self.y2.saturating_sub(self.y1)
    }

    pub fn area(&self) -> u64 {
        (self.width() as u64) * (self.height() as u64)
    }

    pub fn center(&self) -> (f32, f32) {
        (
            (self.x1 as f32 + self.x2 as f32) / 2.0,
            (self.y1 as f32 + self.y2 as f32) / 2.0,
        )
    }

    /// True when the two boxes share any interior pixel.
    pub fn intersects(&self, other: &Bbox) -> bool {
        self.x1 < other.x2 && other.x1 < self.x2 && self.y1 < other.y2 && other.y1 < self.y2
    }

    /// Area of overlap, 0 when disjoint.
    pub fn intersection_area(&self, other: &Bbox) -> u64 {
        let ix1 = self.x1.max(other.x1);
        let ix2 = self.x2.min(other.x2);
        let iy1 = self.y1.max(other.y1);
        let iy2 = self.y2.min(other.y2);
        if ix1 < ix2 && iy1 < iy2 {
            ((ix2 - ix1) as u64) * ((iy2 - iy1) as u64)
        } else {
            0
        }
    }

    /// Smallest box containing both.
    pub fn union(&self, other: &Bbox) -> Bbox {
        Bbox {
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
            x2: self.x2.max(other.x2),
            y2: self.y2.max(other.y2),
        }
    }

    pub fn contains_point(&self, x: u32, y: u32) -> bool {
        x >= self.x1 && x < self.x2 && y >= self.y1 && y < self.y2
    }
}

#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Morphological dilation radius (Chebyshev) applied before labelling, so
    /// the several disconnected strokes of one icon merge into one cluster.
    pub dilate: u32, // default 3
    /// Clusters with fewer than this many ORIGINAL (undilated) ink pixels are
    /// dropped as noise.
    pub min_pixels: u32, // default 12
}

impl Default for ClusterConfig {
    fn default() -> Self {
        ClusterConfig {
            dilate: 3,
            min_pixels: 12,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cluster {
    pub bbox: Bbox,
    /// Count of ORIGINAL ink pixels (not dilated ones) inside this cluster.
    pub pixels: u32,
}

/// Chebyshev (8-connected square) dilation by `radius`. radius 0 returns a clone.
pub fn dilate(mask: &Mask, radius: u32) -> Mask {
    if radius == 0 || mask.width == 0 || mask.height == 0 {
        return mask.clone();
    }

    let w = mask.width as usize;
    let h = mask.height as usize;
    let r = radius as usize;

    // Horizontal pass
    let mut buf1 = vec![false; w * h];
    for y in 0..h {
        let row_offset = y * w;
        let mut active_count = 0u32;
        let init_end = r.min(w - 1);
        for i in 0..=init_end {
            if mask.bits[row_offset + i] {
                active_count += 1;
            }
        }
        buf1[row_offset] = active_count > 0;

        for x in 1..w {
            let right = x + r;
            if right < w && mask.bits[row_offset + right] {
                active_count += 1;
            }
            if x > r {
                let left = x - r - 1;
                if mask.bits[row_offset + left] {
                    active_count -= 1;
                }
            }
            buf1[row_offset + x] = active_count > 0;
        }
    }

    // Vertical pass
    let mut buf2 = vec![false; w * h];
    for x in 0..w {
        let mut active_count = 0u32;
        let init_end = r.min(h - 1);
        for i in 0..=init_end {
            if buf1[i * w + x] {
                active_count += 1;
            }
        }
        buf2[x] = active_count > 0;

        for y in 1..h {
            let right = y + r;
            if right < h && buf1[right * w + x] {
                active_count += 1;
            }
            if y > r {
                let left = y - r - 1;
                if buf1[left * w + x] {
                    active_count -= 1;
                }
            }
            buf2[y * w + x] = active_count > 0;
        }
    }

    Mask {
        width: mask.width,
        height: mask.height,
        bits: buf2,
    }
}

/// 8-connected component labelling over the dilated mask, with pixel counts
/// and bboxes measured against the ORIGINAL mask. Returned in a deterministic
/// order: sorted by (bbox.y1, bbox.x1).
pub fn find_clusters(mask: &Mask, cfg: &ClusterConfig) -> Vec<Cluster> {
    if mask.width == 0 || mask.height == 0 {
        return Vec::new();
    }

    let dilated = dilate(mask, cfg.dilate);
    let w = mask.width as usize;
    let h = mask.height as usize;
    let total = w * h;

    let mut visited = vec![false; total];
    let mut clusters = Vec::new();
    let mut stack = Vec::new();

    for y in 0..mask.height {
        for x in 0..mask.width {
            let idx = (y as usize) * w + (x as usize);
            if !dilated.bits[idx] || visited[idx] {
                continue;
            }

            visited[idx] = true;
            stack.push((x, y));

            let mut orig_pixels = 0u32;
            let mut min_x = u32::MAX;
            let mut min_y = u32::MAX;
            let mut max_x = 0u32;
            let mut max_y = 0u32;

            while let Some((px, py)) = stack.pop() {
                if mask.get(px, py) {
                    orig_pixels += 1;
                    min_x = min_x.min(px);
                    min_y = min_y.min(py);
                    max_x = max_x.max(px);
                    max_y = max_y.max(py);
                }

                // 8-connected neighbors
                let pxi = px as i32;
                let pyi = py as i32;

                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = pxi + dx;
                        let ny = pyi + dy;
                        if nx >= 0 && nx < mask.width as i32 && ny >= 0 && ny < mask.height as i32 {
                            let nux = nx as u32;
                            let nuy = ny as u32;
                            let nidx = (nuy as usize) * w + (nux as usize);
                            if dilated.bits[nidx] && !visited[nidx] {
                                visited[nidx] = true;
                                stack.push((nux, nuy));
                            }
                        }
                    }
                }
            }

            if orig_pixels >= cfg.min_pixels {
                let bbox = Bbox {
                    x1: min_x,
                    y1: min_y,
                    x2: max_x + 1,
                    y2: max_y + 1,
                };
                clusters.push(Cluster {
                    bbox,
                    pixels: orig_pixels,
                });
            }
        }
    }

    clusters.sort_by(|a, b| {
        a.bbox
            .y1
            .cmp(&b.bbox.y1)
            .then_with(|| a.bbox.x1.cmp(&b.bbox.x1))
            .then_with(|| a.bbox.y2.cmp(&b.bbox.y2))
            .then_with(|| a.bbox.x2.cmp(&b.bbox.x2))
    });

    clusters
}
#[cfg(test)]
#[path = "cluster_tests.rs"]
mod tests;
