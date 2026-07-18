use clap::{Args, Parser};
use spryteo_cli::{parse_mode, run_pipeline};
use spryteo_core::{ColorSpec, ConvertOptions, OutputFormat};

#[derive(Parser)]
#[command(name = "spryteo", version, about = "Vectorization tool")]
enum Cli {
    /// Convert a raster image to SVG
    Convert(ConvertArgs),
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

    /// Target colour palette size (e.g. 8)
    #[arg(long)]
    colors: Option<u8>,

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

    /// Write the metadata sidecar as JSON to this path
    #[arg(long)]
    json: Option<std::path::PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    match cli {
        Cli::Convert(args) => {
            let default_opts = ConvertOptions::default();
            let opts = ConvertOptions {
                mode: args.mode,
                colors: match args.colors {
                    Some(n) => ColorSpec::N(n),
                    None => ColorSpec::Auto,
                },
                tolerance: args.tolerance.unwrap_or(default_opts.tolerance),
                smoothness: args.smoothness.unwrap_or(default_opts.smoothness),
                turdsize: args.turdsize.unwrap_or(default_opts.turdsize),
                precision: args.precision.unwrap_or(default_opts.precision),
                output: if args.pretty {
                    OutputFormat::SvgPretty
                } else {
                    OutputFormat::Svg
                },
                ..default_opts
            };

            match run_pipeline(&args.input, &args.output, args.json.as_deref(), &opts) {
                Ok(result) => {
                    let input_str = args.input.to_string_lossy();
                    let output_str = args.output.to_string_lossy();
                    println!(
                        "{} -> {} ({} nodes, {} bytes)",
                        input_str,
                        output_str,
                        result.meta.stats.node_count,
                        result.meta.stats.byte_count
                    );
                    std::process::exit(0);
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(err.exit_code());
                }
            }
        }
    }
}
