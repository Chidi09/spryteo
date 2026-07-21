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
mod tests {
    use super::*;

    fn make_mask(width: u32, height: u32, ink: &[(u32, u32)]) -> Mask {
        let mut bits = vec![false; (width * height) as usize];
        for &(x, y) in ink {
            let idx = (y * width + x) as usize;
            bits[idx] = true;
        }
        Mask {
            width,
            height,
            bits,
        }
    }

    #[test]
    fn test_bbox_geometry() {
        let b1 = Bbox {
            x1: 10,
            y1: 10,
            x2: 20,
            y2: 20,
        };
        let b2 = Bbox {
            x1: 15,
            y1: 15,
            x2: 25,
            y2: 25,
        };
        let b_touch = Bbox {
            x1: 20,
            y1: 10,
            x2: 30,
            y2: 20,
        };
        let b_disjoint = Bbox {
            x1: 30,
            y1: 30,
            x2: 40,
            y2: 40,
        };

        assert_eq!(b1.width(), 10);
        assert_eq!(b1.height(), 10);
        assert_eq!(b1.area(), 100);
        assert_eq!(b1.center(), (15.0, 15.0));

        assert!(b1.intersects(&b2));
        assert!(b2.intersects(&b1));
        assert!(!b1.intersects(&b_touch));
        assert!(!b1.intersects(&b_disjoint));

        assert_eq!(b1.intersection_area(&b2), 25);
        assert_eq!(b1.intersection_area(&b_touch), 0);
        assert_eq!(b1.intersection_area(&b_disjoint), 0);

        let u = b1.union(&b2);
        assert_eq!(
            u,
            Bbox {
                x1: 10,
                y1: 10,
                x2: 25,
                y2: 25
            }
        );

        assert!(b1.contains_point(10, 10));
        assert!(b1.contains_point(19, 19));
        assert!(!b1.contains_point(20, 20));
        assert!(!b1.contains_point(9, 10));
    }

    #[test]
    fn test_dilate_radius_zero_is_identity() {
        let mask = make_mask(5, 5, &[(2, 2)]);
        let d = dilate(&mask, 0);
        assert_eq!(d.bits, mask.bits);
    }

    #[test]
    fn test_dilate_expands_single_pixel() {
        let mask_center = make_mask(5, 5, &[(2, 2)]);
        let d_center = dilate(&mask_center, 1);
        assert_eq!(d_center.count(), 9);

        let mask_corner = make_mask(5, 5, &[(0, 0)]);
        let d_corner = dilate(&mask_corner, 1);
        assert_eq!(d_corner.count(), 4);
        assert!(d_corner.get(0, 0));
        assert!(d_corner.get(1, 0));
        assert!(d_corner.get(0, 1));
        assert!(d_corner.get(1, 1));
    }

    #[test]
    fn test_find_clusters_separates_disjoint_blobs() {
        // Two 4x4 squares far apart
        let mut ink = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                ink.push((x, y));
                ink.push((x + 20, y + 20));
            }
        }
        let mask = make_mask(30, 30, &ink);
        let cfg = ClusterConfig {
            dilate: 1,
            min_pixels: 10,
        };
        let clusters = find_clusters(&mask, &cfg);
        assert_eq!(clusters.len(), 2);

        assert_eq!(
            clusters[0].bbox,
            Bbox {
                x1: 0,
                y1: 0,
                x2: 4,
                y2: 4
            }
        );
        assert_eq!(clusters[0].pixels, 16);

        assert_eq!(
            clusters[1].bbox,
            Bbox {
                x1: 20,
                y1: 20,
                x2: 24,
                y2: 24
            }
        );
        assert_eq!(clusters[1].pixels, 16);
    }

    #[test]
    fn test_find_clusters_merges_nearby_strokes_via_dilation() {
        // Two 2x2 squares separated by 2 pixels: (0,0)-(1,1) and (4,0)-(5,1)
        let mut ink = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                ink.push((x, y));
                ink.push((x + 4, y));
            }
        }
        let mask = make_mask(10, 10, &ink);

        let cfg_no_dilate = ClusterConfig {
            dilate: 0,
            min_pixels: 1,
        };
        let clusters_no_dilate = find_clusters(&mask, &cfg_no_dilate);
        assert_eq!(clusters_no_dilate.len(), 2);

        let cfg_dilate = ClusterConfig {
            dilate: 3,
            min_pixels: 1,
        };
        let clusters_dilate = find_clusters(&mask, &cfg_dilate);
        assert_eq!(clusters_dilate.len(), 1);
        assert_eq!(
            clusters_dilate[0].bbox,
            Bbox {
                x1: 0,
                y1: 0,
                x2: 6,
                y2: 2
            }
        );
        assert_eq!(clusters_dilate[0].pixels, 8);
    }

    #[test]
    fn test_find_clusters_drops_noise() {
        let mut ink = vec![(0, 0)]; // 1 pixel speck
        for y in 10..14 {
            for x in 10..14 {
                ink.push((x, y)); // 16 pixels square
            }
        }
        let mask = make_mask(20, 20, &ink);
        let cfg = ClusterConfig {
            dilate: 1,
            min_pixels: 12,
        };
        let clusters = find_clusters(&mask, &cfg);
        assert_eq!(clusters.len(), 1);
        assert_eq!(
            clusters[0].bbox,
            Bbox {
                x1: 10,
                y1: 10,
                x2: 14,
                y2: 14
            }
        );
        assert_eq!(clusters[0].pixels, 16);
    }

    #[test]
    fn test_find_clusters_is_deterministic() {
        let mut ink = Vec::new();
        for y in (0..30).step_by(5) {
            for x in (0..30).step_by(5) {
                ink.push((x, y));
            }
        }
        let mask = make_mask(35, 35, &ink);
        let cfg = ClusterConfig {
            dilate: 1,
            min_pixels: 1,
        };

        let run1 = find_clusters(&mask, &cfg);
        let run2 = find_clusters(&mask, &cfg);

        assert_eq!(run1.len(), run2.len());
        for (c1, c2) in run1.iter().zip(run2.iter()) {
            assert_eq!(c1.bbox, c2.bbox);
            assert_eq!(c1.pixels, c2.pixels);
        }
    }

    #[test]
    fn test_cluster_large_region_does_not_stack_overflow() {
        let mut ink = Vec::new();
        for y in 0..400 {
            for x in 0..400 {
                ink.push((x, y));
            }
        }
        let mask = make_mask(400, 400, &ink);
        let cfg = ClusterConfig {
            dilate: 0,
            min_pixels: 1,
        };
        let clusters = find_clusters(&mask, &cfg);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].pixels, 160000);
    }

    #[test]
    fn test_zero_size_mask_does_not_panic() {
        let mask = Mask {
            width: 0,
            height: 0,
            bits: vec![],
        };
        let cfg = ClusterConfig::default();
        let dilated = dilate(&mask, 2);
        assert_eq!(dilated.width, 0);
        let clusters = find_clusters(&mask, &cfg);
        assert!(clusters.is_empty());
    }
}
