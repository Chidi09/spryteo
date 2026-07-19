use clap::{Args, Parser};
use spryteo_cli::{derive_svg_summary, format_meta_report, parse_mode, parse_preset, run_pipeline};
use spryteo_core::{ColorSpec, ConvertOptions, Layering, Meta, OutputFormat, Preset, Tri};

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

#[derive(Parser)]
#[command(name = "spryteo", version, about = "Vectorization tool")]
enum Cli {
    /// Convert a raster image to SVG
    Convert(ConvertArgs),
    /// Print metadata for an SVG produced by `spryteo convert`
    Inspect(InspectArgs),
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

    /// Target colour palette size (e.g. 8)
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
    #[arg(long)]
    pretty: bool,

    /// Force monochrome fill color to inherit via currentColor
    #[arg(long)]
    current_color: bool,

    /// Write the metadata sidecar as JSON to this path
    #[arg(long)]
    json: Option<std::path::PathBuf>,
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
                output: if args.pretty {
                    OutputFormat::SvgPretty
                } else {
                    OutputFormat::Svg
                },
                emit_css: args.css.clone(),
                ..default_opts
            };

            match run_pipeline(&args.input, &args.output, args.json.as_deref(), &opts) {
                Ok(result) => {
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
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(err.exit_code());
                }
            }
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
    }
}
