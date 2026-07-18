use spryteo_core::Rgb;

/// D65 white point reference values for XYZ → Lab conversion.
const XN: f64 = 0.95047;
const YN: f64 = 1.0;
const ZN: f64 = 1.08883;

const DELTA: f64 = 6.0 / 29.0;
const DELTA_CUBED: f64 = DELTA * DELTA * DELTA;

/// A colour in CIELAB (D65) perceptual colour space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

/// Convert an sRGB 8-bit component to linear (undo gamma correction).
fn srgb_to_linear(c: u8) -> f64 {
    let v = c as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert a linear component to sRGB 8-bit (apply gamma).
fn linear_to_srgb(v: f64) -> u8 {
    let clamped = v.clamp(0.0, 1.0);
    let c = if clamped <= 0.0031308 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0 + 0.5).round().clamp(0.0, 255.0) as u8
}

/// Convert an sRGB 8-bit colour to CIELAB (D65 white point).
///
/// Formula: sRGB → linear RGB → XYZ (D65) → CIELAB.
/// This is the standard transformation used throughout the quantiser for
/// perceptually-uniform colour distance calculations.
pub fn srgb_to_lab(rgb: &Rgb) -> Lab {
    let r = srgb_to_linear(rgb.r);
    let g = srgb_to_linear(rgb.g);
    let b = srgb_to_linear(rgb.b);

    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;

    let fx = lab_f(x / XN);
    let fy = lab_f(y / YN);
    let fz = lab_f(z / ZN);

    Lab {
        l: 116.0 * fy - 16.0,
        a: 500.0 * (fx - fy),
        b: 200.0 * (fy - fz),
    }
}

/// Convert CIELAB back to sRGB 8-bit.
///
/// Formula: CIELAB → XYZ → linear RGB → sRGB.
pub fn lab_to_srgb(lab: &Lab) -> Rgb {
    let fy = (lab.l + 16.0) / 116.0;
    let fx = lab.a / 500.0 + fy;
    let fz = fy - lab.b / 200.0;

    let x = XN * lab_f_inv(fx);
    let y = YN * lab_f_inv(fy);
    let z = ZN * lab_f_inv(fz);

    let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let b = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;

    Rgb {
        r: linear_to_srgb(r),
        g: linear_to_srgb(g),
        b: linear_to_srgb(b),
    }
}

/// The `f(t)` function in the CIELAB conversion: cube root for large t,
/// linear approximation near zero to avoid singularity.
fn lab_f(t: f64) -> f64 {
    if t > DELTA_CUBED {
        t.powf(1.0 / 3.0)
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

/// The inverse of `f(t)`.
fn lab_f_inv(t: f64) -> f64 {
    if t > DELTA {
        t * t * t
    } else {
        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
    }
}

/// Squared Euclidean distance between two CIELAB colours (perceptual proxy).
pub fn lab_distance_sq(a: &Lab, b: &Lab) -> f64 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    dl * dl + da * da + db * db
}

/// Extract the `Rgb` value at pixel `(x, y)` from a flat RGBA buffer.
pub fn get_rgb(pixels: &[u8], width: u32, x: u32, y: u32) -> Rgb {
    let idx = ((y * width + x) * 4) as usize;
    Rgb {
        r: pixels[idx],
        g: pixels[idx + 1],
        b: pixels[idx + 2],
    }
}

/// Extract the alpha value at pixel `(x, y)` from a flat RGBA buffer.
pub fn get_alpha(pixels: &[u8], width: u32, x: u32, y: u32) -> u8 {
    let idx = ((y * width + x) * 4) as usize;
    pixels[idx + 3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_to_lab_black() {
        let lab = srgb_to_lab(&Rgb { r: 0, g: 0, b: 0 });
        assert!(lab.l.abs() < 1.0);
        assert!(lab.a.abs() < 1.0);
        assert!(lab.b.abs() < 1.0);
    }

    #[test]
    fn srgb_to_lab_white() {
        let lab = srgb_to_lab(&Rgb {
            r: 255,
            g: 255,
            b: 255,
        });
        assert!((lab.l - 100.0).abs() < 1.0);
    }

    #[test]
    fn round_trip_preserves_colour() {
        let input = Rgb {
            r: 123,
            g: 67,
            b: 200,
        };
        let lab = srgb_to_lab(&input);
        let output = lab_to_srgb(&lab);
        // Round-trip should be lossy within a couple of code values
        let dr = (input.r as i16 - output.r as i16).abs();
        let dg = (input.g as i16 - output.g as i16).abs();
        let db = (input.b as i16 - output.b as i16).abs();
        assert!(dr <= 2, "r diff: {dr}");
        assert!(dg <= 2, "g diff: {dg}");
        assert!(db <= 2, "b diff: {db}");
    }
}
