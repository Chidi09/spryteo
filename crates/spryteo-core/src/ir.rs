use serde::{Deserialize, Serialize};

/// An sRGB 8-bit colour with red, green, and blue components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A gradient stop representing a color at a specific offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32, // 0.0..=1.0
    pub color: Rgb,
}

/// A fill specification for a scene node, supporting solid colors and gradients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    Solid(Rgb),
    LinearGradient {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,                  // gradient vector, in the shape's local coordinate space
        stops: Vec<GradientStop>, // ordered by offset, at least 2 entries
    },
    RadialGradient {
        cx: f64,
        cy: f64,
        r: f64, // circle in the shape's local coordinate space
        stops: Vec<GradientStop>,
    },
}

// ── Stage 1: Decode & normalise ─────────────────────────────────────────────

/// A decoded and normalised raster image ready for the pipeline.
///
/// Pixels are stored as a flat RGBA 8-8-8-8 byte array in row-major order,
/// top-left origin.  EXIF orientation has been applied, ICC→sRGB conversion
/// has been performed, and any premultiplied alpha has been un-premultiplied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Flat RGBA pixel data, 4 × `width` × `height` bytes.
    pub pixels: Vec<u8>,
}

// ── Stage 2: Input classification ───────────────────────────────────────────

/// The input image after the classifier has assigned it a concrete profile.
///
/// If the user selected `Mode::Auto`, the `mode` field holds the classifier's
/// best guess.  The background colour, if detectable, is also recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedInput {
    pub image: RasterImage,
    /// The resolved mode after classification (never `Auto`).
    pub mode: Mode,
    /// Detected background colour, if any (§3.3).
    pub background_color: Option<Rgb>,
}

// Re-export Mode from options so ir.rs can reference it.
use crate::options::Mode;

// ── Stage 3: Quantised layers ───────────────────────────────────────────────

/// A single quantised colour layer.
///
/// Each layer carries a soft mask with per-pixel coverage values (0–255) and
/// a solid `color`.  Layers are ordered bottom-up by `z_order`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Per-pixel coverage mask, `width` × `height` bytes, values 0–255.
    /// A value of 0 means the pixel is not covered by this layer; 255 means
    /// fully covered.
    pub mask: Vec<u8>,
    /// The solid colour assigned to this layer.
    pub color: Rgb,
    /// Paint order (0 = bottom, first to be painted).
    pub z_order: usize,
}

/// A stack of quantised colour layers produced by the quantiser.
///
/// Layers are stored in paint order (bottom / background first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerStack {
    pub layers: Vec<Layer>,
}

// ── Stage 4: Contour extraction ─────────────────────────────────────────────

/// A single closed contour (exterior or hole) traced from a layer's soft mask,
/// stored at subpixel coordinates.
///
/// The hole hierarchy is represented through recursive nesting: if a contour
/// has children, those children are holes *inside* this contour.  Each child
/// may in turn have its own children (islands inside the hole), and so on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contour {
    /// Subpixel vertex coordinates in the image plane (px).
    pub points: Vec<(f64, f64)>,
    /// Child contours (holes) inside this contour.
    pub children: Vec<Contour>,
}

/// All contours extracted from a `LayerStack`, grouped per layer in paint
/// order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContourSet {
    /// Outer vector indexes layers; each inner vector holds that layer's
    /// top-level contours (the ones that are not holes).
    pub layers: Vec<Vec<Contour>>,
}

// ── Stage 5: Curve fitting ──────────────────────────────────────────────────

/// A single segment within a fitted vector path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PathElement {
    /// Move the pen to (`x`, `y`) without drawing.
    MoveTo(f64, f64),
    /// Draw a straight line from the current position to (`x`, `y`).
    LineTo(f64, f64),
    /// Draw a cubic Bézier curve to (`x3`, `y3`) with control points
    /// (`x1`, `y1`) and (`x2`, `y2`).
    CurveTo(f64, f64, f64, f64, f64, f64),
    /// Close the current subpath with a straight line back to its start.
    ClosePath,
}

/// A recognised geometric primitive that can be emitted as a dedicated SVG
/// element rather than a generic `<path>`.
///
/// Order of checking (§3.8): circle → ellipse → rect → arc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Primitive {
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
    },
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rx: Option<f64>,
        ry: Option<f64>,
    },
    Arc {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        start_angle: f64,
        end_angle: f64,
        rotation: f64,
    },
}

/// A single fitted curve (open or closed) produced from one contour.
///
/// When primitive recognition succeeds, `primitive` is set to `Some` and
/// the `segments` field still holds the raw path data for fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Curve {
    pub segments: Vec<PathElement>,
    /// The recognised primitive form, if any (§3.8).
    pub primitive: Option<Primitive>,
}

/// All fitted curves for all layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveSet {
    pub curves: Vec<Curve>,
}

// ── Stage 6: Scene graph ────────────────────────────────────────────────────

/// A stroke specification for a scene node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Rgb,
    pub width: f64,
}

/// A local transform (translation offset) applied to a scene node.
///
/// When `transform_origin` is `Centroid`, this holds the shape's centroid so
/// that CSS `rotate()` / `scale()` animate from that point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transform {
    pub translate_x: f64,
    pub translate_y: f64,
}

/// The shape content of a scene node — either a raw vector path or a
/// recognised primitive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Shape {
    Path(Vec<PathElement>),
    Primitive(Primitive),
}

/// A single drawable node in the scene graph.
///
/// Each node corresponds to exactly one shape — the "never merge" invariant
/// (§3.12) guarantees that distinct shapes are never boolean-combined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub fill: Option<Fill>,
    pub stroke: Option<Stroke>,
    pub transform: Transform,
    pub shape: Shape,
}

/// A group (`<g>`) in the scene graph.
///
/// Z-order is implicit in `Vec` ordering: items earlier in the vectors are
/// painted first (bottom).  There is no separate sortable z-order field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub nodes: Vec<Node>,
    pub groups: Vec<Group>,
}

/// The full scene graph — grouped, ID'd, z-ordered, with resolved fills,
/// strokes, and transforms.
///
/// This is the final structural IR before SVG emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneGraph {
    pub groups: Vec<Group>,
}

// ── Stage 7: SVG document + metadata ────────────────────────────────────────

/// The final serialised SVG document.
///
/// Holds the output markup string plus structural metadata such as the
/// viewBox so that consumers can re-derive scene information without
/// re-parsing the SVG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvgDocument {
    pub svg: String,
    pub view_box: Option<(f64, f64, f64, f64)>,
}

/// Axis-aligned bounding box for a shape or group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bbox {
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
}

/// Aggregate statistics for the entire conversion result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub node_count: usize,
    pub path_count: usize,
    pub byte_count: usize,
}

/// Per-element metadata recorded in the sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMeta {
    pub id: String,
    pub bbox: Bbox,
    pub centroid: (f64, f64),
    pub area: f64,
    pub fill: Option<Rgb>,
    pub group: String,
    pub z_order: usize,
    pub suggested_draw_order: usize,
}

/// The metadata sidecar (§3.12) — a JSON-serialisable payload returned
/// alongside the SVG with per-node layout information and aggregate stats.
///
/// This is the primary machine-oriented output that enables agentic
/// workflows: a consumer can inspect every shape's bounding box, centroid,
/// area, fill, group membership, and paint order without parsing SVG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub nodes: Vec<NodeMeta>,
    pub stats: Stats,
    #[serde(default)]
    pub current_color_applied: bool,
}

/// The top-level result returned by every conversion surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertResult {
    pub svg: String,
    pub meta: Meta,
}
