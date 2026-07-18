//! Color quantization, layering, palette handling (§3.4).
//!
//! ## Stage 2: Input classification (`classify`)
//! Heuristics-based classifier that assigns an input image to one of four
//! profiles: `PixelArt`, `Icon`, `LineArt`, or `Photo`.  Supports forced
//! mode (skip heuristics) and automatic background colour detection.
//!
//! ## Stage 3: Quantization (`quantize`)
//! Produces a `LayerStack` from a classified input.  For icon-mode:
//! - Exact histogram if unique colours ≤ target
//! - Deterministic k-means++ in CIELAB space (fixed seed) otherwise
//! - Nearest-palette assignment for `ColorSpec::Palette`
//!
//! ## Out of scope (this dispatch)
//! - Photo-mode stacked/cutout layering split (§3.4) — Phase 4.
//! - Line-art stroke-width histogram unimodality check (§3.2) — documented
//!   simplification; only ink-ratio is checked.

pub mod classifier;
pub mod color;
pub mod quantize;

pub use classifier::{classify, detect_background};
pub use color::Lab;
pub use quantize::quantize;
