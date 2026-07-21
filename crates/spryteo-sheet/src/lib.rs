pub mod chroma;
pub mod lattice;

pub use chroma::{
    auto_sat_threshold, check_chroma_usable, chroma_mask, ink_coverage, saturation, ChromaConfig,
    Mask, SheetError,
};
pub use lattice::{
    find_bands, infer_lattice, project, score_lattice, Axis, Band, Lattice, LatticeConfig,
};
