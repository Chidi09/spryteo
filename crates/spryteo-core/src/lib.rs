//! IR types, ConvertOptions, pipeline orchestration, traits.

pub mod cancel;
pub mod error;
pub mod ir;
pub mod options;
pub mod schema;

pub use cancel::{CancelToken, Clock, ManualClock, POLL_INTERVAL};
pub use error::{LimitKind, SpryteoError};
pub use ir::{
    Bbox, ClassifiedInput, Contour, ContourSet, ConvertResult, Curve, CurveOrigin, CurveSet, Fill,
    GradientStop, Group, GroupMeta, Layer, LayerStack, Meta, Node, NodeMeta, PathElement,
    Primitive, RasterImage, Rgb, SceneGraph, Shape, Stats, Stroke, SvgDocument, Transform,
};
pub use options::{
    format_hex_color, options_from_json, options_from_json_with_base, parse_alpha_mode,
    parse_hex_color, AlphaMode, Background, ColorSpec, ConvertOptions, ConvertOptionsPatch,
    Grouping, IdStyle, Layering, Mode, OutputFormat, Preset, TOrigin, Tri,
};

#[cfg(test)]
mod tests;
