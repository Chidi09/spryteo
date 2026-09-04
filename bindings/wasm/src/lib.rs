//! wasm-bindgen browser build.

use spryteo_core::{CancelToken, Clock, ConvertOptions, ConvertResult};
use spryteo_sheet::{PipelineOptions, SheetOutcome};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// A wasm-bindgen start function to set up console panic hook.
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Millisecond clock for the browser.
///
/// `std::time::Instant::now()` panics on `wasm32-unknown-unknown`, so the
/// engine's default `StdClock` is unavailable here. `Date::now()` is
/// wall-clock and can in principle step backwards, so readings are clamped
/// to be monotonic — a deadline must never un-expire.
struct BrowserClock {
    origin_ms: f64,
    last: std::sync::atomic::AtomicU64,
}

impl BrowserClock {
    fn new() -> Self {
        Self {
            origin_ms: js_sys::Date::now(),
            last: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Clock for BrowserClock {
    fn now_ms(&self) -> u64 {
        let elapsed = (js_sys::Date::now() - self.origin_ms).max(0.0) as u64;
        let prev = self.last.load(std::sync::atomic::Ordering::Relaxed);
        if elapsed < prev {
            return prev;
        }
        self.last
            .store(elapsed, std::sync::atomic::Ordering::Relaxed);
        elapsed
    }
}

/// Build the cancellation token for a conversion, honouring `timeout_ms`
/// on the browser clock.
fn wasm_cancel_token(opts: &ConvertOptions) -> CancelToken {
    match opts.timeout_ms {
        Some(ms) => CancelToken::with_timeout(Arc::new(BrowserClock::new()), ms),
        None => CancelToken::none(),
    }
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

    // The WASM binding is an adapter: bytes and an options string in,
    // a JsValue out. The pipeline itself lives in the shared engine, with
    // the browser clock supplied for deadline enforcement (#19, #3).
    let cancel = wasm_cancel_token(&opts);
    spryteo_engine::convert_with(
        spryteo_engine::EngineRequest::new(bytes, &opts).with_cancel(&cancel),
    )
    .map_err(|e| e.to_string())
}

/// The main exported function, exposed via `#[wasm_bindgen]`
#[wasm_bindgen]
pub fn convert(bytes: &[u8], options_json: &str) -> Result<JsValue, JsValue> {
    match convert_impl(bytes, options_json) {
        Ok(res) => {
            serde_wasm_bindgen::to_value(&res).map_err(|e| JsValue::from_str(&e.to_string()))
        }
        Err(err_msg) => Err(JsValue::from_str(&err_msg)),
    }
}

/// A second, simpler convenience export equivalent to calling `convert(bytes, "{}")`.
#[wasm_bindgen]
pub fn convert_default(bytes: &[u8]) -> Result<JsValue, JsValue> {
    convert(bytes, "{}")
}

/// Core sheet conversion logic returning standard Rust types for testability on native.
pub fn convert_sheet_impl(bytes: &[u8], options_json: &str) -> Result<SheetOutcome, String> {
    let options_json_trimmed = options_json.trim();
    let cfg = if options_json_trimmed.is_empty() || options_json_trimmed == "{}" {
        PipelineOptions::default()
    } else {
        // Parse the user options into a serde_json::Value
        let user_val: serde_json::Value = serde_json::from_str(options_json)
            .map_err(|e| format!("Failed to parse options JSON: {}", e))?;

        // If it's not an object, it's invalid
        if !user_val.is_object() {
            return Err("Failed to parse options JSON: expected a JSON object".to_string());
        }

        // Serialize default options to a Value
        let mut default_val = serde_json::to_value(PipelineOptions::default())
            .map_err(|e| format!("Failed to serialize default options: {}", e))?;

        // Merge user_val into default_val
        if let (Some(default_obj), Some(user_obj)) =
            (default_val.as_object_mut(), user_val.as_object())
        {
            for (k, v) in user_obj {
                default_obj.insert(k.clone(), v.clone());
            }
        }

        // Deserialize back to PipelineOptions
        serde_json::from_value::<PipelineOptions>(default_val)
            .map_err(|e| format!("Failed to parse options JSON: {}", e))?
    };

    let convert_opts = ConvertOptions::default();

    let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        spryteo_sheet::run_sheet_pipeline(bytes, &convert_opts, &cfg)
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

/// The sheet mode exported function, exposed via `#[wasm_bindgen]`
#[wasm_bindgen]
pub fn convert_sheet(bytes: &[u8], options_json: &str) -> Result<JsValue, JsValue> {
    match convert_sheet_impl(bytes, options_json) {
        Ok(res) => {
            serde_wasm_bindgen::to_value(&res).map_err(|e| JsValue::from_str(&e.to_string()))
        }
        Err(err_msg) => Err(JsValue::from_str(&err_msg)),
    }
}

/// A convenience export for sheet mode equivalent to calling `convert_sheet(bytes, "{}")`.
#[wasm_bindgen]
pub fn convert_sheet_default(bytes: &[u8]) -> Result<JsValue, JsValue> {
    convert_sheet(bytes, "{}")
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

    fn get_synthetic_sheet_png_bytes() -> Vec<u8> {
        let w = 160u32;
        let h = 80u32;
        let mut img = RgbaImage::new(w, h);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        let black = Rgba([0, 0, 0, 255]);

        // Icon 1: Square outline (~24px) centered at x: 20..44, y: 28..52
        for x in 20..=44 {
            for y in 28..=52 {
                if x <= 22 || x >= 42 || y <= 30 || y >= 50 {
                    img.put_pixel(x, y, black);
                }
            }
        }

        // Icon 2: X shape (~24px) centered at x: 110..134, y: 28..52
        for i in 0..=24 {
            for t in 0..=2 {
                let x1 = 110 + i;
                let y1 = 28 + i + t;
                if x1 < w && y1 < h {
                    img.put_pixel(x1, y1, black);
                }
                let x2 = 110 + i;
                let y2 = if 52 >= i + t { 52 - i - t } else { 0 };
                if x2 < w && y2 < h {
                    img.put_pixel(x2, y2, black);
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
    #[allow(clippy::len_zero)]
    fn test_sheet_conversion() {
        let bytes = get_synthetic_sheet_png_bytes();
        let outcome = convert_sheet_impl(&bytes, "{}").expect("convert_sheet_impl failed");
        assert!(
            outcome.icons.len() >= 1,
            "expected outcome.icons.len() >= 1, got 0"
        );
        for icon in &outcome.icons {
            assert!(
                icon.svg.starts_with("<svg"),
                "expected SVG to start with <svg, got {}",
                &icon.svg[..icon.svg.len().min(20)]
            );
        }
        assert_eq!(outcome.report.written, outcome.icons.len());
    }

    #[test]
    fn test_sheet_malformed_options_json() {
        let bytes = get_synthetic_sheet_png_bytes();

        let res_malformed = convert_sheet_impl(&bytes, "{not json");
        assert!(res_malformed.is_err());
        assert!(res_malformed
            .err()
            .unwrap()
            .contains("Failed to parse options JSON"));

        let res_array = convert_sheet_impl(&bytes, "[1,2]");
        assert!(res_array.is_err());
        assert!(res_array.err().unwrap().contains("expected a JSON object"));
    }

    #[test]
    fn test_sheet_garbage_bytes() {
        let garbage = b"Not a real image file content at all";
        let res = convert_sheet_impl(garbage, "{}");
        assert!(res.is_err());
    }

    #[test]
    fn test_sheet_options_plumbing() {
        let bytes = get_synthetic_sheet_png_bytes();
        let outcome = convert_sheet_impl(&bytes, r#"{"name_prefix": "glyph"}"#)
            .expect("convert_sheet_impl failed");
        assert!(!outcome.icons.is_empty());
        for icon in &outcome.icons {
            assert!(
                icon.name.starts_with("glyph-"),
                "expected icon name to start with 'glyph-', got {}",
                icon.name
            );
        }
    }
}
