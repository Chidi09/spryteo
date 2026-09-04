use crate::cluster::{Bbox, Cluster};
use crate::lattice::Lattice;

#[derive(Debug, Clone)]
pub struct Reconciliation {
    /// Cells matched 1:1 with exactly one cluster.
    pub matched: usize,
    /// Clusters overlapping 2+ cells: the lattice MERGED icons that clustering
    /// kept apart, or an icon overflows its cell.
    pub straddling: Vec<Bbox>,
    /// Clusters overlapping NO cell: real ink the lattice missed entirely.
    pub orphans: Vec<Bbox>,
    /// (col, row) of cells containing no cluster: the lattice hallucinated a
    /// row or column, or an icon is missing.
    pub empty_cells: Vec<(usize, usize)>,
    /// Cells matched by 2+ clusters. NOT an error on its own -- these icons are
    /// genuinely multi-component (measured: 288 components across 105 icons on
    /// the target sheet) -- but a cell with an unusually high count is worth
    /// surfacing.
    pub multi_cluster_cells: Vec<(usize, usize, usize)>, // (col, row, cluster_count)
    /// Agreement score in 0.0..=1.0; 1.0 when every cell matched exactly once
    /// and nothing straddled or orphaned.
    pub agreement: f32,
}

pub fn reconcile(lat: &Lattice, clusters: &[Cluster]) -> Reconciliation {
    let total_cells = lat.cell_count();
    if total_cells == 0 {
        return Reconciliation {
            matched: 0,
            straddling: Vec::new(),
            orphans: Vec::new(),
            empty_cells: Vec::new(),
            multi_cluster_cells: Vec::new(),
            agreement: 0.0,
        };
    }

    let num_cols = lat.cols.len();
    let num_rows = lat.rows.len();

    let mut cell_boxes = Vec::with_capacity(total_cells);
    for r in 0..num_rows {
        for c in 0..num_cols {
            if let Some((x, y, w, h)) = lat.cell_rect(c, r) {
                cell_boxes.push((
                    c,
                    r,
                    Bbox {
                        x1: x,
                        y1: y,
                        x2: x + w,
                        y2: y + h,
                    },
                ));
            }
        }
    }

    let mut cell_cluster_counts = vec![vec![0usize; num_cols]; num_rows];
    let mut straddling = Vec::new();
    let mut orphans = Vec::new();

    for cluster in clusters {
        let area = cluster.bbox.area();
        let mut overlap_cells = Vec::new();

        for &(c, r, ref cb) in &cell_boxes {
            let inter = cluster.bbox.intersection_area(cb);
            if area > 0 && (inter as f64) >= 0.25 * (area as f64) {
                overlap_cells.push((c, r));
            }
        }

        match overlap_cells.len() {
            0 => orphans.push(cluster.bbox),
            1 => {
                let (c, r) = overlap_cells[0];
                cell_cluster_counts[r][c] += 1;
            }
            _ => {
                straddling.push(cluster.bbox);
                for &(c, r) in &overlap_cells {
                    cell_cluster_counts[r][c] += 1;
                }
            }
        }
    }

    let sort_bbox = |a: &Bbox, b: &Bbox| {
        a.y1.cmp(&b.y1)
            .then_with(|| a.x1.cmp(&b.x1))
            .then_with(|| a.y2.cmp(&b.y2))
            .then_with(|| a.x2.cmp(&b.x2))
    };

    straddling.sort_by(sort_bbox);
    orphans.sort_by(sort_bbox);

    let mut matched = 0;
    let mut empty_cells = Vec::new();
    let mut multi_cluster_cells = Vec::new();

    for (r, row_counts) in cell_cluster_counts.iter().enumerate() {
        for (c, &count) in row_counts.iter().enumerate() {
            match count {
                0 => empty_cells.push((c, r)),
                1 => matched += 1,
                _ => multi_cluster_cells.push((c, r, count)),
            }
        }
    }

    let base_agreement = matched as f32 / total_cells as f32;
    let penalty = (straddling.len() as f32 * 0.1) + (orphans.len() as f32 * 0.1);
    let agreement = (base_agreement - penalty).clamp(0.0, 1.0);

    Reconciliation {
        matched,
        straddling,
        orphans,
        empty_cells,
        multi_cluster_cells,
        agreement,
    }
}
#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;
