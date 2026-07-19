//! MCP server exposing the Spryteo engine. Phase 5 -- see /ROADMAP.md §5.5.

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use spryteo_core::{
    ClassifiedInput, Contour, ContourSet, ConvertOptions, ConvertResult, Fill, LayerStack, Mode,
    RasterImage, SpryteoError, Tri,
};

#[derive(Clone)]
struct SpryteoServer {
    tool_router: ToolRouter<SpryteoServer>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ConvertImageArgs {
    /// Base64-encoded raw image bytes (PNG/JPEG/GIF/WebP/BMP).
    image_base64: String,
    /// Optional partial ConvertOptions as a JSON object (e.g. {"stroke": true, "mode": "photo"}).
    /// Omitted fields use ConvertOptions::default().
    #[serde(default)]
    options: Option<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct InspectSvgArgs {
    /// The JSON metadata sidecar (Meta object) returned by convert_image.
    /// Note: This tool accepts the serialized `meta` JSON sidecar, not the raw SVG markup.
    meta_json: String,
}

#[tool_router]
impl SpryteoServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Convert a raster image (PNG, JPEG, GIF, WebP, BMP) to a vectorized SVG with an animation-friendly scene graph and metadata sidecar. Accepts the raw image bytes encoded in base64, plus an optional partial JSON options object (e.g. {\"stroke\": true, \"mode\": \"photo\"})."
    )]
    async fn convert_image(
        &self,
        Parameters(args): Parameters<ConvertImageArgs>,
    ) -> Result<CallToolResult, McpError> {
        match convert_image_inner(&args.image_base64, args.options) {
            Ok(result) => match serde_json::to_string(&result) {
                Ok(json_str) => Ok(CallToolResult::success(vec![ContentBlock::text(json_str)])),
                Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Failed to serialize conversion result: {}",
                    e
                ))])),
            },
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e)])),
        }
    }

    #[tool(
        description = "Inspect the metadata sidecar of a vectorized SVG. Accepts the JSON metadata string (the 'meta' object) returned by convert_image, and returns a human-readable summary of the nodes, groups, centroids, bboxes, areas, and colors."
    )]
    fn inspect_svg(
        &self,
        Parameters(args): Parameters<InspectSvgArgs>,
    ) -> Result<CallToolResult, McpError> {
        match inspect_svg_inner(&args.meta_json) {
            Ok(summary) => Ok(CallToolResult::success(vec![ContentBlock::text(summary)])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e)])),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SpryteoServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Spryteo MCP server for vectorizing raster images into clean, structured, and animateable SVG markup."
                    .to_string(),
            )
    }
}

// ── Pipeline Implementation Helpers (duplicating WASM/CLI patterns) ─────────

fn count_contour_and_children(c: &Contour) -> usize {
    1 + c
        .children
        .iter()
        .map(count_contour_and_children)
        .sum::<usize>()
}

fn should_try_gradients(gradients: &Tri, resolved_mode: &Mode) -> bool {
    match gradients {
        Tri::On => true,
        Tri::Off => false,
        Tri::Auto => matches!(resolved_mode, Mode::Photo),
    }
}

const GRADIENT_LAB_TOLERANCE: f64 = 12.0;
const FIXED_SEED: u64 = 42;

fn build_fills(
    image: &RasterImage,
    layer_stack: &LayerStack,
    contour_set: &ContourSet,
    resolved_mode: &Mode,
    opts: &ConvertOptions,
) -> Vec<Fill> {
    let try_gradients = should_try_gradients(&opts.gradients, resolved_mode);
    let mut fills = Vec::new();
    for (layer, contours) in layer_stack.layers.iter().zip(contour_set.layers.iter()) {
        let fill = if try_gradients {
            spryteo_quant::detect_gradient(image, layer, GRADIENT_LAB_TOLERANCE)
                .unwrap_or(Fill::Solid(layer.color))
        } else {
            Fill::Solid(layer.color)
        };
        let mut count = 0;
        for contour in contours {
            count += count_contour_and_children(contour);
        }
        for _ in 0..count {
            fills.push(fill.clone());
        }
    }
    fills
}

fn run_convert(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;
    let was_jpeg = spryteo_raster::is_jpeg(bytes);

    let classified = spryteo_quant::classify(raster_image, &opts.mode);
    let downscaled_image =
        spryteo_raster::downscale_large_photo(classified.image, &classified.mode);
    let width = downscaled_image.width;
    let height = downscaled_image.height;
    let preprocessed_image =
        spryteo_raster::preprocess(downscaled_image, &classified.mode, was_jpeg);
    let classified = ClassifiedInput {
        image: preprocessed_image,
        mode: classified.mode,
        background_color: classified.background_color,
    };
    let mut layer_stack =
        spryteo_quant::quantize(&classified, &opts.colors, &opts.layering, FIXED_SEED);
    let rect_color = spryteo_quant::apply_background_policy(&mut layer_stack, classified.background_color, &opts.background);
    let contour_set = spryteo_trace::extract_contours(&layer_stack, width, height, opts.turdsize);
    let curve_set = spryteo_fit::fit_contours(&contour_set, opts.tolerance, opts.smoothness);

    let fills = build_fills(
        &classified.image,
        &layer_stack,
        &contour_set,
        &classified.mode,
        opts,
    );
    let scene = spryteo_svg::build_scene_graph(
        &curve_set,
        &opts.id_style,
        &opts.transform_origin,
        &fills,
        opts.arcs,
    );
    let result = spryteo_svg::emit_svg(&scene, width, height, opts, rect_color);
    Ok(result)
}

fn run_convert_stroke(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;
    let width = raster_image.width;
    let height = raster_image.height;
    let stroke_result = spryteo_stroke::trace_stroke(&raster_image, opts.tolerance);
    let scene = spryteo_svg::build_stroke_scene_graph(
        &stroke_result.curves,
        &opts.id_style,
        &stroke_result.widths,
    );
    let result = spryteo_svg::emit_stroke_svg(&scene, width, height, opts);
    Ok(result)
}

// ── Core functions exposed for testing ──────────────────────────────────────

pub fn decode_base64_image(base64_str: &str) -> Result<Vec<u8>, String> {
    let trimmed = base64_str.trim();
    let clean_b64 = if let Some(stripped) = trimmed.strip_prefix("data:") {
        if let Some(comma_idx) = stripped.find(',') {
            &stripped[comma_idx + 1..]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    use base64::{prelude::BASE64_STANDARD, Engine};
    BASE64_STANDARD
        .decode(clean_b64.trim())
        .map_err(|e| format!("Failed to decode base64 image: {}", e))
}

pub fn merge_options(user_val: Option<serde_json::Value>) -> Result<ConvertOptions, String> {
    let mut default_val = serde_json::to_value(ConvertOptions::default())
        .map_err(|e| format!("Failed to serialize default options: {}", e))?;

    if let Some(user_val) = user_val {
        if !user_val.is_object() {
            return Err("Failed to parse options: expected a JSON object".to_string());
        }
        if let (Some(default_obj), Some(user_obj)) =
            (default_val.as_object_mut(), user_val.as_object())
        {
            for (k, v) in user_obj {
                default_obj.insert(k.clone(), v.clone());
            }
        }
    }

    serde_json::from_value::<ConvertOptions>(default_val)
        .map_err(|e| format!("Failed to parse options: {}", e))
}

pub fn convert_image_inner(
    image_base64: &str,
    options: Option<serde_json::Value>,
) -> Result<ConvertResult, String> {
    let bytes = decode_base64_image(image_base64)?;
    let opts = merge_options(options)?;

    let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if opts.stroke {
            run_convert_stroke(&bytes, &opts)
        } else {
            run_convert(&bytes, &opts)
        }
    }));

    match run_result {
        Ok(Ok(res)) => Ok(res),
        Ok(Err(err)) => Err(err.to_string()),
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic".to_string()
            };
            Err(format!("Pipeline panicked: {}", msg))
        }
    }
}

pub fn inspect_svg_inner(meta_json: &str) -> Result<String, String> {
    let meta: spryteo_core::ir::Meta = serde_json::from_str(meta_json)
        .map_err(|e| format!("Failed to parse metadata JSON: {}", e))?;

    let mut summary = String::new();
    summary.push_str("### Spryteo SVG Inspection Summary\n\n");
    summary.push_str(&format!("- **Total Nodes**: {}\n", meta.stats.node_count));
    summary.push_str(&format!("- **Path Count**: {}\n", meta.stats.path_count));
    summary.push_str(&format!(
        "- **Byte Count**: {} bytes\n\n",
        meta.stats.byte_count
    ));

    summary.push_str("#### Nodes List:\n");
    if meta.nodes.is_empty() {
        summary.push_str("_No individual node metadata available._\n");
    } else {
        summary.push_str("| Node ID | Group | Color (sRGB) | Bounding Box (x_min, y_min, x_max, y_max) | Centroid | Area |\n");
        summary.push_str("|---|---|---|---|---|---|\n");
        for node in &meta.nodes {
            let color_str = if let Some(rgb) = node.fill {
                format!("rgb({}, {}, {})", rgb.r, rgb.g, rgb.b)
            } else {
                "None / Stroke".to_string()
            };
            summary.push_str(&format!(
                "| `{}` | {} | {} | ({:.2}, {:.2}, {:.2}, {:.2}) | ({:.2}, {:.2}) | {:.2} |\n",
                node.id,
                node.group,
                color_str,
                node.bbox.x_min,
                node.bbox.y_min,
                node.bbox.x_max,
                node.bbox.y_max,
                node.centroid.0,
                node.centroid.1,
                node.area
            ));
        }
    }
    Ok(summary)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let service = SpryteoServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{prelude::BASE64_STANDARD, Engine};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    fn get_synthetic_png_b64(stroke: bool) -> String {
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        if stroke {
            for i in 5..=25 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let px = (i + dx) as u32;
                        let py = (i + dy) as u32;
                        if px < 32 && py < 32 {
                            img.put_pixel(px, py, Rgba([0, 0, 0, 255]));
                        }
                    }
                }
            }
        } else {
            for y in 0..32 {
                for x in 0..32 {
                    let dx = x as f32 - 15.5;
                    let dy = y as f32 - 15.5;
                    if dx * dx + dy * dy <= 8.0 * 8.0 {
                        img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                    }
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();
        BASE64_STANDARD.encode(&png_bytes)
    }

    #[test]
    fn test_convert_image_valid_base64() {
        let b64 = get_synthetic_png_b64(false);
        let res = convert_image_inner(&b64, None).unwrap();
        assert!(res.svg.contains("<svg"));
        assert!(!res.svg.contains("pathLength="));
    }

    #[test]
    fn test_convert_image_invalid_base64() {
        let res = convert_image_inner("not a valid base64 string!!!", None);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("Failed to decode base64"));
    }

    #[test]
    fn test_convert_image_stroke_routing() {
        let b64 = get_synthetic_png_b64(true);
        let opts = Some(serde_json::json!({
            "stroke": true
        }));
        let res = convert_image_inner(&b64, opts).unwrap();
        assert!(res.svg.contains("pathLength="));
    }

    #[test]
    fn test_inspect_svg_valid() {
        let b64 = get_synthetic_png_b64(false);
        let res = convert_image_inner(&b64, None).unwrap();
        let meta_json = serde_json::to_string(&res.meta).unwrap();

        let summary = inspect_svg_inner(&meta_json).unwrap();
        assert!(summary.contains("Spryteo SVG Inspection Summary"));
        assert!(summary.contains(&format!("Total Nodes**: {}", res.meta.stats.node_count)));
        assert!(summary.contains("| Node ID |"));
    }

    #[test]
    fn test_inspect_svg_malformed_json() {
        let res = inspect_svg_inner("not valid json");
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("Failed to parse metadata JSON"));
    }
}
