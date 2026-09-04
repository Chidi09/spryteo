use crate::chroma::Mask;
use crate::cluster::Bbox;
use crate::lattice::Lattice;

#[derive(Debug, Clone)]
pub struct IconCell {
    pub col: usize,
    pub row: usize,
    /// The lattice cell rect this icon came from.
    pub cell_rect: Bbox,
    /// The TIGHT ink bounding box, recomputed inside this cell, plus padding.
    /// This is what gets cropped.
    pub crop: Bbox,
    /// Ink pixels inside `cell_rect`.
    pub ink_pixels: u32,
}

#[derive(Debug, Clone)]
pub struct CellConfig {
    /// Pixels of padding added around the tight ink bbox, clipped to the image.
    pub pad: u32, // default 2
    /// Cells with fewer ink pixels than this are skipped entirely.
    pub min_ink: u32, // default 12
}

impl Default for CellConfig {
    fn default() -> Self {
        CellConfig {
            pad: 2,
            min_ink: 12,
        }
    }
}

/// Tight bbox of set mask bits within `rect`. None when `rect` holds no ink.
pub fn ink_bbox_in(mask: &Mask, rect: &Bbox) -> Option<Bbox> {
    if mask.width == 0 || mask.height == 0 {
        return None;
    }

    let x_min = rect.x1.min(mask.width);
    let x_max = rect.x2.min(mask.width);
    let y_min = rect.y1.min(mask.height);
    let y_max = rect.y2.min(mask.height);

    if x_min >= x_max || y_min >= y_max {
        return None;
    }

    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;

    for y in y_min..y_max {
        for x in x_min..x_max {
            if mask.get(x, y) {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if found {
        Some(Bbox {
            x1: min_x,
            y1: min_y,
            x2: max_x + 1,
            y2: max_y + 1,
        })
    } else {
        None
    }
}

/// One IconCell per non-empty lattice cell, in row-major order.
pub fn extract_cells(mask: &Mask, lat: &Lattice, cfg: &CellConfig) -> Vec<IconCell> {
    let mut cells = Vec::new();
    let num_rows = lat.rows.len();
    let num_cols = lat.cols.len();

    for r in 0..num_rows {
        for c in 0..num_cols {
            let Some((x, y, w, h)) = lat.cell_rect(c, r) else {
                continue;
            };
            let cell_rect = Bbox {
                x1: x,
                y1: y,
                x2: x + w,
                y2: y + h,
            };

            let x_min = cell_rect.x1.min(mask.width);
            let x_max = cell_rect.x2.min(mask.width);
            let y_min = cell_rect.y1.min(mask.height);
            let y_max = cell_rect.y2.min(mask.height);

            let mut ink_pixels = 0u32;
            for yy in y_min..y_max {
                for xx in x_min..x_max {
                    if mask.get(xx, yy) {
                        ink_pixels += 1;
                    }
                }
            }

            if ink_pixels < cfg.min_ink {
                continue;
            }

            let Some(tight) = ink_bbox_in(mask, &cell_rect) else {
                continue;
            };

            let crop_x1 = tight.x1.saturating_sub(cfg.pad);
            let crop_y1 = tight.y1.saturating_sub(cfg.pad);
            let crop_x2 = (tight.x2.saturating_add(cfg.pad)).min(mask.width);
            let crop_y2 = (tight.y2.saturating_add(cfg.pad)).min(mask.height);

            let crop = Bbox {
                x1: crop_x1,
                y1: crop_y1,
                x2: crop_x2,
                y2: crop_y2,
            };

            cells.push(IconCell {
                col: c,
                row: r,
                cell_rect,
                crop,
                ink_pixels,
            });
        }
    }

    cells
}
#[cfg(test)]
#[path = "cells_tests.rs"]
mod tests;
