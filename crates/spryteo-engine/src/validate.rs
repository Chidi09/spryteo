//! Centralised option validation (ROADMAP §3.1).
//!
//! Every surface funnels through here, so an out-of-range value produces
//! the same typed error whether it arrived as a CLI flag, an npm options
//! object, an MCP tool argument, or a WASM JSON string. Validation runs
//! before decode so a bad option costs nothing.

use spryteo_core::{ColorSpec, ConvertOptions, SpryteoError};

/// Largest colour count the quantizer will honour. `ColorSpec::N` is a
/// `u8`, so the type already caps at 255; this is the documented ceiling
/// from §3.4 above which k-means stops being meaningful for vector output.
pub const MAX_COLORS: u8 = 64;

/// Smallest working dimension worth tracing. Below roughly this size the
/// contour stage has fewer pixels than a typical curve has control points,
/// so the output is noise rather than a cheaper approximation.
pub const MIN_TRACE_DIMENSION: u32 = 8;

fn invalid(msg: impl Into<String>) -> SpryteoError {
    SpryteoError::InvalidInput(msg.into())
}

/// Validate a fully-constructed [`ConvertOptions`].
///
/// Returns the first violation found, in a stable field order so the error
/// a given bad options object produces is deterministic across surfaces.
pub fn validate(opts: &ConvertOptions) -> Result<(), SpryteoError> {
    match &opts.colors {
        ColorSpec::N(0) => {
            return Err(invalid("colors must be at least 2, got 0"));
        }
        ColorSpec::N(1) => {
            return Err(invalid("colors must be at least 2, got 1"));
        }
        ColorSpec::N(n) if *n > MAX_COLORS => {
            return Err(invalid(format!(
                "colors must be at most {MAX_COLORS}, got {n}"
            )));
        }
        ColorSpec::Palette(p) if p.is_empty() => {
            return Err(invalid("palette must contain at least one colour"));
        }
        ColorSpec::Palette(p) if p.len() > MAX_COLORS as usize => {
            return Err(invalid(format!(
                "palette must contain at most {MAX_COLORS} colours, got {}",
                p.len()
            )));
        }
        _ => {}
    }

    if !opts.tolerance.is_finite() || opts.tolerance < 0.0 {
        return Err(invalid(format!(
            "tolerance must be a finite non-negative number, got {}",
            opts.tolerance
        )));
    }

    if !opts.smoothness.is_finite() || !(0.0..=1.34).contains(&opts.smoothness) {
        return Err(invalid(format!(
            "smoothness must be between 0.0 and 1.34, got {}",
            opts.smoothness
        )));
    }

    if opts.precision > 10 {
        return Err(invalid(format!(
            "precision must be at most 10, got {}",
            opts.precision
        )));
    }

    if let Some(max_dim) = opts.max_trace_dimension {
        if max_dim < MIN_TRACE_DIMENSION {
            return Err(invalid(format!(
                "max_trace_dimension must be at least {MIN_TRACE_DIMENSION}, got {max_dim}"
            )));
        }
    }

    if opts.max_pixels == 0 {
        return Err(invalid("max_pixels must be greater than 0"));
    }

    if opts.max_input_bytes == 0 {
        return Err(invalid("max_input_bytes must be greater than 0"));
    }

    Ok(())
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;
