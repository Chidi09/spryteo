use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use spryteo_core::{ClassifiedInput, ColorSpec, ConvertOptions, Mode};
use std::io::Cursor;
use std::time::Instant;

fn generate_photo(size: u32) -> Vec<u8> {
    let mut img = RgbaImage::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let r = (x as f32 / size as f32 * 255.0) as u8;
            let g = (y as f32 / size as f32 * 255.0) as u8;
            let b = (((x + y) as f32 / (2 * size) as f32) * 255.0) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
    let mut png_bytes = Vec::new();
    DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .unwrap();
    png_bytes
}

fn main() {
    let bytes = generate_photo(1024);
    // Match the browser demo exactly: `convert_default` uses
    // ConvertOptions::default() (Mode::Auto + ColorSpec::Auto). Override
    // via args: `perf_probe n12` uses Photo mode with 12 fixed colors.
    let opts = if std::env::args().any(|a| a == "n12") {
        ConvertOptions {
            mode: Mode::Photo,
            colors: ColorSpec::N(12),
            ..Default::default()
        }
    } else {
        ConvertOptions::default()
    };
    const FIXED_SEED: u64 = 42;

    let t_all = Instant::now();

    let t = Instant::now();
    let raster_image = spryteo_raster::decode(&bytes, &opts).unwrap();
    let was_jpeg = spryteo_raster::is_jpeg(&bytes);
    eprintln!("decode:      {:?}", t.elapsed());

    let t = Instant::now();
    let classified = spryteo_quant::classify(raster_image, &opts.mode);
    eprintln!("classify:    {:?}", t.elapsed());

    let t = Instant::now();
    let downscaled = spryteo_raster::downscale_large_photo(classified.image, &classified.mode);
    let width = downscaled.width;
    let height = downscaled.height;
    let preprocessed = spryteo_raster::preprocess(downscaled, &classified.mode, was_jpeg);
    let classified = ClassifiedInput {
        image: preprocessed,
        mode: classified.mode,
        background_color: classified.background_color,
    };
    eprintln!("preprocess:  {:?}", t.elapsed());

    let t = Instant::now();
    let layer_stack = spryteo_quant::quantize(&classified, &opts.colors, &opts.layering, FIXED_SEED);
    eprintln!("quantize:    {:?}  ({} layers)", t.elapsed(), layer_stack.layers.len());

    let t = Instant::now();
    let contour_set = spryteo_trace::extract_contours(&layer_stack, width, height, opts.turdsize);
    eprintln!("trace:       {:?}", t.elapsed());

    let t = Instant::now();
    let curve_set = spryteo_fit::fit_contours(&contour_set, opts.tolerance, opts.smoothness);
    eprintln!("fit:         {:?}", t.elapsed());

    let t = Instant::now();
    let fills = spryteo_cli::build_fills(
        &classified.image,
        &layer_stack,
        &contour_set,
        &classified.mode,
        &opts,
    );
    eprintln!("fills:       {:?}", t.elapsed());

    let t = Instant::now();
    let scene = spryteo_svg::build_scene_graph(
        &curve_set,
        &opts.id_style,
        &opts.transform_origin,
        &fills,
        opts.arcs,
    );
    let result = spryteo_svg::emit_svg(&scene, width, height, &opts);
    eprintln!("svg:         {:?}", t.elapsed());

    eprintln!("TOTAL:       {:?}", t_all.elapsed());
    eprintln!("svg bytes: {}, nodes: {}", result.svg.len(), result.meta.nodes.len());
}
