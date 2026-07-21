pub mod cells;
pub mod chroma;
pub mod cluster;
pub mod color;
pub mod lattice;
pub mod luma;
pub mod reconcile;
pub mod textband;

pub use cells::{extract_cells, ink_bbox_in, CellConfig, IconCell};
pub use chroma::{
    auto_sat_threshold, check_chroma_usable, chroma_mask, ink_coverage, saturation, ChromaConfig,
    Mask, SheetError,
};
pub use cluster::{dilate, find_clusters, Bbox, Cluster, ClusterConfig};
pub use color::{dominant_color, fit_linear_gradient, ColorConfig, GradientFit};
pub use lattice::{
    find_bands, infer_lattice, project, score_lattice, Axis, Band, Lattice, LatticeConfig,
};
pub use luma::{
    analyze_luma, background_luminance, check_luma_usable, luma_coverage, luma_mask, luminance,
    LumaConfig, LumaInfo,
};
pub use reconcile::{reconcile, Reconciliation};
pub use textband::{classify_rows, label_band_below, strip_text_rows, TextBandConfig, TextBands};
