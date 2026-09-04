//! MCP server exposing the Spryteo engine. Phase 5 -- see /ROADMAP.md §5.5.

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use spryteo_core::{ConvertOptions, ConvertResult};

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

    // The MCP server is an adapter: base64 in, options JSON in, serialized
    // result out. Stroke/fill dispatch, limits, cancellation, panic
    // trapping and error mapping all live in the shared engine (#19).
    spryteo_engine::convert_with_timeout(&bytes, &opts).map_err(|e| e.to_string())
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
#[path = "main_tests.rs"]
mod tests;
