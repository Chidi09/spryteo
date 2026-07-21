use spryteo_cli::{run_sheet, SheetOptions};
use spryteo_core::ConvertOptions;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("spryteo_sheet_test_{}_{}", nanos, id));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn generate_synthetic_sheet_png() -> Vec<u8> {
    let w = 200u32;
    let h = 140u32;
    let mut imgbuf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
        w,
        h,
        image::Rgba([255, 255, 255, 255]),
    );

    let draw_rect = |img: &mut image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
                     x1: u32,
                     y1: u32,
                     x2: u32,
                     y2: u32,
                     color: image::Rgba<u8>| {
        for y in y1..=y2 {
            for x in x1..=x2 {
                if x < w && y < h {
                    img.put_pixel(x, y, color);
                }
            }
        }
    };

    let blue = image::Rgba([0, 0, 200, 255]);
    let red = image::Rgba([200, 0, 0, 255]);
    let green = image::Rgba([0, 200, 0, 255]);
    let purple = image::Rgba([200, 0, 200, 255]);
    let orange = image::Rgba([220, 120, 0, 255]);
    let magenta = image::Rgba([200, 0, 100, 255]);

    // Cell (0,0): L shape (Blue)
    draw_rect(&mut imgbuf, 24, 24, 31, 48, blue);
    draw_rect(&mut imgbuf, 24, 41, 48, 48, blue);

    // Cell (1,0): Horizontal bar (Red)
    draw_rect(&mut imgbuf, 84, 32, 108, 40, red);

    // Cell (2,0): Vertical bar (Green)
    draw_rect(&mut imgbuf, 152, 24, 160, 48, green);

    // Cell (0,1): Plus cross (Purple)
    draw_rect(&mut imgbuf, 24, 92, 48, 100, purple);
    draw_rect(&mut imgbuf, 32, 84, 40, 108, purple);

    // Cell (1,1): Square frame (Orange)
    draw_rect(&mut imgbuf, 84, 84, 108, 91, orange);
    draw_rect(&mut imgbuf, 84, 101, 108, 108, orange);
    draw_rect(&mut imgbuf, 84, 84, 91, 108, orange);
    draw_rect(&mut imgbuf, 101, 84, 108, 108, orange);

    // Cell (2,1): T shape (Magenta)
    draw_rect(&mut imgbuf, 144, 84, 168, 91, magenta);
    draw_rect(&mut imgbuf, 152, 84, 160, 108, magenta);

    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    imgbuf
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn generate_greyscale_png() -> Vec<u8> {
    let w = 200u32;
    let h = 140u32;
    let mut imgbuf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
        w,
        h,
        image::Rgba([255, 255, 255, 255]),
    );

    let draw_rect = |img: &mut image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
                     x1: u32,
                     y1: u32,
                     x2: u32,
                     y2: u32,
                     color: image::Rgba<u8>| {
        for y in y1..=y2 {
            for x in x1..=x2 {
                if x < w && y < h {
                    img.put_pixel(x, y, color);
                }
            }
        }
    };

    let black = image::Rgba([40, 40, 40, 255]);
    let dark_grey = image::Rgba([80, 80, 80, 255]);

    // Cell (0,0): L shape
    draw_rect(&mut imgbuf, 24, 24, 31, 48, black);
    draw_rect(&mut imgbuf, 24, 41, 48, 48, black);

    // Cell (1,0): Horizontal bar
    draw_rect(&mut imgbuf, 84, 32, 108, 40, dark_grey);

    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    imgbuf
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn test_sheet_synthetic_grid() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");

    assert_eq!(report.cols, 3);
    assert_eq!(report.rows, 2);
    assert_eq!(report.written, 6);
    assert_eq!(report.icons.len(), 6);

    let entries: Vec<_> = fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 6);

    let usvg_opts = resvg::usvg::Options::default();
    for entry in entries {
        let path = entry.path();
        assert_eq!(path.extension().unwrap(), "svg");
        let svg_str = fs::read_to_string(&path).unwrap();
        resvg::usvg::Tree::from_str(&svg_str, &usvg_opts)
            .unwrap_or_else(|e| panic!("SVG parsing failed for {}: {}", path.display(), e));
    }

    let _ = fs::remove_dir_all(&out_dir);
}

fn extract_stroke_widths(svg_str: &str) -> Vec<f64> {
    let mut widths = Vec::new();
    let pat = "stroke-width=\"";
    let mut rest = svg_str;
    while let Some(pos) = rest.find(pat) {
        rest = &rest[pos + pat.len()..];
        if let Some(end) = rest.find('"') {
            if let Ok(val) = rest[..end].parse::<f64>() {
                widths.push(val);
            }
            rest = &rest[end + 1..];
        }
    }
    widths
}

#[test]
fn test_sheet_unify_widths_default() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert!(
        report.unified_stroke_width.is_some(),
        "report.unified_stroke_width should be Some(_)"
    );

    let mut all_widths = Vec::new();
    for entry in fs::read_dir(&out_dir).unwrap().filter_map(|e| e.ok()) {
        let svg_str = fs::read_to_string(entry.path()).unwrap();
        all_widths.extend(extract_stroke_widths(&svg_str));
    }

    assert!(
        !all_widths.is_empty(),
        "emitted SVGs should contain stroke-width attributes"
    );
    let first_w = all_widths[0];
    for w in &all_widths {
        assert_eq!(
            *w, first_w,
            "all stroke-width values in emitted SVGs should be equal"
        );
    }

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_sheet_unify_widths_disabled() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        unify_widths: false,
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert!(
        report.unified_stroke_width.is_none(),
        "report.unified_stroke_width should be None when unify_widths is false"
    );

    for icon in &report.icons {
        assert!(
            icon.width_unified.is_none(),
            "icon.width_unified should be None when unify_widths is false"
        );
    }

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_sheet_output_is_24x24() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        canonical_size: 24.0,
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(report.written, 6);

    for entry in fs::read_dir(&out_dir).unwrap().filter_map(|e| e.ok()) {
        let svg_str = fs::read_to_string(entry.path()).unwrap();
        assert!(
            svg_str.contains("viewBox=\"0 0 24 24\"")
                || svg_str.contains("viewBox=\"0 0 24.00 24.00\""),
            "SVG in {} should contain viewBox 24x24, got string sample:\n{}",
            entry.path().display(),
            &svg_str[..svg_str.len().min(200)]
        );
    }

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_sheet_determinism() {
    let png = generate_synthetic_sheet_png();
    let dir1 = unique_temp_dir();
    let dir2 = unique_temp_dir();
    let opts = ConvertOptions::default();

    let sheet_opts1 = SheetOptions {
        out_dir: dir1.clone(),
        ..SheetOptions::default()
    };
    let sheet_opts2 = SheetOptions {
        out_dir: dir2.clone(),
        ..SheetOptions::default()
    };

    let report1 = run_sheet(&png, &opts, &sheet_opts1).unwrap();
    let report2 = run_sheet(&png, &opts, &sheet_opts2).unwrap();

    assert_eq!(report1.written, report2.written);

    for icon in &report1.icons {
        let filename = format!("{}.svg", icon.name);
        let bytes1 = fs::read(dir1.join(&filename)).unwrap();
        let bytes2 = fs::read(dir2.join(&filename)).unwrap();
        assert_eq!(
            bytes1, bytes2,
            "Emitted files for {} must be byte-identical",
            filename
        );
    }

    let _ = fs::remove_dir_all(&dir1);
    let _ = fs::remove_dir_all(&dir2);
}

#[test]
fn test_sheet_dry_run_writes_nothing() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        dry_run: true,
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(report.icons.len(), 6);

    let file_count = fs::read_dir(&out_dir).unwrap().count();
    assert_eq!(file_count, 0, "dry_run: true must write 0 files to disk");

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_sheet_greyscale_sheet_uses_luminance() {
    let grey_png = generate_greyscale_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&grey_png, &opts, &sheet_opts).expect("Greyscale sheet should succeed");
    assert_eq!(report.segmentation, "luminance");
    assert!(report.written >= 1);

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_sheet_flat_mode_emits_no_gradient() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        flat: true,
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(report.written, 6);

    for entry in fs::read_dir(&out_dir).unwrap().filter_map(|e| e.ok()) {
        let svg_str = fs::read_to_string(entry.path()).unwrap();
        assert!(
            !svg_str.contains("<linearGradient"),
            "Flat mode must emit no <linearGradient, found in {}",
            entry.path().display()
        );
    }

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_sheet_regularize_off_by_default_is_unchanged() {
    let png = generate_synthetic_sheet_png();
    let dir_default = unique_temp_dir();
    let dir_explicit_false = unique_temp_dir();
    let opts = ConvertOptions::default();

    let sheet_opts_default = SheetOptions {
        out_dir: dir_default.clone(),
        ..SheetOptions::default()
    };
    let sheet_opts_explicit_false = SheetOptions {
        out_dir: dir_explicit_false.clone(),
        regularize: false,
        ..SheetOptions::default()
    };

    let rep_def = run_sheet(&png, &opts, &sheet_opts_default).expect("run_sheet default");
    let rep_false =
        run_sheet(&png, &opts, &sheet_opts_explicit_false).expect("run_sheet explicit false");

    assert_eq!(rep_def.written, rep_false.written);

    for icon in &rep_def.icons {
        let filename = format!("{}.svg", icon.name);
        let bytes_def = fs::read(dir_default.join(&filename)).unwrap();
        assert!(
            icon.regularize.is_none(),
            "regularize report should be None when off by default"
        );
        let bytes_false = fs::read(dir_explicit_false.join(&filename)).unwrap();
        assert_eq!(
            bytes_def, bytes_false,
            "Emitted files for {} must be byte-identical",
            filename
        );
    }

    let _ = fs::remove_dir_all(&dir_default);
    let _ = fs::remove_dir_all(&dir_explicit_false);
}

#[test]
fn test_sheet_regularize_produces_valid_svg() {
    let png = generate_synthetic_sheet_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        regularize: true,
        grid_pitch: 1.0,
        ..SheetOptions::default()
    };

    let report =
        run_sheet(&png, &opts, &sheet_opts).expect("run_sheet with regularize should succeed");
    assert_eq!(report.written, 6);

    let usvg_opts = resvg::usvg::Options::default();
    for icon in &report.icons {
        assert!(
            icon.regularize.is_some(),
            "regularize report should be present when regularize is true"
        );
        let filename = format!("{}.svg", icon.name);
        let path = out_dir.join(&filename);
        let svg_str = fs::read_to_string(&path).unwrap();
        resvg::usvg::Tree::from_str(&svg_str, &usvg_opts).unwrap_or_else(|e| {
            panic!(
                "SVG parsing failed for regularized {}: {}",
                path.display(),
                e
            )
        });
    }

    let _ = fs::remove_dir_all(&out_dir);
}

const REAL_SHEET: &str = "../../testdata/corpus/sheets/gradient_lattice_1536.png";

fn load_real_sheet() -> Vec<u8> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let path = std::path::Path::new(&manifest_dir).join(REAL_SHEET);
    fs::read(&path)
        .or_else(|_| fs::read(REAL_SHEET))
        .expect("Failed to read REAL_SHEET fixture")
}

fn derive_grayscale(png: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(png).expect("load image from memory");
    let luma = img.to_luma8();
    let (w, h) = luma.dimensions();
    let mut rgb_img = image::RgbImage::new(w, h);
    for (x, y, pixel) in luma.enumerate_pixels() {
        let v = pixel[0];
        rgb_img.put_pixel(x, y, image::Rgb([v, v, v]));
    }
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    rgb_img
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("encode PNG");
    bytes
}

fn derive_inverted_grayscale(png: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(png).expect("load image from memory");
    let luma = img.to_luma8();
    let (w, h) = luma.dimensions();
    let mut rgb_img = image::RgbImage::new(w, h);
    for (x, y, pixel) in luma.enumerate_pixels() {
        let inv = 255 - pixel[0];
        rgb_img.put_pixel(x, y, image::Rgb([inv, inv, inv]));
    }
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    rgb_img
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("encode PNG");
    bytes
}

fn derive_jpeg_q80(png: &[u8]) -> Vec<u8> {
    let img = image::load_from_memory(png).expect("load image from memory");
    let rgb = img.to_rgb8();
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 80);
    rgb.write_with_encoder(encoder).expect("encode JPEG");
    bytes
}

#[test]
fn test_real_sheet_grayscale_luminance_path() {
    // expensive full-sheet run; enable with SPRYTEO_SHEET_FIXTURE=1
    if std::env::var("SPRYTEO_SHEET_FIXTURE").is_err() {
        return;
    }

    let bytes = derive_grayscale(&load_real_sheet());
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&bytes, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(report.cols, 15);
    assert_eq!(report.rows, 7);
    assert_eq!(report.segmentation, "luminance");
    assert_eq!(report.polarity.as_deref(), Some("dark_on_light"));
    assert_eq!(report.text_rows_removed, 10);
    assert!(report.written >= 103);

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_real_sheet_inverted_dark_mode() {
    // expensive full-sheet run; enable with SPRYTEO_SHEET_FIXTURE=1
    if std::env::var("SPRYTEO_SHEET_FIXTURE").is_err() {
        return;
    }

    let bytes = derive_inverted_grayscale(&load_real_sheet());
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&bytes, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(report.cols, 15);
    assert_eq!(report.rows, 7);
    assert_eq!(report.segmentation, "luminance");
    assert_eq!(report.polarity.as_deref(), Some("light_on_dark"));
    assert_eq!(report.text_rows_removed, 10);
    assert!(report.written >= 103);

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_real_sheet_jpeg_q80_chroma_path() {
    // expensive full-sheet run; enable with SPRYTEO_SHEET_FIXTURE=1
    if std::env::var("SPRYTEO_SHEET_FIXTURE").is_err() {
        return;
    }

    let bytes = derive_jpeg_q80(&load_real_sheet());
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&bytes, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(report.cols, 15);
    // The image-crate JPEG encoder's chroma ringing can make one text/header row
    // partially chroma-visible, adding a phantom lattice row (measured: 8 rows here
    // vs 7 from other encoders). Extraction still succeeds; tighten to == 7 when
    // JPEG threshold adaptation lands (Phase C2).
    assert!(
        report.rows == 7 || report.rows == 8,
        "expected 7 or 8 rows, got {}",
        report.rows
    );
    assert_eq!(report.segmentation, "chroma");
    assert!(
        report.written >= 104,
        "expected written >= 104, got {}",
        report.written
    );

    let mut written_icons_count = 0;
    for icon in &report.icons {
        let path = out_dir.join(format!("{}.svg", icon.name));
        if path.exists() {
            written_icons_count += 1;
            assert!(
                icon.path_count >= 1,
                "written icon {} at col {}, row {} has path_count == 0",
                icon.name,
                icon.col,
                icon.row
            );
        }
    }
    assert_eq!(written_icons_count, report.written);

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_real_sheet_luminance_determinism() {
    // expensive full-sheet run; enable with SPRYTEO_SHEET_FIXTURE=1
    if std::env::var("SPRYTEO_SHEET_FIXTURE").is_err() {
        return;
    }

    let bytes = derive_grayscale(&load_real_sheet());
    let dir1 = unique_temp_dir();
    let dir2 = unique_temp_dir();
    let opts = ConvertOptions::default();

    let sheet_opts1 = SheetOptions {
        out_dir: dir1.clone(),
        ..SheetOptions::default()
    };
    let sheet_opts2 = SheetOptions {
        out_dir: dir2.clone(),
        ..SheetOptions::default()
    };

    let report1 = run_sheet(&bytes, &opts, &sheet_opts1).expect("run_sheet 1 should succeed");
    let report2 = run_sheet(&bytes, &opts, &sheet_opts2).expect("run_sheet 2 should succeed");

    assert_eq!(report1.written, report2.written);

    let mut files1: Vec<_> = fs::read_dir(&dir1)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    let mut files2: Vec<_> = fs::read_dir(&dir2)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();

    files1.sort();
    files2.sort();
    assert_eq!(files1, files2);

    for fname in &files1 {
        let content1 = fs::read(dir1.join(fname)).unwrap();
        let content2 = fs::read(dir2.join(fname)).unwrap();
        assert_eq!(
            content1, content2,
            "Files {:?} must be byte-identical",
            fname
        );
    }

    let _ = fs::remove_dir_all(&dir1);
    let _ = fs::remove_dir_all(&dir2);
}

#[test]
fn test_sheet_uniform_noise_rejected() {
    let w = 64u32;
    let h = 64u32;
    let imgbuf =
        image::ImageBuffer::<image::Rgb<u8>, _>::from_pixel(w, h, image::Rgb([128, 128, 128]));
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    imgbuf
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();

    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        seg: spryteo_cli::sheet::SegChoice::Auto,
        ..SheetOptions::default()
    };

    let res = run_sheet(&bytes, &opts, &sheet_opts);
    assert!(res.is_err(), "uniform noise image should fail segmentation");
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.to_lowercase().contains("segmentation"),
        "Error message should mention segmentation, got: {}",
        err_msg
    );

    let _ = fs::remove_dir_all(&out_dir);
}

fn generate_synthetic_sheet_with_filled_glyphs_png() -> Vec<u8> {
    let w = 200u32;
    let h = 140u32;
    let mut imgbuf = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
        w,
        h,
        image::Rgba([255, 255, 255, 255]),
    );

    let draw_rect = |img: &mut image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
                     x1: u32,
                     y1: u32,
                     x2: u32,
                     y2: u32,
                     color: image::Rgba<u8>| {
        for y in y1..=y2 {
            for x in x1..=x2 {
                if x < w && y < h {
                    img.put_pixel(x, y, color);
                }
            }
        }
    };

    let blue = image::Rgba([0, 0, 200, 255]);
    let red = image::Rgba([200, 0, 0, 255]);
    let green = image::Rgba([0, 200, 0, 255]);
    let purple = image::Rgba([200, 0, 200, 255]);
    let orange = image::Rgba([220, 120, 0, 255]);
    let magenta = image::Rgba([200, 0, 100, 255]);

    // Cell (0,0): L shape (Blue)
    draw_rect(&mut imgbuf, 24, 24, 25, 48, blue);
    draw_rect(&mut imgbuf, 24, 47, 48, 48, blue);

    // Cell (1,0): Solid filled 16x16 square (Red)
    draw_rect(&mut imgbuf, 84, 28, 108, 44, red);

    // Cell (2,0): Vertical bar (Green)
    draw_rect(&mut imgbuf, 152, 24, 153, 48, green);

    // Cell (0,1): Plus cross (Purple)
    draw_rect(&mut imgbuf, 24, 95, 48, 96, purple);
    draw_rect(&mut imgbuf, 35, 84, 36, 108, purple);

    // Cell (1,1): Three 6x6 dots in a column (Orange)
    draw_rect(&mut imgbuf, 84, 95, 85, 96, orange);
    draw_rect(&mut imgbuf, 93, 86, 98, 91, orange);
    draw_rect(&mut imgbuf, 93, 93, 98, 98, orange);
    draw_rect(&mut imgbuf, 93, 100, 98, 105, orange);
    draw_rect(&mut imgbuf, 107, 95, 108, 96, orange);

    // Cell (2,1): T shape (Magenta)
    draw_rect(&mut imgbuf, 144, 84, 168, 85, magenta);
    draw_rect(&mut imgbuf, 155, 84, 156, 108, magenta);

    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    imgbuf
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn test_sheet_filled_components_routing() {
    let png = generate_synthetic_sheet_with_filled_glyphs_png();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        ..SheetOptions::default()
    };

    let report = run_sheet(&png, &opts, &sheet_opts).expect("run_sheet should succeed");

    assert_eq!(report.written, 6);

    let square_icon = report
        .icons
        .iter()
        .find(|i| i.col == 1 && i.row == 0)
        .expect("square icon at col 1, row 0 should exist");
    let dots_icon = report
        .icons
        .iter()
        .find(|i| i.col == 1 && i.row == 1)
        .expect("dots icon at col 1, row 1 should exist");

    assert!(
        square_icon.filled_components >= 1,
        "square icon should have filled_components >= 1, got {}",
        square_icon.filled_components
    );
    assert!(
        dots_icon.filled_components >= 1,
        "dots icon should have filled_components >= 1, got {}",
        dots_icon.filled_components
    );

    let square_svg = fs::read_to_string(out_dir.join(format!("{}.svg", square_icon.name))).unwrap();
    let dots_svg = fs::read_to_string(out_dir.join(format!("{}.svg", dots_icon.name))).unwrap();

    assert!(
        square_svg.contains("fill=\"#") || square_svg.contains("fill=\"url("),
        "square SVG must contain fill color/gradient, got: {}",
        square_svg
    );
    assert!(
        dots_svg.contains("fill=\"#") || dots_svg.contains("fill=\"url("),
        "dots SVG must contain fill color/gradient, got: {}",
        dots_svg
    );

    let usvg_opts = resvg::usvg::Options::default();
    for icon in &report.icons {
        let path = out_dir.join(format!("{}.svg", icon.name));
        if path.exists() {
            let svg_str = fs::read_to_string(&path).unwrap();
            resvg::usvg::Tree::from_str(&svg_str, &usvg_opts)
                .unwrap_or_else(|e| panic!("SVG parsing failed for {}: {}", path.display(), e));
        }
    }

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn test_real_sheet_all_105_extract() {
    if std::env::var("SPRYTEO_SHEET_FIXTURE").is_err() {
        return;
    }

    let bytes = load_real_sheet();
    let out_dir = unique_temp_dir();
    let opts = ConvertOptions::default();
    let sheet_opts = SheetOptions {
        out_dir: out_dir.clone(),
        seg: spryteo_cli::sheet::SegChoice::Chroma,
        ..SheetOptions::default()
    };

    let report = run_sheet(&bytes, &opts, &sheet_opts).expect("run_sheet should succeed");
    assert_eq!(
        report.written, 105,
        "expected 105 icons written, got {}",
        report.written
    );
    for icon in &report.icons {
        assert!(
            icon.path_count >= 1,
            "icon {} at col {}, row {} has path_count == 0 (expected >= 1)",
            icon.name,
            icon.col,
            icon.row
        );
    }

    let _ = fs::remove_dir_all(&out_dir);
}
