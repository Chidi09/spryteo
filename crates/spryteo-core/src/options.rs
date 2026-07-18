use serde::{Deserialize, Serialize};

use crate::ir::Rgb;

/// How the engine should interpret the input image.
///
/// `Auto` runs the classifier from §3.2 and selects the pipeline profile
/// automatically.  The other variants force a specific profile regardless of
/// what the classifier would pick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Mode {
    Auto,
    Icon,
    PixelArt,
    LineArt,
    Photo,
}

/// Specifies the target colour palette for the output.
///
/// `Auto` picks the colour count heuristically (elbow on within-cluster
/// error).  `N` sets an exact number of colours (2..=64).  `Palette`
/// supplies a fixed set of brand colours to use instead of clustering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColorSpec {
    Auto,
    N(u8),
    Palette(Vec<Rgb>),
}

/// How quantized colour layers compose to form the final image.
///
/// `Stacked` paints layers bottom-up; each layer's region includes everything
/// above it, which produces fewer sliver artifacts.  `Cutout` uses exact
/// disjoint regions, yielding smaller output but sometimes worse edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Layering {
    Stacked,
    Cutout,
}

/// A three-state toggle used for options that have an automatic mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Tri {
    Auto,
    On,
    Off,
}

/// How shapes are collected into `<g>` groups in the output SVG.
///
/// `Component` groups by connected component – always available, no ML
/// required.  `Semantic` uses MobileSAM mask-guided grouping (requires
/// the `semantic` feature and downloaded model weights).  `Flat` emits
/// every shape at the top level with no grouping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Grouping {
    Component,
    Semantic,
    Flat,
}

/// Strategy for generating `id` attributes on SVG elements.
///
/// `Hash` produces deterministic IDs based on the content
/// (`blake3(geometry + fill + z-index)`, first 8 hex chars).  `Sequential`
/// numbers shapes in paint order.  `None` omits `id` attributes entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdStyle {
    Hash,
    Sequential,
    None,
}

/// Where the transform origin is placed for CSS animation.
///
/// `Centroid` sets `transform="translate(cx cy)"` on the shape so that
/// `rotate()` and `scale()` in CSS behave naturally without
/// `transform-box`.  `Baked` leaves coordinates absolute (no translate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TOrigin {
    Centroid,
    Baked,
}

/// How alpha (transparency) in the source image is handled.
///
/// `Keep` preserves alpha as a soft mask through quantization.  `Matte`
/// composites the image over a solid colour before tracing.  `Threshold`
/// treats pixels with alpha ≥ the given value as fully opaque and the rest
/// as fully transparent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlphaMode {
    Keep,
    Matte(Rgb),
    Threshold(u8),
}

/// How the detected background region is treated in the output.
///
/// `Keep` traces the background as a normal layer.  `Drop` omits the
/// background layer entirely.  `Rect` emits the background as a single
/// solid `<rect>` element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Background {
    Keep,
    Drop,
    Rect,
}

/// Output format for the generated SVG.
///
/// `Svg` emits minified markup.  `SvgPretty` adds indentation and line
/// breaks.  `Jsx` applies React-compatible attribute transforms (e.g.
/// `class` → `className`, `stroke-width` → `strokeWidth`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    Svg,
    SvgPretty,
    Jsx,
}

/// Preset CSS animation theme injected alongside the SVG.
///
/// `Draw` emits a `stroke-dasharray` draw-on animation.  `Fade` fades
/// shapes in by group.  `Pop` applies a small bounce-scale entrance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Preset {
    Draw,
    Fade,
    Pop,
}

/// The single options surface for the entire Spryteo pipeline.
///
/// Mirrored 1:1 in CLI flags, the npm options object, API JSON bodies, and
/// the MCP tool schema.  Every field is serde-serialisable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertOptions {
    /// Pipeline profile (§3.2).  Controls whether the engine treats the
    /// input as a photo, icon, line-art, or pixel-art image.
    pub mode: Mode,

    /// Enable centerline / stroke tracing (§3.9).  When true the output
    /// traces the *centre* of each line (for `stroke-dasharray` draw-on
    /// animation) instead of outlining filled shapes.
    pub stroke: bool,

    /// Colour palette specification (§3.4).  Controls how many colours
    /// the quantiser targets and whether a fixed palette is enforced.
    pub colors: ColorSpec,

    /// Layer composition mode (§3.4).  `Stacked` paints layers bottom-up
    /// with overlap; `Cutout` uses disjoint regions.
    pub layering: Layering,

    /// Global curve-fit error budget in pixels (§3.7).  Lower values
    /// produce more faithful but larger output; higher values give
    /// smoother curves with fewer nodes.
    pub tolerance: f32,

    /// Corner-preservation strength (§3.6).  Maps to Potrace's `alphamax`
    /// parameter — controls how aggressively sharp corners are rounded.
    /// 0.0 = keep every corner, 1.34 = smooth everything.
    pub smoothness: f32,

    /// Minimum region area in square pixels (§3.5).  Regions (connected
    /// components) whose area is below this threshold are discarded as
    /// speckle noise.
    pub turdsize: u32,

    /// Gradient detection mode (§3.10).  `Auto` enables gradients for
    /// photos and disables them for icons; `On`/`Off` overrides.
    pub gradients: Tri,

    /// Grouping strategy (§3.11).  Controls how individual shapes are
    /// collected into `<g>` elements in the output.
    pub grouping: Grouping,

    /// ID generation style (§3.12).  Determines whether and how SVG
    /// elements receive stable `id` attributes.
    pub id_style: IdStyle,

    /// Transform origin placement (§3.12).  Controls whether each shape
    /// is translated to its centroid so CSS transforms rotate/scale
    /// from the shape's centre.
    pub transform_origin: TOrigin,

    /// Number of decimal places in SVG path coordinates (§3.13).
    /// Default 2; 1 is common for small icons.
    pub precision: u8,

    /// Maximum dimension (width or height) in pixels for tracing (§3.1).
    /// If `None` no downscaling is applied before tracing.
    pub max_trace_dimension: Option<u32>,

    /// Background treatment (§3.3).  Controls whether a detected
    /// background is kept, dropped, or replaced with a solid `<rect>`.
    pub background: Background,

    /// Alpha channel handling (§3.1).  Controls whether source
    /// transparency is preserved, matted, or thresholded.
    pub alpha_mode: AlphaMode,

    /// Emit SVG arc (`A`) commands for detected circular arcs (§3.8).
    /// Some animation tooling handles arcs poorly, so this defaults off
    /// even when arcs are detected in geometry.
    pub arcs: bool,

    /// Output markup format (§3.13).  Controls whether the SVG string
    /// is minified, pretty-printed, or transformed for React JSX.
    pub output: OutputFormat,

    /// Optional CSS animation preset to inject alongside the SVG (§3.12).
    pub emit_css: Option<Preset>,

    /// Maximum pixel count of the decoded image (§3.1).  Inputs with
    /// more pixels than this are rejected before any processing.
    pub max_pixels: u64,

    /// Maximum size of the raw input bytes (§3.1).  Inputs larger than
    /// this are rejected before decoding.
    pub max_input_bytes: u64,

    /// Optional timeout in milliseconds (§3.1).  If `None` the pipeline
    /// runs without a deadline.
    pub timeout_ms: Option<u64>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Auto,
            stroke: false,
            colors: ColorSpec::Auto,
            layering: Layering::Stacked,
            tolerance: 0.5,
            smoothness: 1.0,
            turdsize: 2,
            gradients: Tri::Auto,
            grouping: Grouping::Component,
            id_style: IdStyle::Hash,
            transform_origin: TOrigin::Centroid,
            precision: 2,
            max_trace_dimension: None,
            background: Background::Keep,
            alpha_mode: AlphaMode::Keep,
            arcs: false,
            output: OutputFormat::Svg,
            emit_css: None,
            max_pixels: 16_000_000,
            max_input_bytes: 8_000_000,
            timeout_ms: None,
        }
    }
}
