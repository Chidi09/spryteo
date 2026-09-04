//! IR types, ConvertOptions, pipeline orchestration, traits.

pub mod cancel;
pub mod error;
pub mod ir;
pub mod options;

pub use cancel::{CancelToken, Clock, ManualClock, POLL_INTERVAL};
pub use error::{LimitKind, SpryteoError};
pub use ir::{
    Bbox, ClassifiedInput, Contour, ContourSet, ConvertResult, Curve, CurveSet, Fill, GradientStop,
    Group, Layer, LayerStack, Meta, Node, NodeMeta, PathElement, Primitive, RasterImage, Rgb,
    SceneGraph, Shape, Stats, Stroke, SvgDocument, Transform,
};
pub use options::{
    AlphaMode, Background, ColorSpec, ConvertOptions, Grouping, IdStyle, Layering, Mode,
    OutputFormat, Preset, TOrigin, Tri,
};

#[cfg(test)]
mod tests;
