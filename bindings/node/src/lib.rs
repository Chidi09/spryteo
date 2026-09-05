//! napi-rs native Node.js addon. Phase 3 implementation.

use napi::bindgen_prelude::{AsyncTask, Buffer};
use napi::{Env, Task};
use napi_derive::napi;
use spryteo_core::{options_from_json, ConvertResult};

/// Core conversion logic returning standard Rust types for testability on native.
pub fn convert_impl(bytes: &[u8], options_json: &str) -> Result<ConvertResult, String> {
    // Options parsing lives in the core so the accepted schema cannot drift
    // between Node, WASM, MCP and the CLI (#2).
    let opts = options_from_json(options_json).map_err(|e| e.to_string())?;

    // The addon is an adapter: Buffer and options string in, JSON out.
    // The pipeline, its limits, cancellation and panic trapping live in
    // the shared engine (#19).
    spryteo_engine::convert_with_timeout(bytes, &opts).map_err(|e| e.to_string())
}

/// Serialize a conversion result, or map the error for JS.
fn convert_to_json(bytes: &[u8], options_str: &str) -> napi::Result<serde_json::Value> {
    match convert_impl(bytes, options_str) {
        Ok(res) => serde_json::to_value(&res)
            .map_err(|e| napi::Error::from_reason(format!("Failed to serialize result: {}", e))),
        Err(err_msg) => Err(napi::Error::from_reason(err_msg)),
    }
}

/// The explicitly blocking API. Runs the whole conversion on the calling
/// JavaScript thread; use [`convert`] unless you want that.
#[napi]
pub fn convert_sync(
    bytes: Buffer,
    options_json: Option<String>,
) -> napi::Result<serde_json::Value> {
    convert_to_json(&bytes, options_json.as_deref().unwrap_or("{}"))
}

/// A conversion queued onto libuv's worker pool.
///
/// Issue #13: `convert` was declared `async` but immediately called
/// `convert_sync`, so every multi-second conversion still blocked the Node
/// event loop despite the Promise-shaped API. `Task::compute` runs on a
/// worker thread instead, and `resolve` marshals the result back on the
/// main thread, so the loop stays responsive.
///
/// The input Buffer is copied into an owned `Vec` because a `Buffer` is
/// tied to the JS heap and cannot be read from another thread.
pub struct ConvertTask {
    bytes: Vec<u8>,
    options_json: String,
}

impl Task for ConvertTask {
    type Output = serde_json::Value;
    type JsValue = napi::JsUnknown;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        convert_to_json(&self.bytes, &self.options_json)
    }

    /// Runs back on the JS thread, where an `Env` exists to build the
    /// object. `serde_json::Value` itself is not a `TypeName`, so the
    /// conversion happens here rather than in the `JsValue` associated
    /// type.
    fn resolve(&mut self, env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        env.to_js_value(&output)
    }
}

/// Convert off the JavaScript event loop, returning a Promise.
///
/// Concurrency is bounded by libuv's thread pool (4 threads by default,
/// set with `UV_THREADPOOL_SIZE`). Conversions beyond that queue rather
/// than oversubscribing the CPU.
#[napi]
pub fn convert(bytes: Buffer, options_json: Option<String>) -> AsyncTask<ConvertTask> {
    AsyncTask::new(ConvertTask {
        bytes: bytes.to_vec(),
        options_json: options_json.unwrap_or_else(|| "{}".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    fn get_synthetic_png_bytes(stroke: bool) -> Vec<u8> {
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        if stroke {
            // Draw diagonal line from (5,5) to (25,25) for stroke mode
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
            // Circle for fill mode
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
        png_bytes
    }

    #[test]
    fn test_empty_options_json() {
        let bytes = get_synthetic_png_bytes(false);

        // Test with empty string
        let res_empty = convert_impl(&bytes, "").unwrap();
        assert!(res_empty.svg.contains("<svg"));
        assert!(!res_empty.svg.contains("pathLength="));

        // Test with "{}"
        let res_braces = convert_impl(&bytes, "{}").unwrap();
        assert!(res_braces.svg.contains("<svg"));
        assert_eq!(res_empty.svg, res_braces.svg);
    }

    #[test]
    fn test_malformed_options_json() {
        let bytes = get_synthetic_png_bytes(false);

        // Not valid JSON
        let res_malformed = convert_impl(&bytes, "{not valid json");
        assert!(res_malformed.is_err());
        assert!(res_malformed
            .err()
            .unwrap()
            .contains("Failed to parse options JSON"));

        // Invalid mode value
        let res_invalid_val = convert_impl(&bytes, r#"{"mode": "not-a-real-mode"}"#);
        assert!(res_invalid_val.is_err());
        assert!(res_invalid_val
            .err()
            .unwrap()
            .contains("Failed to parse options JSON"));

        // Not a JSON object (e.g. an array)
        let res_array = convert_impl(&bytes, "[1, 2, 3]");
        assert!(res_array.is_err());
        assert!(res_array.err().unwrap().contains("expected a JSON object"));
    }

    #[test]
    fn test_garbage_input_bytes() {
        let garbage = b"Not a real image file content at all";
        let res = convert_impl(garbage, "{}");
        assert!(res.is_err());
        let err_msg = res.err().unwrap();
        assert!(err_msg.contains("Invalid") || err_msg.contains("input"));
    }

    #[test]
    fn test_stroke_option_routing() {
        // Assert opts.stroke = true routes to stroke pipeline and contains pathLength
        let bytes_stroke = get_synthetic_png_bytes(true);
        let res_stroke = convert_impl(&bytes_stroke, r#"{"stroke": true}"#).unwrap();
        assert!(res_stroke.svg.contains("pathLength="));

        // Assert opts.stroke = false does not contain pathLength
        let bytes_fill = get_synthetic_png_bytes(false);
        let res_fill = convert_impl(&bytes_fill, r#"{"stroke": false}"#).unwrap();
        assert!(!res_fill.svg.contains("pathLength="));
    }
    /// A red disc, a dark blue disc inside it and a yellow core inside that,
    /// on a white ground: one object built from several colour layers.
    ///
    /// Grouping is what makes such an object animatable as an object rather
    /// than as a handful of unrelated quantization fragments, and #11 asks
    /// for it on every surface, so each surface checks it on this same
    /// fixture.
    const MULTICOLOR_OBJECT: &[u8] = include_bytes!("../../../testdata/multicolor_object_64.png");

    fn assert_multicolor_object_grouped(svg: &str, meta: &spryteo_core::ir::Meta) {
        // The ground stands on its own; the object is a single three-deep
        // tree rather than three siblings.
        assert_eq!(
            meta.groups.len(),
            2,
            "expected the ground and one object, got {:?}",
            meta.groups.iter().map(|g| &g.id).collect::<Vec<_>>()
        );
        assert_eq!(meta.groups[0].depth(), 1, "the ground adopts nothing");
        assert_eq!(meta.groups[1].depth(), 3, "the object nests three deep");

        // And the SVG carries the same shape the metadata describes.
        assert!(
            svg.contains("</g></g></g>"),
            "SVG should close a three-deep group chain: {svg}"
        );
    }

    #[test]
    fn a_multicolor_object_comes_back_as_one_nested_group_tree() {
        let res = convert_impl(MULTICOLOR_OBJECT, "{}").unwrap();
        assert_multicolor_object_grouped(&res.svg, &res.meta);
    }

    /// Every surface must publish the same sidecar contract (#22): the
    /// schema version, the lossless paint, the shape kind and outline
    /// length, and a payload that survives JSON in both directions. A
    /// binding that quietly serialized a reduced `Meta` would look fine
    /// until someone tried to animate from it.
    fn assert_meta_contract(meta: &spryteo_core::ir::Meta) {
        use spryteo_core::ir::{PaintMeta, ShapeKind, META_SCHEMA_VERSION};

        assert_eq!(meta.schema_version, META_SCHEMA_VERSION);
        assert!(!meta.nodes.is_empty());

        for node in &meta.nodes {
            assert!(!node.id.is_empty(), "every node is addressable");
            assert_eq!(
                node.group_path.last(),
                Some(&node.group),
                "the ancestor chain must end at the node's own group"
            );
            let paint = node.paint.as_ref().expect("a filled node records paint");
            match paint {
                PaintMeta::Solid { color } => {
                    assert_eq!(Some(*color), node.fill, "fill is the representative colour")
                }
                other => panic!("this fixture is flat colour, got {other:?}"),
            }
            assert!(node.closed, "traced fill outlines close");
            assert!(node.path_length > 0.0, "{} has no outline length", node.id);
            assert!(
                matches!(
                    node.shape,
                    ShapeKind::Path | ShapeKind::Circle | ShapeKind::Rect
                ),
                "unexpected shape kind {:?}",
                node.shape
            );
        }

        // `suggested_draw_order` is a permutation of the nodes, distinct
        // from paint order and never a partial or repeated ranking.
        let mut ranks: Vec<usize> = meta.nodes.iter().map(|n| n.suggested_draw_order).collect();
        ranks.sort_unstable();
        assert_eq!(ranks, (0..meta.nodes.len()).collect::<Vec<_>>());

        let json = serde_json::to_string(meta).expect("the sidecar serialises");
        let back: spryteo_core::ir::Meta =
            serde_json::from_str(&json).expect("the sidecar deserialises");
        assert_eq!(meta, &back, "the sidecar must survive a JSON round trip");
    }

    #[test]
    fn the_metadata_sidecar_matches_the_published_contract() {
        let res = convert_impl(MULTICOLOR_OBJECT, "{}").unwrap();
        assert_meta_contract(&res.meta);
    }
}
