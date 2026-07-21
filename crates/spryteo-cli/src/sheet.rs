use serde::{Deserialize, Serialize};
use spryteo_core::{ConvertOptions, SpryteoError};
pub use spryteo_sheet::{
    IconRegularizeReport, IconReport, PipelineOptions, SegChoice, SheetOutcome, SheetReport,
};
use std::path::PathBuf;

/// Options configuring icon contact sheet extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetOptions {
    pub out_dir: PathBuf,
    pub supersample: u32,    // default 4
    pub canonical_size: f64, // default 24.0
    pub pad: u32,            // default 2
    pub flat: bool,          // emit currentColor instead of a gradient
    pub sat_threshold: Option<u8>,
    pub name_prefix: String, // default "icon"
    pub manifest: Option<PathBuf>,
    pub dry_run: bool,
    pub regularize: bool, // default false
    pub grid_pitch: f64,  // default 1.0
    pub seg: SegChoice,
    pub unify_widths: bool, // default true
}

impl Default for SheetOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::new(),
            supersample: 4,
            canonical_size: 24.0,
            pad: 2,
            flat: false,
            sat_threshold: None,
            name_prefix: "icon".to_string(),
            manifest: None,
            dry_run: false,
            regularize: false,
            grid_pitch: 1.0,
            seg: SegChoice::Auto,
            unify_widths: true,
        }
    }
}

pub fn run_sheet(
    bytes: &[u8],
    opts: &ConvertOptions,
    sheet: &SheetOptions,
) -> Result<SheetReport, SpryteoError> {
    let pipeline_opts = PipelineOptions {
        supersample: sheet.supersample,
        canonical_size: sheet.canonical_size,
        pad: sheet.pad,
        flat: sheet.flat,
        sat_threshold: sheet.sat_threshold,
        name_prefix: sheet.name_prefix.clone(),
        regularize: sheet.regularize,
        grid_pitch: sheet.grid_pitch,
        seg: sheet.seg,
        unify_widths: sheet.unify_widths,
    };

    let outcome = spryteo_sheet::run_sheet_pipeline(bytes, opts, &pipeline_opts)?;
    let SheetOutcome { mut report, icons } = outcome;

    if !sheet.dry_run {
        if let Err(e) = std::fs::create_dir_all(&sheet.out_dir) {
            return Err(SpryteoError::Internal(format!(
                "Failed to create output directory '{}': {}",
                sheet.out_dir.display(),
                e
            )));
        }
    }

    if !sheet.dry_run {
        let mut written_count = 0usize;
        for icon in icons {
            let out_filename = format!("{}.svg", icon.name);
            let out_path = sheet.out_dir.join(&out_filename);

            if let Err(e) = std::fs::write(&out_path, &icon.svg) {
                if let Some(rep) = report.icons.iter_mut().find(|r| r.name == icon.name) {
                    rep.warnings.push(format!("Failed to write SVG: {}", e));
                }
            } else {
                written_count += 1;
            }
        }
        report.written = written_count;
    } else {
        report.written = 0;
    }

    if let Some(ref manifest_path) = sheet.manifest {
        if !sheet.dry_run {
            if let Some(parent) = manifest_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let json_str = serde_json::to_string_pretty(&report).map_err(|e| {
                SpryteoError::Internal(format!("Failed to serialize manifest JSON: {}", e))
            })?;
            std::fs::write(manifest_path, json_str).map_err(|e| {
                SpryteoError::Internal(format!(
                    "Failed to write manifest to '{}': {}",
                    manifest_path.display(),
                    e
                ))
            })?;
        }
    }

    Ok(report)
}
