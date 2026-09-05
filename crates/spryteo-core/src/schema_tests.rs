use super::*;
use std::path::{Path, PathBuf};

fn schema_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is crates/spryteo-core.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schema")
        .canonicalize()
        .expect("the schema directory is checked in")
}

/// Compare generated output against the copy in the repository, or rewrite
/// it when `UPDATE_SCHEMA=1`.
fn check_or_update(path: &Path, generated: &str) {
    if std::env::var("UPDATE_SCHEMA").is_ok() {
        std::fs::write(path, generated)
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        return;
    }
    let checked_in = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    assert_eq!(
        checked_in,
        generated,
        "{} is out of date with the Rust types. \
         Regenerate it with `UPDATE_SCHEMA=1 cargo test -p spryteo-core`, \
         and treat any change to an existing field as a schema version bump.",
        path.display(),
    );
}

fn repo_root() -> PathBuf {
    schema_dir()
        .parent()
        .expect("the schema directory has a parent")
        .to_path_buf()
}

#[test]
fn the_published_json_schema_matches_the_types() {
    check_or_update(&schema_dir().join("meta.schema.json"), &meta_json_schema());
}

#[test]
fn the_published_typescript_matches_the_types() {
    let ts = meta_typescript();
    check_or_update(&schema_dir().join("meta.d.ts"), &ts);

    // An npm package cannot reference a file outside its own directory, so
    // each binding ships its own copy. Checking them here is what keeps the
    // copies from becoming stale forks of the contract.
    for binding in ["bindings/node", "bindings/wasm"] {
        check_or_update(&repo_root().join(binding).join("meta.d.ts"), &ts);
    }
}

#[test]
fn the_typescript_declares_every_named_type() {
    let ts = meta_typescript();
    for name in [
        "Meta",
        "NodeMeta",
        "GroupMeta",
        "Stats",
        "Bbox",
        "Rgb",
        "GradientStop",
        "PaintMeta",
        "StrokeMeta",
        "ShapeKind",
    ] {
        assert!(
            ts.contains(&format!("export interface {name} "))
                || ts.contains(&format!("export type {name} ")),
            "{name} is missing from the generated TypeScript"
        );
    }

    // The tagged unions must survive as unions, not collapse into a single
    // loose object: `kind` is what a consumer switches on.
    assert!(ts.contains(r#""linear-gradient""#));
    assert!(ts.contains(r#""radial-gradient""#));
    assert!(ts.contains(r#""current-color""#));
    assert!(ts.contains(r#""path" | "circle" | "ellipse" | "rect""#));
}

/// A sidecar written before the schema carried a version, kept verbatim.
///
/// Every field added since is optional, and this fixture is the standing
/// proof: if a later change makes one of them required, this stops parsing
/// and the version has to be bumped with a migration note rather than
/// silently breaking whatever is already reading these files.
const META_V0: &str = include_str!("../../../testdata/meta_v0.json");

#[test]
fn a_pre_versioning_sidecar_still_deserializes() {
    use crate::ir::{META_SCHEMA_UNVERSIONED, META_SCHEMA_VERSION};

    let meta: crate::ir::Meta = serde_json::from_str(META_V0).expect("v0 sidecars still parse");

    assert_eq!(meta.schema_version, META_SCHEMA_UNVERSIONED);
    assert_ne!(META_SCHEMA_VERSION, META_SCHEMA_UNVERSIONED);
    assert_eq!(meta.nodes.len(), 1);
    assert_eq!(meta.nodes[0].id, "s-abc123");
    assert_eq!(meta.nodes[0].z_order, 0);
    assert_eq!(meta.stats.byte_count, 128);

    // Absent fields read as their defaults rather than failing.
    let node = &meta.nodes[0];
    assert!(node.group_path.is_empty());
    assert!(node.paint.is_none());
    assert!(node.stroke.is_none());
    assert_eq!(node.shape, crate::ir::ShapeKind::Path);
    assert!(!node.closed);
    assert_eq!(node.path_length, 0.0);
    assert!(meta.groups.is_empty());
}

#[test]
fn a_current_sidecar_round_trips_through_json() {
    use crate::ir::{Meta, META_SCHEMA_VERSION};

    // Start from the v0 fixture so the round trip covers defaulted fields
    // as well as written ones.
    let mut meta: Meta = serde_json::from_str(META_V0).unwrap();
    meta.schema_version = META_SCHEMA_VERSION;
    meta.nodes[0].paint = Some(crate::ir::PaintMeta::LinearGradient {
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
        stops: vec![
            crate::ir::GradientStop {
                offset: 0.0,
                color: crate::ir::Rgb { r: 0, g: 0, b: 0 },
            },
            crate::ir::GradientStop {
                offset: 1.0,
                color: crate::ir::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            },
        ],
    });

    let json = serde_json::to_string(&meta).unwrap();
    let back: Meta = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back, "the sidecar must survive a JSON round trip");

    // The gradient survives as a gradient, not as its first stop.
    assert!(json.contains("linear-gradient"));
}
