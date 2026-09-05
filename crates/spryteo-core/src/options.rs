//! The public options contract (ROADMAP §3, issue #2).
//!
//! One options surface, mirrored 1:1 in CLI flags, the npm options object,
//! MCP tool arguments, and any future HTTP body. The JSON encoding here is
//! the documented public contract, not whatever Serde's defaults happen to
//! produce:
//!
//! - Enums are lowercase/kebab-case strings (`"icon"`, `"pixel-art"`), not
//!   Rust variant names.
//! - `colors` is `"auto"`, an integer, or an array of hex colours.
//! - `alphaMode` is `"keep"`, `"matte:#rrggbb"`, or `"threshold:N"`.
//! - The animation preset key is `css`, matching the CLI flag and the
//!   documented examples, rather than the internal field name.
//! - Keys are camelCase with snake_case accepted as an alias.
//! - Unknown keys are an error, so a typo or a stale option name fails
//!   loudly instead of being silently dropped.
//!
//! Before this, `ConvertOptions` used default Serde enum encoding, so the
//! documented example `{"mode":"icon","colors":8,"css":"draw"}` failed to
//! deserialize on `mode` and `colors` and silently ignored `css`.

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};

use crate::error::SpryteoError;
use crate::ir::Rgb;

/// How the engine should interpret the input image.
///
/// `Auto` runs the classifier from §3.2 and selects the pipeline profile
/// automatically.  The other variants force a specific profile regardless of
/// what the classifier would pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Auto,
    Icon,
    #[serde(alias = "pixelart", alias = "pixel_art")]
    PixelArt,
    #[serde(alias = "lineart", alias = "line_art")]
    LineArt,
    Photo,
}

/// Specifies the target colour palette for the output.
///
/// `Auto` picks the colour count heuristically (elbow on within-cluster
/// error).  `N` sets an exact number of colours (2..=64).  `Palette`
/// supplies a fixed set of brand colours to use instead of clustering.
///
/// JSON form: `"auto"`, an integer such as `8`, or an array of colours
/// such as `["#ff0000", "#00ff00"]` (issue #21).
#[derive(Debug, Clone, PartialEq)]
pub enum ColorSpec {
    Auto,
    N(u8),
    Palette(Vec<Rgb>),
}

/// Parse `#rgb`, `#rrggbb`, or `rrggbb` into an [`Rgb`].
pub fn parse_hex_color(s: &str) -> Result<Rgb, String> {
    let h = s.trim().trim_start_matches('#');
    let expand = |c: u8| -> u8 {
        let v = (c as char).to_digit(16).unwrap_or(0) as u8;
        v * 17
    };
    match h.len() {
        3 => {
            if !h.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("invalid hex colour '{s}'"));
            }
            let b = h.as_bytes();
            Ok(Rgb {
                r: expand(b[0]),
                g: expand(b[1]),
                b: expand(b[2]),
            })
        }
        6 => {
            if !h.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("invalid hex colour '{s}'"));
            }
            Ok(Rgb {
                r: u8::from_str_radix(&h[0..2], 16).map_err(|e| e.to_string())?,
                g: u8::from_str_radix(&h[2..4], 16).map_err(|e| e.to_string())?,
                b: u8::from_str_radix(&h[4..6], 16).map_err(|e| e.to_string())?,
            })
        }
        _ => Err(format!(
            "invalid hex colour '{s}': expected #rgb or #rrggbb"
        )),
    }
}

/// Render an [`Rgb`] as `#rrggbb`.
pub fn format_hex_color(c: &Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

impl Serialize for ColorSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ColorSpec::Auto => s.serialize_str("auto"),
            ColorSpec::N(n) => s.serialize_u8(*n),
            ColorSpec::Palette(colors) => {
                let mut seq = s.serialize_seq(Some(colors.len()))?;
                for c in colors {
                    seq.serialize_element(&format_hex_color(c))?;
                }
                seq.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ColorSpec;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("\"auto\", an integer colour count, or an array of hex colours")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<ColorSpec, E> {
                if v.eq_ignore_ascii_case("auto") {
                    Ok(ColorSpec::Auto)
                } else {
                    Err(E::custom(format!(
                        "invalid colors value '{v}': expected \"auto\", an integer, or an array of hex colours"
                    )))
                }
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<ColorSpec, E> {
                u8::try_from(v)
                    .map(ColorSpec::N)
                    .map_err(|_| E::custom(format!("colors value {v} is out of range (0-255)")))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<ColorSpec, E> {
                if v < 0 {
                    return Err(E::custom(format!("colors value {v} cannot be negative")));
                }
                self.visit_u64(v as u64)
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<ColorSpec, A::Error> {
                let mut colors = Vec::new();
                while let Some(entry) = seq.next_element::<PaletteEntry>()? {
                    colors.push(entry.into_rgb().map_err(de::Error::custom)?);
                }
                Ok(ColorSpec::Palette(colors))
            }
        }
        d.deserialize_any(V)
    }
}

/// One palette element: either `"#rrggbb"` or `{"r":..,"g":..,"b":..}`.
#[derive(Deserialize)]
#[serde(untagged)]
enum PaletteEntry {
    Hex(String),
    Rgb { r: u8, g: u8, b: u8 },
}

impl PaletteEntry {
    fn into_rgb(self) -> Result<Rgb, String> {
        match self {
            PaletteEntry::Hex(s) => parse_hex_color(&s),
            PaletteEntry::Rgb { r, g, b } => Ok(Rgb { r, g, b }),
        }
    }
}

/// How quantized colour layers compose to form the final image.
///
/// `Stacked` paints layers bottom-up; each layer's region includes everything
/// above it, which produces fewer sliver artifacts.  `Cutout` uses exact
/// disjoint regions, yielding smaller output but sometimes worse edges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layering {
    Stacked,
    Cutout,
}

/// A three-state toggle used for options that have an automatic mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Grouping {
    Component,
    Semantic,
    Flat,
}

/// Strategy for generating `id` attributes on SVG elements.
///
/// `Hash` produces deterministic IDs derived from the shape's full
/// canonicalized geometry and paint.  `Sequential` numbers shapes in paint
/// order.  `None` omits `id` attributes entirely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
///
/// JSON form: `"keep"`, `"matte:#rrggbb"`, or `"threshold:128"`. The
/// object forms `{"matte":"#rrggbb"}` and `{"threshold":128}` are also
/// accepted.
#[derive(Debug, Clone, PartialEq)]
pub enum AlphaMode {
    Keep,
    Matte(Rgb),
    Threshold(u8),
}

/// Parse the public string form of an alpha mode. Shared by the CLI flag
/// parser and the JSON deserializer so both accept exactly the same
/// spellings.
pub fn parse_alpha_mode(s: &str) -> Result<AlphaMode, String> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("keep") {
        return Ok(AlphaMode::Keep);
    }
    if let Some(rest) = t.strip_prefix("matte:") {
        return parse_hex_color(rest).map(AlphaMode::Matte);
    }
    if let Some(rest) = t.strip_prefix("threshold:") {
        return rest
            .trim()
            .parse::<u8>()
            .map(AlphaMode::Threshold)
            .map_err(|_| format!("invalid alpha threshold '{rest}': expected 0-255"));
    }
    Err(format!(
        "invalid alphaMode '{s}': expected keep, matte:#rrggbb, or threshold:0-255"
    ))
}

impl std::fmt::Display for AlphaMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlphaMode::Keep => write!(f, "keep"),
            AlphaMode::Matte(c) => write!(f, "matte:{}", format_hex_color(c)),
            AlphaMode::Threshold(t) => write!(f, "threshold:{t}"),
        }
    }
}

impl Serialize for AlphaMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AlphaMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = AlphaMode;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("\"keep\", \"matte:#rrggbb\", or \"threshold:0-255\"")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<AlphaMode, E> {
                parse_alpha_mode(v).map_err(E::custom)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<AlphaMode, A::Error> {
                let key: String = match map.next_key()? {
                    Some(k) => k,
                    None => return Err(de::Error::custom("empty alphaMode object")),
                };
                let result = match key.as_str() {
                    "matte" => {
                        let entry: PaletteEntry = map.next_value()?;
                        AlphaMode::Matte(entry.into_rgb().map_err(de::Error::custom)?)
                    }
                    "threshold" => AlphaMode::Threshold(map.next_value()?),
                    other => {
                        return Err(de::Error::custom(format!(
                            "unknown alphaMode key '{other}': expected 'matte' or 'threshold'"
                        )))
                    }
                };
                if map.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom(
                        "alphaMode object must have exactly one key",
                    ));
                }
                Ok(result)
            }
        }
        d.deserialize_any(V)
    }
}

/// How the detected background region is treated in the output.
///
/// `Keep` traces the background as a normal layer.  `Drop` omits the
/// background layer entirely.  `Rect` emits the background as a single
/// solid `<rect>` element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Background {
    Keep,
    Drop,
    Rect,
}

/// Output format for the generated SVG.
///
/// `Svg` emits minified markup.  `SvgPretty` adds indentation and line
/// breaks.  `Jsx` emits a reusable React component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    Svg,
    #[serde(alias = "pretty", alias = "svgPretty")]
    SvgPretty,
    Jsx,
}

/// Preset CSS animation theme injected alongside the SVG.
///
/// `Draw` emits a `stroke-dasharray` draw-on animation.  `Fade` fades
/// shapes in by group.  `Pop` applies a small bounce-scale entrance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    Draw,
    Fade,
    Pop,
}

/// The single options surface for the entire Spryteo pipeline.
///
/// Mirrored 1:1 in CLI flags, the npm options object, API JSON bodies, and
/// the MCP tool schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    #[serde(alias = "id_style")]
    pub id_style: IdStyle,

    /// Transform origin placement (§3.12).  Controls whether each shape
    /// is translated to its centroid so CSS transforms rotate/scale
    /// from the shape's centre.
    #[serde(alias = "transform_origin")]
    pub transform_origin: TOrigin,

    /// Number of decimal places in SVG path coordinates (§3.13).
    /// Default 2; 1 is common for small icons.
    pub precision: u8,

    /// Maximum dimension (width or height) in pixels for tracing (§3.1).
    /// If `None` no downscaling is applied before tracing.
    #[serde(alias = "max_trace_dimension")]
    pub max_trace_dimension: Option<u32>,

    /// Background treatment (§3.3).  Controls whether a detected
    /// background is kept, dropped, or replaced with a solid `<rect>`.
    pub background: Background,

    /// Alpha channel handling (§3.1).  Controls whether source
    /// transparency is preserved, matted, or thresholded.
    #[serde(alias = "alpha_mode")]
    pub alpha_mode: AlphaMode,

    /// Emit SVG arc (`A`) commands for detected circular arcs (§3.8).
    /// Some animation tooling handles arcs poorly, so this defaults off
    /// even when arcs are detected in geometry.
    pub arcs: bool,

    /// Force monochrome fill color to inherit via `currentColor`.
    /// When enabled and the output contains exactly one flat fill color
    /// (excluding any background rect), that fill color is written as
    /// `currentColor` instead of a hex value.
    #[serde(alias = "current_color")]
    pub current_color: bool,

    /// Output markup format (§3.13).  Controls whether the SVG string
    /// is minified, pretty-printed, or emitted as a React component.
    pub output: OutputFormat,

    /// Optional CSS animation preset to inject alongside the SVG (§3.12).
    ///
    /// The public key is `css`, matching the `--css` CLI flag and the
    /// documented examples. `emit_css` is accepted as an alias for the
    /// internal name this field used to serialize under.
    #[serde(rename = "css", alias = "emitCss", alias = "emit_css")]
    pub emit_css: Option<Preset>,

    /// Maximum pixel count of the decoded image (§3.1).  Inputs with
    /// more pixels than this are rejected before any processing.
    #[serde(alias = "max_pixels")]
    pub max_pixels: u64,

    /// Maximum size of the raw input bytes (§3.1).  Inputs larger than
    /// this are rejected before decoding.
    #[serde(alias = "max_input_bytes")]
    pub max_input_bytes: u64,

    /// Optional timeout in milliseconds (§3.1).  If `None` the pipeline
    /// runs without a deadline. `0` means "already expired": the first
    /// cancellation checkpoint fails.
    #[serde(alias = "timeout_ms")]
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
            current_color: false,
            output: OutputFormat::Svg,
            emit_css: None,
            max_pixels: 16_000_000,
            max_input_bytes: 8_000_000,
            timeout_ms: None,
        }
    }
}

/// A partial options object: every field optional, unknown keys rejected.
///
/// This is how surfaces apply a user's JSON on top of the defaults. The
/// obvious alternative — serializing the defaults to a `serde_json::Value`,
/// merging the user's keys in, and deserializing back — cannot work once
/// fields accept aliases: a user writing `alpha_mode` would produce an
/// object carrying both `alphaMode` and `alpha_mode`, which is a duplicate
/// field. Patching also produces far better error messages, because the
/// failure is attributed to the key the user actually wrote.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConvertOptionsPatch {
    pub mode: Option<Mode>,
    pub stroke: Option<bool>,
    pub colors: Option<ColorSpec>,
    pub layering: Option<Layering>,
    pub tolerance: Option<f32>,
    pub smoothness: Option<f32>,
    pub turdsize: Option<u32>,
    pub gradients: Option<Tri>,
    pub grouping: Option<Grouping>,
    #[serde(alias = "id_style")]
    pub id_style: Option<IdStyle>,
    #[serde(alias = "transform_origin")]
    pub transform_origin: Option<TOrigin>,
    pub precision: Option<u8>,
    #[serde(alias = "max_trace_dimension")]
    pub max_trace_dimension: Option<u32>,
    pub background: Option<Background>,
    #[serde(alias = "alpha_mode")]
    pub alpha_mode: Option<AlphaMode>,
    pub arcs: Option<bool>,
    #[serde(alias = "current_color")]
    pub current_color: Option<bool>,
    pub output: Option<OutputFormat>,
    #[serde(rename = "css", alias = "emitCss", alias = "emit_css")]
    pub emit_css: Option<Preset>,
    #[serde(alias = "max_pixels")]
    pub max_pixels: Option<u64>,
    #[serde(alias = "max_input_bytes")]
    pub max_input_bytes: Option<u64>,
    #[serde(alias = "timeout_ms")]
    pub timeout_ms: Option<u64>,
}

impl ConvertOptionsPatch {
    /// Apply this patch on top of `base`, returning the merged options.
    ///
    /// Note that `maxTraceDimension` and `timeoutMs` are themselves
    /// `Option` in `ConvertOptions`: a patch that omits them leaves the
    /// base value alone, and there is deliberately no way to write
    /// "explicitly none" — passing `null` is rejected rather than silently
    /// meaning "reset to default".
    pub fn apply(self, base: ConvertOptions) -> ConvertOptions {
        ConvertOptions {
            mode: self.mode.unwrap_or(base.mode),
            stroke: self.stroke.unwrap_or(base.stroke),
            colors: self.colors.unwrap_or(base.colors),
            layering: self.layering.unwrap_or(base.layering),
            tolerance: self.tolerance.unwrap_or(base.tolerance),
            smoothness: self.smoothness.unwrap_or(base.smoothness),
            turdsize: self.turdsize.unwrap_or(base.turdsize),
            gradients: self.gradients.unwrap_or(base.gradients),
            grouping: self.grouping.unwrap_or(base.grouping),
            id_style: self.id_style.unwrap_or(base.id_style),
            transform_origin: self.transform_origin.unwrap_or(base.transform_origin),
            precision: self.precision.unwrap_or(base.precision),
            max_trace_dimension: self.max_trace_dimension.or(base.max_trace_dimension),
            background: self.background.unwrap_or(base.background),
            alpha_mode: self.alpha_mode.unwrap_or(base.alpha_mode),
            arcs: self.arcs.unwrap_or(base.arcs),
            current_color: self.current_color.unwrap_or(base.current_color),
            output: self.output.unwrap_or(base.output),
            emit_css: self.emit_css.or(base.emit_css),
            max_pixels: self.max_pixels.unwrap_or(base.max_pixels),
            max_input_bytes: self.max_input_bytes.unwrap_or(base.max_input_bytes),
            timeout_ms: self.timeout_ms.or(base.timeout_ms),
        }
    }
}

/// Parse a partial options JSON string into full [`ConvertOptions`].
///
/// An empty string or `{}` yields the defaults. This is the single entry
/// point every non-CLI surface uses, so the contract cannot drift between
/// Node, WASM, MCP, and any future HTTP API (issue #2).
pub fn options_from_json(json: &str) -> Result<ConvertOptions, SpryteoError> {
    options_from_json_with_base(json, ConvertOptions::default())
}

/// [`options_from_json`] applied on top of a caller-supplied base.
pub fn options_from_json_with_base(
    json: &str,
    base: ConvertOptions,
) -> Result<ConvertOptions, SpryteoError> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return Ok(base);
    }

    // Reject non-objects with a message that names the actual shape, rather
    // than letting Serde report a confusing per-field error.
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| SpryteoError::InvalidInput(format!("Failed to parse options JSON: {e}")))?;
    if !value.is_object() {
        return Err(SpryteoError::InvalidInput(
            "Failed to parse options JSON: expected a JSON object".to_string(),
        ));
    }

    let patch: ConvertOptionsPatch = serde_json::from_value(value)
        .map_err(|e| SpryteoError::InvalidInput(format!("Failed to parse options JSON: {e}")))?;
    Ok(patch.apply(base))
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
