# Semver commitment

Spryteo follows Cargo/semver conventions (`workspace.package.version` in
`Cargo.toml`) for the whole workspace: `crates/spryteo-core` is the
contract crate, and the two structures every other surface (CLI, npm,
WASM, MCP) is built around are `ConvertOptions` and the `Meta` sidecar
(`crates/spryteo-core/src/options.rs`, `crates/spryteo-core/src/ir.rs`).
This document is the commitment referenced by ROADMAP.md §6/§9's Phase 6
"semver commitment on ConvertOptions + meta schema" item.

## `ConvertOptions`

- **Adding a field** is a minor-version change, provided the field has a
  sensible `Default` (checked by `ConvertOptions::default()` staying
  buildable and every existing caller compiling unmodified). This is the
  common case and the reason every binding (wasm/node/mcp) partial-merges
  user JSON against `ConvertOptions::default()` rather than requiring a
  fully-specified object -- new fields never break old callers who don't
  know about them yet.
- **Removing or renaming a field**, or **changing a field's type** in a
  way that is not a strict widening (e.g. `u8` -> `u16` colour count is
  fine; `u8` -> `String` is not), is a major-version change.
- **Changing an enum's variants** (`Mode`, `ColorSpec`, `Layering`, `Tri`,
  `IdStyle`, `TOrigin`, `OutputFormat`, `Preset`) by adding a variant is
  minor; removing or renaming a variant is major, since `serde`'s default
  (non-`#[serde(other)]`) deserialization rejects unknown variant names on
  the way in, and any consumer matching exhaustively on the enum breaks on
  the way out. If a variant is ever deprecated, prefer keeping it as a
  parse-only alias (`#[serde(alias = "...")]`) for one major version
  before removing it.
- **Changing default *values*** (not fields/types, just what
  `ConvertOptions::default()` produces for an existing field) is treated
  as a minor-version change but is called out explicitly in the changelog,
  since it changes output for callers who didn't set that field --
  unlike a schema change it won't fail to compile/parse, it'll just
  silently produce different pixels/paths. The `tolerance` /
  `GRADIENT_LAB_TOLERANCE`-style constants discovered mid-build this
  session are exactly the kind of value that must not move without a
  changelog entry once this crate is 1.0.

## `Meta` sidecar (`Meta`, `NodeMeta`, `Stats`, `Bbox`)

This is the "agent interface" (ROADMAP.md §3.12) -- external tooling
(the MCP server's `inspect_svg`, the planned HTTP API, any script reading
`--json` output) is expected to parse it directly, so it gets the same
discipline as a public API, not just a Rust struct:

- **Adding a field** to `NodeMeta`/`Meta`/`Stats` is minor. Every
  existing consumer parsing this JSON with a permissive/partial parser
  (or just reading the fields it knows about) keeps working.
- **Removing or renaming a field**, or changing a field's JSON
  representation (e.g. `fill: Option<Rgb>` becoming a hex string) is
  major.
- **The `id` field's format** (`blake3(...)` hash, `s-` prefix, 8 hex
  chars per ROADMAP.md §3.12) is part of the contract: the stability
  guarantee is "re-running on a slightly edited source keeps unchanged
  shapes' IDs identical" (§3.12). Changing the hash algorithm, the input
  fields hashed, or the prefix/length is a major-version change even
  though the field's *type* (`String`) doesn't change, because it breaks
  the actual guarantee consumers rely on (referencing a shape by ID
  across re-runs).
- **`group` field values** (`g-s-<n>`, `g-mask-<mask.id>` per
  `spryteo-semantic`) are allowed to gain new prefixes/shapes as new
  grouping tiers are added (default containment-based, mask-guided) --
  this is additive and minor, since consumers should treat `group` as an
  opaque string key into the `SceneGraph`'s `<g id=...>` tree, not parse
  its internal structure.

- **`schema_version`** (`Meta::schema_version`, currently `1`) is how a
  consumer tells versions apart, and it should be branched on rather than
  sniffing for fields. A sidecar written before versioning existed has no
  such key and deserializes as `0`. Every field added since carries a
  serde default, which is what keeps those old payloads parsing; the
  fixture `testdata/meta_v0.json` and the test that reads it are there to
  make a regression on that point fail loudly rather than quietly.
- **The published schema** is generated from the Rust types, not written
  alongside them: `schema/meta.schema.json` and `schema/meta.d.ts`, with
  the TypeScript also shipped inside the Node and WASM packages. A test
  regenerates and compares them, so a change to a type that is not
  reflected in the published contract fails the build. Regenerate with
  `UPDATE_SCHEMA=1 cargo test -p spryteo-core`, and read a diff to those
  files as the question "is this a minor addition or a version bump?".
- **`suggested_draw_order`** is defined as a reveal order (nesting depth,
  then descending area, then paint order as a tiebreak) and is a
  permutation of the node list, distinct from `z_order`. Its *definition*
  may be refined in a minor version — it is a suggestion, and no consumer
  can depend on a particular ranking being stable across releases — but
  the guarantees that it is a permutation and that `z_order` alone
  describes rendering are part of the contract.

## Pre-1.0 exception

Before the `v1.0` tag (ROADMAP.md §9 Phase 6), the workspace version is
pinned at `0.0.1` and this document states *intent*, not an active
guarantee -- per Cargo/semver convention, `0.x` releases may break minor
versions. Once `v1.0.0` is tagged on crates.io/npm, the rules above
become binding for `ConvertOptions` and the `Meta` sidecar specifically
(internal, non-`pub` implementation details of individual pipeline
crates are never covered by this document).
