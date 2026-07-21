use clap::{Args, Parser};
#[cfg(feature = "ml")]
use spryteo_cli::run_pipeline_sam;
use spryteo_cli::{
    derive_svg_summary, format_meta_report, parse_mode, parse_preset, run_pipeline_with_bytes,
};
use spryteo_core::{
    AlphaMode, Background, ColorSpec, ConvertOptions, Grouping, IdStyle, Layering, Meta,
    OutputFormat, Preset, Rgb, TOrigin, Tri,
};

fn parse_masks(path: &std::path::Path) -> Result<Vec<spryteo_semantic::Mask>, String> {
    let content = std::fs::read(path)
        .map_err(|e| format!("Failed to read masks file '{}': {}", path.display(), e))?;
    let masks: Vec<spryteo_semantic::Mask> = serde_json::from_slice(&content)
        .map_err(|e| format!("Invalid masks JSON in '{}': {}", path.display(), e))?;
    for mask in &masks {
        let expected = (mask.width as usize) * (mask.height as usize);
        if mask.pixels.len() != expected {
            return Err(format!(
                "Mask '{}': width*height ({}) does not match pixels length ({})",
                mask.id,
                expected,
                mask.pixels.len()
            ));
        }
    }
    Ok(masks)
}

fn parse_layering(s: &str) -> Result<Layering, String> {
    match s.to_lowercase().as_str() {
        "stacked" => Ok(Layering::Stacked),
        "cutout" => Ok(Layering::Cutout),
        _ => Err(format!(
            "Invalid layering '{}'. Expected one of: stacked, cutout",
            s
        )),
    }
}

fn parse_gradients(s: &str) -> Result<Tri, String> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(Tri::Auto),
        "on" => Ok(Tri::On),
        "off" => Ok(Tri::Off),
        _ => Err(format!(
            "Invalid gradients value '{}'. Expected one of: auto, on, off",
            s
        )),
    }
}

fn parse_background(s: &str) -> Result<Background, String> {
    match s.to_lowercase().as_str() {
        "keep" => Ok(Background::Keep),
        "drop" => Ok(Background::Drop),
        "rect" => Ok(Background::Rect),
        _ => Err(format!(
            "Invalid background '{}'. Expected one of: keep, drop, rect",
            s
        )),
    }
}

fn parse_grouping(s: &str) -> Result<Grouping, String> {
    match s.to_lowercase().as_str() {
        "component" => Ok(Grouping::Component),
        "semantic" => Ok(Grouping::Semantic),
        "flat" => Ok(Grouping::Flat),
        _ => Err(format!(
            "Invalid grouping '{}'. Expected one of: component, semantic, flat",
            s
        )),
    }
}

fn parse_id_style(s: &str) -> Result<IdStyle, String> {
    match s.to_lowercase().as_str() {
        "hash" => Ok(IdStyle::Hash),
        "sequential" => Ok(IdStyle::Sequential),
        "none" => Ok(IdStyle::None),
        _ => Err(format!(
            "Invalid id-style '{}'. Expected one of: hash, sequential, none",
            s
        )),
    }
}

fn parse_transform_origin(s: &str) -> Result<TOrigin, String> {
    match s.to_lowercase().as_str() {
        "centroid" => Ok(TOrigin::Centroid),
        "baked" => Ok(TOrigin::Baked),
        _ => Err(format!(
            "Invalid transform-origin '{}'. Expected one of: centroid, baked",
            s
        )),
    }
}

fn parse_hex_rgb(s: &str) -> Result<Rgb, String> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid color '{}'. Expected #rrggbb", s));
    }
    Ok(Rgb {
        r: u8::from_str_radix(&hex[0..2], 16).unwrap(),
        g: u8::from_str_radix(&hex[2..4], 16).unwrap(),
        b: u8::from_str_radix(&hex[4..6], 16).unwrap(),
    })
}

fn parse_alpha_mode(s: &str) -> Result<AlphaMode, String> {
    let lower = s.to_lowercase();
    if lower == "keep" {
        return Ok(AlphaMode::Keep);
    }
    if let Some(color) = lower.strip_prefix("matte:") {
        return Ok(AlphaMode::Matte(parse_hex_rgb(color)?));
    }
    if let Some(value) = lower.strip_prefix("threshold:") {
        return value
            .parse::<u8>()
            .map(AlphaMode::Threshold)
            .map_err(|_| format!("Invalid alpha threshold '{}'. Expected 0-255", value));
    }
    Err(format!(
        "Invalid alpha-mode '{}'. Expected keep, matte:#rrggbb, or threshold:0-255",
        s
    ))
}

#[derive(Parser)]
#[command(name = "spryteo", version, about = "Vectorization tool")]
enum Cli {
    /// Convert a raster image to SVG
    Convert(ConvertArgs),
    /// Print metadata for an SVG produced by `spryteo convert`
    Inspect(InspectArgs),
    /// Extract every icon from a contact sheet into separate SVG files
    Sheet(SheetArgs),
}

#[derive(Args, Clone, Debug)]
struct ConvertArgs {
    /// Input file path
    input: std::path::PathBuf,

    /// Output SVG file path
    #[arg(short, long)]
    output: std::path::PathBuf,

    /// How the engine should interpret the input image
    #[arg(long, default_value = "auto", value_parser = parse_mode)]
    mode: spryteo_core::Mode,

    /// Run the centerline / stroke tracing pipeline instead of fill mode
    #[arg(long)]
    stroke: bool,

    /// Optional CSS preset (e.g. draw, fade, pop)
    #[arg(long, value_parser = parse_preset)]
    css: Option<Preset>,

    /// Target colour palette size (e.g. 8). This is a ceiling, not a
    /// guarantee: visually indistinguishable clusters are merged after
    /// quantization, so the final count can be lower.
    #[arg(long)]
    colors: Option<u8>,

    /// Layer composition mode for photo-mode quantization
    #[arg(long, value_parser = parse_layering)]
    layering: Option<Layering>,

    /// Gradient detection: auto (photos only), on, or off
    #[arg(long, value_parser = parse_gradients)]
    gradients: Option<Tri>,

    /// Global curve-fit error budget in pixels
    #[arg(long)]
    tolerance: Option<f32>,

    /// Corner-preservation strength
    #[arg(long)]
    smoothness: Option<f32>,

    /// Minimum region area in square pixels
    #[arg(long)]
    turdsize: Option<u32>,

    /// Number of decimal places in SVG path coordinates
    #[arg(long)]
    precision: Option<u8>,

    /// Output pretty-printed SVG markup instead of minified
    #[arg(long, conflicts_with = "jsx")]
    pretty: bool,

    /// Output a React JSX component instead of plain SVG
    #[arg(long)]
    jsx: bool,

    /// Force monochrome fill color to inherit via currentColor
    #[arg(long)]
    current_color: bool,

    /// Background treatment: keep, drop, or rect
    #[arg(long, value_parser = parse_background)]
    background: Option<Background>,

    /// Alpha handling: keep, matte:#rrggbb, or threshold:0-255
    #[arg(long, value_parser = parse_alpha_mode)]
    alpha_mode: Option<AlphaMode>,

    /// Grouping strategy: component, semantic, or flat
    #[arg(long, value_parser = parse_grouping)]
    grouping: Option<Grouping>,

    /// ID generation: hash, sequential, or none
    #[arg(long, value_parser = parse_id_style)]
    id_style: Option<IdStyle>,

    /// Transform origin placement: centroid or baked
    #[arg(long, value_parser = parse_transform_origin)]
    transform_origin: Option<TOrigin>,

    /// Emit circular-arc path commands where detected
    #[arg(long)]
    arcs: bool,

    /// Downscale inputs larger than this dimension before tracing
    #[arg(long)]
    max_trace_dimension: Option<u32>,

    /// Maximum input pixel count accepted before rejecting
    #[arg(long)]
    max_pixels: Option<u64>,

    /// Maximum input size in bytes accepted before rejecting
    #[arg(long)]
    max_input_bytes: Option<u64>,

    /// Abort the conversion after this many milliseconds
    #[arg(long)]
    timeout_ms: Option<u64>,

    /// Write the metadata sidecar as JSON to this path
    #[arg(long)]
    json: Option<std::path::PathBuf>,

    /// JSON file of segmentation masks for --grouping semantic (array of {id, width, height, pixels})
    #[arg(long)]
    masks: Option<std::path::PathBuf>,

    /// Path to SAM encoder ONNX model (requires --grouping semantic, enables auto-mask generation)
    #[cfg(feature = "ml")]
    #[arg(long)]
    sam_encoder: Option<std::path::PathBuf>,

    /// Path to SAM decoder ONNX model (requires --grouping semantic, enables auto-mask generation)
    #[cfg(feature = "ml")]
    #[arg(long)]
    sam_decoder: Option<std::path::PathBuf>,
}

#[derive(Args, Clone, Debug)]
struct InspectArgs {
    /// SVG file to inspect (as produced by `spryteo convert`)
    svg: std::path::PathBuf,

    /// Metadata sidecar JSON path (as written by `convert --json`). When
    /// omitted, a lighter summary is re-derived directly from the SVG
    /// markup (element counts, IDs, viewBox -- no centroid/area/group tree).
    #[arg(long)]
    meta: Option<std::path::PathBuf>,
}

#[derive(Args, Clone, Debug)]
struct SheetArgs {
    /// Input raster icon contact sheet image
    input: std::path::PathBuf,

    /// Output directory for extracted SVGs
    #[arg(short, long)]
    out_dir: std::path::PathBuf,

    /// Upscaling factor for stroke tracing
    #[arg(long, default_value = "4")]
    supersample: u32,

    /// Canonical viewBox size in pixels
    #[arg(long, default_value = "24.0")]
    canonical_size: f64,

    /// Pixels of padding added around each icon crop
    #[arg(long, default_value = "2")]
    pad: u32,

    /// Emit currentColor instead of a linear gradient
    #[arg(long)]
    flat: bool,

    /// Saturation threshold for chroma segmentation (0-255)
    #[arg(long)]
    sat_threshold: Option<u8>,

    /// Prefix for output SVG filenames
    #[arg(long, default_value = "icon")]
    name_prefix: String,

    /// Optional path to write JSON manifest report
    #[arg(long)]
    manifest: Option<std::path::PathBuf>,

    /// Perform analysis and return report without writing files to disk
    #[arg(long)]
    dry_run: bool,

    /// Global curve-fit error budget in pixels
    #[arg(long)]
    tolerance: Option<f32>,

    /// Number of decimal places in SVG path coordinates
    #[arg(long)]
    precision: Option<u8>,

    /// Regularize geometry (trades raster fidelity for cleaner, more consistent geometry)
    #[arg(long)]
    regularize: bool,

    /// Grid pitch in canonical units for regularization
    #[arg(long, default_value = "1.0")]
    grid_pitch: f64,
}

fn main() {
    let cli = Cli::parse();
    match cli {
        Cli::Convert(args) => {
            let default_opts = ConvertOptions::default();
            let opts = ConvertOptions {
                mode: args.mode,
                stroke: args.stroke,
                colors: match args.colors {
                    Some(n) => ColorSpec::N(n),
                    None => ColorSpec::Auto,
                },
                layering: args.layering.clone().unwrap_or(default_opts.layering),
                gradients: args.gradients.clone().unwrap_or(default_opts.gradients),
                tolerance: args.tolerance.unwrap_or(default_opts.tolerance),
                smoothness: args.smoothness.unwrap_or(default_opts.smoothness),
                turdsize: args.turdsize.unwrap_or(default_opts.turdsize),
                precision: args.precision.unwrap_or(default_opts.precision),
                current_color: args.current_color,
                background: args.background.clone().unwrap_or(default_opts.background),
                alpha_mode: args.alpha_mode.clone().unwrap_or(default_opts.alpha_mode),
                grouping: args.grouping.clone().unwrap_or(default_opts.grouping),
                id_style: args.id_style.clone().unwrap_or(default_opts.id_style),
                transform_origin: args
                    .transform_origin
                    .clone()
                    .unwrap_or(default_opts.transform_origin),
                arcs: args.arcs,
                max_trace_dimension: args
                    .max_trace_dimension
                    .or(default_opts.max_trace_dimension),
                max_pixels: args.max_pixels.unwrap_or(default_opts.max_pixels),
                max_input_bytes: args.max_input_bytes.unwrap_or(default_opts.max_input_bytes),
                timeout_ms: args.timeout_ms.or(default_opts.timeout_ms),
                output: if args.jsx {
                    OutputFormat::Jsx
                } else if args.pretty {
                    OutputFormat::SvgPretty
                } else {
                    OutputFormat::Svg
                },
                emit_css: args.css.clone(),
            };

            let bytes = match std::fs::read(&args.input) {
                Ok(b) => b,
                Err(source) => {
                    eprintln!(
                        "Error: failed to read '{}': {}",
                        args.input.display(),
                        source
                    );
                    std::process::exit(1);
                }
            };

            #[cfg(feature = "ml")]
            {
                let encoder_path = args.sam_encoder.clone().or_else(|| {
                    std::env::var("SPRYTEO_SAM_ENCODER")
                        .ok()
                        .map(std::path::PathBuf::from)
                });
                let decoder_path = args.sam_decoder.clone().or_else(|| {
                    std::env::var("SPRYTEO_SAM_DECODER")
                        .ok()
                        .map(std::path::PathBuf::from)
                });
                match (encoder_path, decoder_path) {
                    (Some(ep), Some(dp)) => {
                        if !matches!(opts.grouping, Grouping::Semantic) {
                            eprintln!(
                                "Error: --sam-encoder/--sam-decoder requires --grouping semantic"
                            );
                            std::process::exit(1);
                        }
                        if args.masks.is_some() {
                            eprintln!(
                                "Error: --sam-encoder/--sam-decoder is mutually exclusive with --masks"
                            );
                            std::process::exit(1);
                        }
                        let result = match run_pipeline_sam(
                            &bytes,
                            &args.output,
                            args.json.as_deref(),
                            &opts,
                            &ep,
                            &dp,
                        ) {
                            Ok(r) => r,
                            Err(err) => {
                                eprintln!("Error: {}", err);
                                std::process::exit(err.exit_code());
                            }
                        };
                        let input_str = args.input.to_string_lossy();
                        let output_str = args.output.to_string_lossy();
                        if opts.stroke {
                            println!(
                                "{} -> {} ({} nodes, {} bytes, {} continuous paths)",
                                input_str,
                                output_str,
                                result.meta.stats.node_count,
                                result.meta.stats.byte_count,
                                result.meta.stats.path_count
                            );
                        } else {
                            println!(
                                "{} -> {} ({} nodes, {} bytes)",
                                input_str,
                                output_str,
                                result.meta.stats.node_count,
                                result.meta.stats.byte_count
                            );
                        }
                        std::process::exit(0);
                    }
                    (Some(_), None) => {
                        eprintln!(
                            "Error: --sam-decoder also required when --sam-encoder is set, \
                             or set SPRYTEO_SAM_DECODER"
                        );
                        std::process::exit(1);
                    }
                    (None, Some(_)) => {
                        eprintln!(
                            "Error: --sam-encoder also required when --sam-decoder is set, \
                             or set SPRYTEO_SAM_ENCODER"
                        );
                        std::process::exit(1);
                    }
                    (None, None) => {} // fall through
                }
            }

            let masks = match &args.masks {
                Some(mask_path) => {
                    if !matches!(opts.grouping, Grouping::Semantic) {
                        eprintln!("Error: --masks requires --grouping semantic");
                        std::process::exit(1);
                    }
                    match parse_masks(mask_path) {
                        Ok(m) => m,
                        Err(msg) => {
                            eprintln!("Error: {}", msg);
                            std::process::exit(1);
                        }
                    }
                }
                None => vec![],
            };

            let result = match run_pipeline_with_bytes(
                &bytes,
                &args.output,
                args.json.as_deref(),
                &opts,
                &masks,
            ) {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(err.exit_code());
                }
            };

            let input_str = args.input.to_string_lossy();
            let output_str = args.output.to_string_lossy();
            if opts.stroke {
                println!(
                    "{} -> {} ({} nodes, {} bytes, {} continuous paths)",
                    input_str,
                    output_str,
                    result.meta.stats.node_count,
                    result.meta.stats.byte_count,
                    result.meta.stats.path_count
                );
            } else {
                println!(
                    "{} -> {} ({} nodes, {} bytes)",
                    input_str,
                    output_str,
                    result.meta.stats.node_count,
                    result.meta.stats.byte_count
                );
            }
            std::process::exit(0);
        }
        Cli::Inspect(args) => {
            let svg_text = match std::fs::read_to_string(&args.svg) {
                Ok(s) => s,
                Err(source) => {
                    eprintln!("Error: failed to read '{}': {}", args.svg.display(), source);
                    std::process::exit(1);
                }
            };

            match &args.meta {
                Some(meta_path) => {
                    let meta_text = match std::fs::read_to_string(meta_path) {
                        Ok(s) => s,
                        Err(source) => {
                            eprintln!(
                                "Error: failed to read '{}': {}",
                                meta_path.display(),
                                source
                            );
                            std::process::exit(1);
                        }
                    };
                    match serde_json::from_str::<Meta>(&meta_text) {
                        Ok(meta) => {
                            print!("{}", format_meta_report(&meta));
                            std::process::exit(0);
                        }
                        Err(source) => {
                            eprintln!(
                                "Error: '{}' is not a valid metadata sidecar: {}",
                                meta_path.display(),
                                source
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    print!("{}", derive_svg_summary(&svg_text));
                    std::process::exit(0);
                }
            }
        }
        Cli::Sheet(args) => {
            let bytes = match std::fs::read(&args.input) {
                Ok(b) => b,
                Err(source) => {
                    eprintln!(
                        "Error: failed to read '{}': {}",
                        args.input.display(),
                        source
                    );
                    std::process::exit(1);
                }
            };

            let mut opts = spryteo_core::ConvertOptions::default();
            if let Some(t) = args.tolerance {
                opts.tolerance = t;
            }
            if let Some(p) = args.precision {
                opts.precision = p;
            }

            let sheet_opts = spryteo_cli::SheetOptions {
                out_dir: args.out_dir,
                supersample: args.supersample,
                canonical_size: args.canonical_size,
                pad: args.pad,
                flat: args.flat,
                sat_threshold: args.sat_threshold,
                name_prefix: args.name_prefix,
                manifest: args.manifest,
                dry_run: args.dry_run,
                regularize: args.regularize,
                grid_pitch: args.grid_pitch,
            };

            let report = match spryteo_cli::run_sheet(&bytes, &opts, &sheet_opts) {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(spryteo_cli::CliError::Spryteo(err).exit_code());
                }
            };

            println!(
                "Sheet grid: {}x{} (confidence: {:.2}), written: {}/{} icons",
                report.cols,
                report.rows,
                report.confidence,
                report.written,
                report.icons.len()
            );
            if report.straddling > 0 || report.orphans > 0 || report.empty_cells > 0 {
                println!(
                    "Warnings: {} straddling, {} orphans, {} empty cells",
                    report.straddling, report.orphans, report.empty_cells
                );
            }
            std::process::exit(0);
        }
    }
}
