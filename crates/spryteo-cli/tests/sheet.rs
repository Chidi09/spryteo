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
