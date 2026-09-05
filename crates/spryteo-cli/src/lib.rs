use spryteo_core::{
    CancelToken, ContourSet, ConvertOptions, ConvertResult, Fill, LayerStack, Mode, Preset,
    RasterImage, SpryteoError,
};

pub mod sheet;
pub use sheet::{run_sheet, IconReport, SheetOptions, SheetReport};

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Spryteo(#[from] SpryteoError),

    #[error("Failed to read input file '{path}': {source}")]
    InputIo {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to write output SVG to '{path}': {source}")]
    OutputIo {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to write JSON metadata to '{path}': {source}")]
    JsonIo {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("JSON serialization error: {source}")]
    Json {
        #[from]
        source: serde_json::Error,
    },
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Spryteo(SpryteoError::InvalidInput(_)) => 1,
            CliError::Spryteo(SpryteoError::LimitExceeded { .. }) => 2,
            CliError::Spryteo(SpryteoError::Timeout) => 3,
            CliError::Spryteo(SpryteoError::Cancelled) => 3,
            CliError::Spryteo(SpryteoError::Internal(_)) => 3,
            CliError::InputIo { .. } => 1,
            CliError::OutputIo { .. } => 3,
            CliError::JsonIo { .. } => 3,
            CliError::Json { .. } => 3,
        }
    }
}

pub fn parse_mode(s: &str) -> Result<Mode, String> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(Mode::Auto),
        "icon" => Ok(Mode::Icon),
        "pixel-art" | "pixel_art" | "pixelart" => Ok(Mode::PixelArt),
        "line-art" | "line_art" | "lineart" => Ok(Mode::LineArt),
        "photo" => Ok(Mode::Photo),
        _ => Err(format!(
            "Invalid mode '{}'. Expected one of: auto, icon, pixel-art, line-art, photo",
            s
        )),
    }
}

pub fn parse_preset(s: &str) -> Result<Preset, String> {
    match s.to_lowercase().as_str() {
        "draw" => Ok(Preset::Draw),
        "fade" => Ok(Preset::Fade),
        "pop" => Ok(Preset::Pop),
        _ => Err(format!(
            "Invalid CSS preset '{}'. Expected one of: draw, fade, pop",
            s
        )),
    }
}

/// Panic-to-error mapping, re-exported from the shared engine so the CLI
/// process cannot unwind out of a pipeline bug.
pub use spryteo_engine::catch_pipeline_panic;

/// Decide each layer's paint. Thin wrapper over the engine's `build_fills`
/// with no cancellation, kept for the CLI's existing callers and tests.
pub fn build_fills(
    image: &RasterImage,
    layer_stack: &LayerStack,
    contour_set: &ContourSet,
    resolved_mode: &Mode,
    opts: &ConvertOptions,
) -> Vec<Fill> {
    spryteo_engine::build_fills(
        image,
        layer_stack,
        contour_set,
        resolved_mode,
        opts,
        &CancelToken::none(),
    )
    .expect("build_fills with an inert CancelToken cannot be cancelled")
}

/// Convert bytes to SVG through the shared engine.
///
/// The CLI is an adapter: it owns argument parsing, file IO and process
/// exit codes. Pipeline order, limits, cancellation and error mapping all
/// live in `spryteo-engine` so the four surfaces cannot drift (#19).
pub fn run_convert(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    spryteo_engine::convert_with_timeout(bytes, opts)
}

/// Like `run_convert` but supplies pre-computed semantic masks.
pub fn run_convert_with_masks(
    bytes: &[u8],
    opts: &ConvertOptions,
    masks: &[spryteo_semantic::Mask],
) -> Result<ConvertResult, SpryteoError> {
    let cancel = CancelToken::from_timeout_opt(opts.timeout_ms);
    spryteo_engine::convert_with(
        spryteo_engine::EngineRequest::new(bytes, opts)
            .with_masks(masks)
            .with_cancel(&cancel),
    )
}

/// Centreline (stroke) conversion through the shared engine.
pub fn run_convert_stroke(
    bytes: &[u8],
    opts: &ConvertOptions,
) -> Result<ConvertResult, SpryteoError> {
    let mut stroke_opts = opts.clone();
    stroke_opts.stroke = true;
    spryteo_engine::convert_with_timeout(bytes, &stroke_opts)
}

pub fn run_pipeline(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
    masks: &[spryteo_semantic::Mask],
) -> Result<ConvertResult, CliError> {
    let bytes = std::fs::read(input_path).map_err(|source| CliError::InputIo {
        path: input_path.to_path_buf(),
        source,
    })?;
    run_pipeline_with_bytes(&bytes, output_path, json_path, opts, masks)
}

/// Like `run_pipeline` but accepts already-read bytes.
pub fn run_pipeline_with_bytes(
    bytes: &[u8],
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
    masks: &[spryteo_semantic::Mask],
) -> Result<ConvertResult, CliError> {
    let result = catch_pipeline_panic(|| {
        if opts.stroke {
            run_convert_stroke(bytes, opts)
        } else {
            run_convert_with_masks(bytes, opts, masks)
        }
    })?;

    std::fs::write(output_path, &result.svg).map_err(|source| CliError::OutputIo {
        path: output_path.to_path_buf(),
        source,
    })?;

    if let Some(jpath) = json_path {
        let json_str = serde_json::to_string_pretty(&result.meta)?;
        std::fs::write(jpath, json_str).map_err(|source| CliError::JsonIo {
            path: jpath.to_path_buf(),
            source,
        })?;
    }

    Ok(result)
}

/// Run SAM inference on the decoded raster and return computed masks.
#[cfg(feature = "ml")]
pub fn run_sam_inference(
    bytes: &[u8],
    opts: &ConvertOptions,
    encoder_path: &std::path::Path,
    decoder_path: &std::path::Path,
) -> Result<Vec<spryteo_semantic::Mask>, CliError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;

    let mut model = spryteo_semantic::sam::SamModel::load(encoder_path, decoder_path)
        .map_err(|e| CliError::Spryteo(SpryteoError::Internal(e.to_string())))?;
    let embedding = model
        .embed(&raster_image)
        .map_err(|e| CliError::Spryteo(SpryteoError::Internal(e.to_string())))?;
    let sam_opts = spryteo_semantic::sam::AutoMaskOptions::default();
    let masks = model
        .segment_everything(&raster_image, &embedding, &sam_opts)
        .map_err(|e| CliError::Spryteo(SpryteoError::Internal(e.to_string())))?;
    Ok(masks)
}

/// Run the full pipeline with automatic SAM mask generation.
#[cfg(feature = "ml")]
pub fn run_pipeline_sam(
    bytes: &[u8],
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
    encoder_path: &std::path::Path,
    decoder_path: &std::path::Path,
) -> Result<ConvertResult, CliError> {
    let masks = run_sam_inference(bytes, opts, encoder_path, decoder_path)?;
    run_pipeline_with_bytes(bytes, output_path, json_path, opts, &masks)
}

/// Render a human-readable report from a `Meta` sidecar (as written by
/// `convert --json`), for `spryteo inspect --meta`.
pub fn format_meta_report(meta: &spryteo_core::Meta) -> String {
    use spryteo_core::ir::PaintMeta;

    let mut out = String::new();
    out.push_str(&format!(
        "schema={} nodes={} paths={} bytes={}\n\n",
        meta.schema_version, meta.stats.node_count, meta.stats.path_count, meta.stats.byte_count
    ));
    for n in &meta.nodes {
        // A gradient is named rather than flattened to its first stop, which
        // would read as a solid fill the SVG does not have.
        let fill_str = match &n.paint {
            Some(PaintMeta::Solid { color }) => {
                format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
            }
            Some(PaintMeta::CurrentColor { .. }) => "currentColor".to_string(),
            Some(PaintMeta::LinearGradient { stops, .. }) => format!("linear({})", stops.len()),
            Some(PaintMeta::RadialGradient { stops, .. }) => format!("radial({})", stops.len()),
            None => match n.fill {
                Some(rgb) => format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b),
                None => "-".to_string(),
            },
        };
        let shape_str = format!(
            "{}{}",
            format!("{:?}", n.shape).to_lowercase(),
            if n.closed { "" } else { "/open" }
        );
        out.push_str(&format!(
            "{:<16} group={:<12} z={:<4} {:<12} fill={:<12} bbox=({:.1},{:.1},{:.1},{:.1}) centroid=({:.1},{:.1}) area={:.1} len={:.1} draw_order={}\n",
            n.id,
            n.group,
            n.z_order,
            shape_str,
            fill_str,
            n.bbox.x_min,
            n.bbox.y_min,
            n.bbox.x_max,
            n.bbox.y_max,
            n.centroid.0,
            n.centroid.1,
            n.area,
            n.path_length,
            n.suggested_draw_order,
        ));
    }
    out
}

fn extract_view_box(svg: &str) -> Option<&str> {
    let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
    let rest = &svg[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Information about a single shape, re-derived from SVG markup.
struct ParsedShape {
    id: Option<String>,
    parent_group: Option<String>,
    fill: Option<String>,
    bbox: Option<(f64, f64, f64, f64)>,
    centroid: Option<(f64, f64)>,
}

/// Extract the value of `name="..."` from inside an SVG tag (without the
/// leading `<` and tag name — the attribute portion only).
///
/// Requires the match to be preceded by a space or be at position 0, so
/// that searching for `d="` does not falsely match inside `id="s-1"`.
fn attr_value<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let pattern = format!("{}=\"", name);
    let bytes = content.as_bytes();
    let mut search_start = 0;
    loop {
        let remainder = &content[search_start..];
        let rel = remainder.find(&pattern)?;
        let abs = search_start + rel;
        if abs == 0 || bytes[abs - 1] == b' ' {
            let value_start = abs + pattern.len();
            let rest = &content[value_start..];
            let end = rest.find('"')?;
            return Some(&rest[..end]);
        }
        search_start = abs + 1;
    }
}

/// Parse `translate(x, y)` from a `transform` attribute value.
fn parse_translate(transform: Option<&str>) -> (f64, f64) {
    let t = match transform {
        Some(s) => s,
        None => return (0.0, 0.0),
    };
    if let Some(args) = t.strip_prefix("translate(") {
        if let Some(end) = args.find(')') {
            let coords = &args[..end];
            if let Some(comma_pos) = coords.find(',') {
                let x = coords[..comma_pos].trim().parse().unwrap_or(0.0);
                let y = coords[comma_pos + 1..].trim().parse().unwrap_or(0.0);
                return (x, y);
            }
            // fallback: space-separated
            let mut parts = coords.split_whitespace();
            let x = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            return (x, y);
        }
    }
    (0.0, 0.0)
}

/// Compute the axis-aligned bounding box of a path's `d` attribute string.
///
/// Parses M, L, C, A, and Z commands.  For cubic Beziers (C), control-point
/// coordinates are included so the returned bbox is a safe over-estimate
/// (never smaller than the true curve extent).  A tight Bezier bbox
/// requires derivative root-finding and is out of scope for this lightweight
/// re-derivation.
fn parse_path_d(d: &str) -> (f64, f64, f64, f64) {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    let mut update = |x: f64, y: f64| {
        if x < x_min {
            x_min = x;
        }
        if x > x_max {
            x_max = x;
        }
        if y < y_min {
            y_min = y;
        }
        if y > y_max {
            y_max = y;
        }
    };

    let tokens: Vec<&str> = d.split_whitespace().collect();
    let mut i = 0;
    let mut cmd = ' ';

    while i < tokens.len() {
        let token = tokens[i];
        let first = token.chars().next().unwrap_or(' ');
        if first.is_ascii_alphabetic() {
            cmd = first;
            i += 1;
            continue;
        }
        match cmd {
            'M' | 'L' | 'm' | 'l' => {
                if i + 1 < tokens.len() {
                    if let (Ok(x), Ok(y)) = (tokens[i].parse(), tokens[i + 1].parse()) {
                        update(x, y);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            'C' | 'c' => {
                if i + 5 < tokens.len() {
                    for j in 0..3 {
                        if let (Ok(x), Ok(y)) =
                            (tokens[i + j * 2].parse(), tokens[i + j * 2 + 1].parse())
                        {
                            update(x, y);
                        }
                    }
                    i += 6;
                } else {
                    i += 1;
                }
            }
            'A' | 'a' => {
                if i + 6 < tokens.len() {
                    if let (Ok(x), Ok(y)) = (tokens[i + 5].parse(), tokens[i + 6].parse()) {
                        update(x, y);
                    }
                    i += 7;
                } else {
                    i += 1;
                }
            }
            'Z' | 'z' => {
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if x_min.is_infinite() {
        x_min = 0.0;
    }
    if x_max.is_infinite() {
        x_max = 0.0;
    }
    if y_min.is_infinite() {
        y_min = 0.0;
    }
    if y_max.is_infinite() {
        y_max = 0.0;
    }

    (x_min, y_min, x_max, y_max)
}

/// Walk the SVG text once, linearly, and for every shape element (circle,
/// ellipse, rect, path) in document order extract id, parent group, fill,
/// bounding box, and bbox-centroid approximation.
///
/// Group nesting is tracked via a stack pushed/popped on `<g>` / `</g>`.
/// The centoid reported here is a bbox-centre approximation, NOT the true
/// area-weighted geometric centroid (which would require integrating the
/// fitted path/primitive geometry).  Area is not computed.
fn parse_svg_shapes(svg: &str) -> Vec<ParsedShape> {
    let mut shapes = Vec::new();
    let mut group_stack: Vec<String> = Vec::new();
    let chars: Vec<char> = svg.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        if chars[i] != '<' {
            i += 1;
            continue;
        }
        i += 1; // skip '<'
        if i >= n {
            break;
        }

        // Closing tag: </g>
        if chars[i] == '/' {
            i += 1;
            let tag_start = i;
            while i < n && chars[i] != '>' {
                i += 1;
            }
            let tag: String = chars[tag_start..i].iter().collect();
            let tag_name = tag
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or("");
            if tag_name == "g" {
                group_stack.pop();
            }
            if i < n {
                i += 1;
            }
            continue;
        }

        // Read tag name
        let tag_start = i;
        while i < n && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' {
            i += 1;
        }
        let tag_name: String = chars[tag_start..i].iter().collect();

        // Read rest of the tag (attributes) until '>'
        let attr_start = i;
        while i < n && chars[i] != '>' {
            i += 1;
        }
        let attr_content: String = chars[attr_start..i].iter().collect();
        if i < n {
            i += 1;
        } // skip '>'

        match tag_name.as_str() {
            "g" => {
                let gid = attr_value(&attr_content, "id").map(|s| s.to_string());
                group_stack.push(gid.unwrap_or_default());
            }
            "circle" | "ellipse" | "rect" | "path" => {
                let sid = attr_value(&attr_content, "id").map(|s| s.to_string());
                let fill = attr_value(&attr_content, "fill").map(|s| s.to_string());
                let transform = attr_value(&attr_content, "transform");
                let translate = parse_translate(transform);
                let parent_group = group_stack.last().filter(|g| !g.is_empty()).cloned();

                let bbox = match tag_name.as_str() {
                    "circle" => {
                        let cx: f64 = attr_value(&attr_content, "cx")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let cy: f64 = attr_value(&attr_content, "cy")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let r: f64 = attr_value(&attr_content, "r")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        Some((cx - r, cy - r, cx + r, cy + r))
                    }
                    "ellipse" => {
                        let cx: f64 = attr_value(&attr_content, "cx")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let cy: f64 = attr_value(&attr_content, "cy")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let rx: f64 = attr_value(&attr_content, "rx")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let ry: f64 = attr_value(&attr_content, "ry")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        Some((cx - rx, cy - ry, cx + rx, cy + ry))
                    }
                    "rect" => {
                        let x: f64 = attr_value(&attr_content, "x")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let y: f64 = attr_value(&attr_content, "y")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let w: f64 = attr_value(&attr_content, "width")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let h: f64 = attr_value(&attr_content, "height")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        Some((x, y, x + w, y + h))
                    }
                    "path" => attr_value(&attr_content, "d").map(parse_path_d),
                    _ => None,
                };

                let bbox = bbox.map(|(x1, y1, x2, y2)| {
                    (
                        x1 + translate.0,
                        y1 + translate.1,
                        x2 + translate.0,
                        y2 + translate.1,
                    )
                });

                let centroid = bbox.map(|(x1, y1, x2, y2)| ((x1 + x2) / 2.0, (y1 + y2) / 2.0));

                shapes.push(ParsedShape {
                    id: sid,
                    parent_group,
                    fill,
                    bbox,
                    centroid,
                });
            }
            _ => {}
        }
    }

    shapes
}

/// Re-derive a per-shape summary directly from SVG markup, for
/// `spryteo inspect` when no `--meta` sidecar path is given.
///
/// bbox, fill, and immediate-parent-group membership are mechanically
/// recovered from the shape attributes and group nesting in the markup.
/// The centroid shown is a bbox-centre approximation (not the true
/// area-weighted centroid).  Area and exact draw-order (z-order) are not
/// recoverable without the --meta sidecar (they require path-geometry
/// integration and the internal per-layer stacking order).
pub fn derive_svg_summary(svg: &str) -> String {
    let path_count = svg.matches("<path").count();
    let group_count = svg.matches("<g ").count() + svg.matches("<g>").count();
    let view_box = extract_view_box(svg).unwrap_or("(none)");
    let shapes = parse_svg_shapes(svg);

    let mut out = String::new();
    out.push_str("no --meta sidecar given; bbox/fill/group re-derived from SVG markup\n");
    out.push_str("(centroid is a bbox-centre approximation; area and true draw-order are not\n");
    out.push_str(" recoverable without --meta)\n");
    out.push_str(&format!(
        "paths={} groups={} bytes={} viewBox={}\n\n",
        path_count,
        group_count,
        svg.len(),
        view_box
    ));

    for s in &shapes {
        let id_str = s.id.as_deref().unwrap_or("-");
        let group_str = s.parent_group.as_deref().unwrap_or("-");
        let fill_str = s.fill.as_deref().unwrap_or("-");
        match s.bbox {
            Some((x1, y1, x2, y2)) => {
                let (cx, cy) = s.centroid.unwrap_or((0.0, 0.0));
                out.push_str(&format!(
                    "{:<16} group={:<12} fill={:<9} bbox=({:.1},{:.1},{:.1},{:.1}) centroid=({:.1},{:.1})\n",
                    id_str, group_str, fill_str, x1, y1, x2, y2, cx, cy,
                ));
            }
            None => {
                out.push_str(&format!(
                    "{:<16} group={:<12} fill={:<9} bbox=- centroid=-\n",
                    id_str, group_str, fill_str,
                ));
            }
        }
    }

    if shapes.is_empty() {
        out.push_str("(no shapes found)\n");
    }

    out
}
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
