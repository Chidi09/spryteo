//! napi-rs native Node.js addon. Phase 3 implementation.

use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use spryteo_core::{
    ClassifiedInput, Contour, ContourSet, ConvertOptions, ConvertResult, Fill, LayerStack, Mode,
    RasterImage, SpryteoError, Tri,
};

fn count_contour_and_children(c: &Contour) -> usize {
    1 + c
        .children
        .iter()
        .map(count_contour_and_children)
        .sum::<usize>()
}

/// Whether gradient detection should be attempted for the resolved input mode,
/// per `ConvertOptions.gradients`: `On` always tries, `Off` never does,
/// `Auto` tries only for `Mode::Photo` (icons keep flat fills).
fn should_try_gradients(gradients: &Tri, resolved_mode: &Mode) -> bool {
    match gradients {
        Tri::On => true,
        Tri::Off => false,
        Tri::Auto => matches!(resolved_mode, Mode::Photo),
    }
}

/// Residual tolerance (Lab distance units) for gradient-vs-flat-fill
/// detection -- deliberately NOT `ConvertOptions.tolerance` (a geometric
/// curve-fit budget in pixels); see spryteo-cli's identical constant for
/// the full rationale (confirmed via a real end-to-end CLI test that 0.5
/// silently rejects every gradient, including a clean synthetic one).
const GRADIENT_LAB_TOLERANCE: f64 = 12.0;

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

const FIXED_SEED: u64 = 42;

fn run_convert(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;
    let width = raster_image.width;
    let height = raster_image.height;
    let was_jpeg = spryteo_raster::is_jpeg(bytes);

    let classified = spryteo_quant::classify(raster_image, &opts.mode);
    let preprocessed_image =
        spryteo_raster::preprocess(classified.image, &classified.mode, was_jpeg);
    let classified = ClassifiedInput {
        image: preprocessed_image,
        mode: classified.mode,
        background_color: classified.background_color,
    };
    let layer_stack =
        spryteo_quant::quantize(&classified, &opts.colors, &opts.layering, FIXED_SEED);
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
    let result = spryteo_svg::emit_svg(&scene, width, height, opts);
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

/// Core conversion logic returning standard Rust types for testability on native.
pub fn convert_impl(bytes: &[u8], options_json: &str) -> Result<ConvertResult, String> {
    let options_json_trimmed = options_json.trim();
    let opts = if options_json_trimmed.is_empty() || options_json_trimmed == "{}" {
        ConvertOptions::default()
    } else {
        // Parse the user options into a serde_json::Value
        let user_val: serde_json::Value = serde_json::from_str(options_json)
            .map_err(|e| format!("Failed to parse options JSON: {}", e))?;

        // If it's not an object, it's invalid
        if !user_val.is_object() {
            return Err("Failed to parse options JSON: expected a JSON object".to_string());
        }

        // Serialize default options to a Value
        let mut default_val = serde_json::to_value(ConvertOptions::default())
            .map_err(|e| format!("Failed to serialize default options: {}", e))?;

        // Merge user_val into default_val
        if let (Some(default_obj), Some(user_obj)) =
            (default_val.as_object_mut(), user_val.as_object())
        {
            for (k, v) in user_obj {
                default_obj.insert(k.clone(), v.clone());
            }
        }

        // Deserialize back to ConvertOptions
        serde_json::from_value::<ConvertOptions>(default_val)
            .map_err(|e| format!("Failed to parse options JSON: {}", e))?
    };

    let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if opts.stroke {
            run_convert_stroke(bytes, &opts)
        } else {
            run_convert(bytes, &opts)
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

#[napi]
pub fn convert_sync(
    bytes: Buffer,
    options_json: Option<String>,
) -> napi::Result<serde_json::Value> {
    let options_str = options_json.as_deref().unwrap_or("{}");
    match convert_impl(&bytes, options_str) {
        Ok(res) => serde_json::to_value(&res)
            .map_err(|e| napi::Error::from_reason(format!("Failed to serialize result: {}", e))),
        Err(err_msg) => Err(napi::Error::from_reason(err_msg)),
    }
}

#[napi]
pub async fn convert(
    bytes: Buffer,
    options_json: Option<String>,
) -> napi::Result<serde_json::Value> {
    convert_sync(bytes, options_json)
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
}
