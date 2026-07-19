use spryteo_core::{
    Contour, ContourSet, ConvertOptions, ConvertResult, Fill, LayerStack, Mode, Preset,
    SpryteoError,
};

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

fn count_contour_and_children(c: &Contour) -> usize {
    1 + c
        .children
        .iter()
        .map(count_contour_and_children)
        .sum::<usize>()
}

pub fn build_fills(layer_stack: &LayerStack, contour_set: &ContourSet) -> Vec<Fill> {
    let mut fills = Vec::new();
    for (layer, contours) in layer_stack.layers.iter().zip(contour_set.layers.iter()) {
        let mut count = 0;
        for contour in contours {
            count += count_contour_and_children(contour);
        }
        for _ in 0..count {
            fills.push(Fill::Solid(layer.color));
        }
    }
    fills
}

const FIXED_SEED: u64 = 42;

pub fn run_convert(bytes: &[u8], opts: &ConvertOptions) -> Result<ConvertResult, SpryteoError> {
    let raster_image = spryteo_raster::decode(bytes, opts)?;
    let width = raster_image.width;
    let height = raster_image.height;

    let classified = spryteo_quant::classify(raster_image, &opts.mode);
    let layer_stack = spryteo_quant::quantize(&classified, &opts.colors, FIXED_SEED);
    let contour_set = spryteo_trace::extract_contours(&layer_stack, width, height, opts.turdsize);
    let curve_set = spryteo_fit::fit_contours(&contour_set, opts.tolerance, opts.smoothness);

    let fills = build_fills(&layer_stack, &contour_set);
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

pub fn run_convert_stroke(
    bytes: &[u8],
    opts: &ConvertOptions,
) -> Result<ConvertResult, SpryteoError> {
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

pub fn run_pipeline(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    json_path: Option<&std::path::Path>,
    opts: &ConvertOptions,
) -> Result<ConvertResult, CliError> {
    let bytes = std::fs::read(input_path).map_err(|source| CliError::InputIo {
        path: input_path.to_path_buf(),
        source,
    })?;

    let result = if opts.stroke {
        run_convert_stroke(&bytes, opts)?
    } else {
        run_convert(&bytes, opts)?
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fills_building() {
        use spryteo_core::ir::{Contour, ContourSet, Fill, Layer, LayerStack, Rgb};

        let red = Rgb { r: 255, g: 0, b: 0 };
        let green = Rgb { r: 0, g: 255, b: 0 };

        let layer0 = Layer {
            mask: vec![],
            color: red,
            z_order: 0,
        };
        let layer1 = Layer {
            mask: vec![],
            color: green,
            z_order: 1,
        };

        let layer_stack = LayerStack {
            layers: vec![layer0, layer1],
        };

        let contour_b = Contour {
            points: vec![],
            children: vec![],
        };
        let contour_a = Contour {
            points: vec![],
            children: vec![contour_b],
        };
        let contour_c = Contour {
            points: vec![],
            children: vec![],
        };
        let contour_d = Contour {
            points: vec![],
            children: vec![],
        };

        let contour_set = ContourSet {
            layers: vec![vec![contour_a], vec![contour_c, contour_d]],
        };

        let fills = build_fills(&layer_stack, &contour_set);
        assert_eq!(fills.len(), 4);
        assert_eq!(fills[0], Fill::Solid(red));
        assert_eq!(fills[1], Fill::Solid(red));
        assert_eq!(fills[2], Fill::Solid(green));
        assert_eq!(fills[3], Fill::Solid(green));
    }

    #[test]
    fn test_e2e_pipeline_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with red circle
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        for y in 0..32 {
            for x in 0..32 {
                let dx = x as f32 - 15.5;
                let dy = y as f32 - 15.5;
                if dx * dx + dy * dy <= 8.0 * 8.0 {
                    img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                }
            }
        }

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let opts = ConvertOptions::default();
        let res1 = run_convert(&png_bytes, &opts).unwrap();

        assert!(res1.svg.contains("<svg"));
        assert!(res1.svg.contains("viewBox="));
        assert!(res1.svg.contains("<path") || res1.svg.contains("<circle"));

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
        assert!(!res1.svg.is_empty());

        let res2 = run_convert(&png_bytes, &opts).unwrap();
        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_garbage_input() {
        let garbage = b"Not a real image file content at all";
        let opts = ConvertOptions::default();
        let res = run_convert(garbage, &opts);

        assert!(res.is_err());
        match res.err().unwrap() {
            SpryteoError::InvalidInput(_) => {}
            other => panic!("Expected SpryteoError::InvalidInput, got {:?}", other),
        }
    }

    #[test]
    fn test_e2e_pipeline_stroke_synthetic_image() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use std::io::Cursor;

        // Generate synthetic PNG: 32x32 white background with black line
        let mut img = RgbaImage::new(32, 32);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }
        // Draw diagonal line from (5,5) to (25,25)
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

        let mut png_bytes = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .unwrap();

        let opts = ConvertOptions {
            stroke: true,
            ..ConvertOptions::default()
        };
        let res1 = run_convert_stroke(&png_bytes, &opts).unwrap();

        assert!(res1.svg.contains("<svg"));
        assert!(res1.svg.contains("viewBox="));
        assert!(res1.svg.contains("pathLength=\"100\""));
        assert!(res1.svg.contains("fill=\"none\""));

        assert!(
            res1.meta.stats.path_count <= 3,
            "Path count was {}",
            res1.meta.stats.path_count
        );

        let open_brackets = res1.svg.matches('<').count();
        let close_brackets = res1.svg.matches('>').count();
        assert_eq!(open_brackets, close_brackets);
        assert!(!res1.svg.is_empty());

        let res2 = run_convert_stroke(&png_bytes, &opts).unwrap();
        assert_eq!(res1.svg, res2.svg);
    }

    #[test]
    fn test_garbage_input_stroke() {
        let garbage = b"Not a real image file content at all";
        let opts = ConvertOptions {
            stroke: true,
            ..ConvertOptions::default()
        };
        let res = run_convert_stroke(garbage, &opts);

        assert!(res.is_err());
        match res.err().unwrap() {
            SpryteoError::InvalidInput(_) => {}
            other => panic!("Expected SpryteoError::InvalidInput, got {:?}", other),
        }
    }
}
