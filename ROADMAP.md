# Spryteo Engine — Implementation Roadmap

Raster → clean, animateable SVG. Rust core, four surfaces (CLI, npm library, HTTP API, WASM browser demo), agent-friendly by design.

This document is the complete spec: every stage, every knob, every acceptance gate. Phases are ordered so each one ships something usable on its own.

---

## 0. Promises already made (the site is the contract)

The landing page commits us to specific capabilities. These are requirements, not aspirations:

| Site copy | Hard requirement |
|---|---|
| "traces raster images and icons into **single continuous paths**" | Centerline/stroke tracing mode (§3.9) — outline tracing alone produces filled shapes, not continuous strokes |
| "**semantically grouped** and ready to animate" | Grouping layer with stable IDs + semantic segmentation (§3.11, §3.12) |
| "`npm install spryteo`" | Published npm package: napi-rs native addon with WASM fallback (§5.2) |
| "for the **agents** that call them" | Deterministic output, JSON metadata sidecar, MCP server (§5.5) |
| Hero demo animates `stroke-dasharray` draw-on | Output must support `pathLength` normalization and stroke-based paths (§3.12) |
| Drop zone accepts png/jpg/svg/gif/webp, 8MB | Decoder support for all five; note Vercel function body limit is ~4.5MB — see §5.4 |

## 1. Architecture: pipeline of typed intermediate representations

Every stage is a pure function on a typed IR. This is what makes the CLI, API, library, and WASM builds share one core with zero divergence, and what makes each stage independently testable.

```
bytes
  → RasterImage        (decode + normalize: sRGB, premultiplied alpha resolved, EXIF applied)
  → ClassifiedInput    (icon | pixel-art | line-art | photo, auto or forced)
  → LayerStack         (N quantized color layers, each a soft mask, with z-order)
  → ContourSet         (per layer: polygons with hole hierarchy, subpixel coords)
  → CurveSet           (per contour: Bezier paths + recognized primitives)
  → SceneGraph         (grouped, ID'd, z-ordered nodes with fills/strokes/transforms)
  → SvgDocument + Meta (serialized SVG + JSON sidecar)
```

Rules:
- No stage reaches backward or holds global state. Config in, IR in, IR out.
- Every IR is serializable (serde) — enables `--dump-ir` debugging, golden tests per stage, and pipeline caching.
- Determinism is a hard invariant: same input + same options → **byte-identical** SVG. No HashMap iteration order in any output path (use BTreeMap/stable sorts), fixed RNG seeds, order-independent parallel reduction.

## 2. Workspace layout

Monorepo restructure: the Astro site moves to `site/`, engine lives in `crates/`.

```
spryteo/
├── site/                     # current Astro site, moved as-is
├── crates/
│   ├── spryteo-core/         # IR types, ConvertOptions, pipeline orchestration, traits
│   ├── spryteo-raster/       # decode (png/jpeg/gif/webp/bmp), EXIF, ICC→sRGB, preprocess
│   ├── spryteo-quant/        # color quantization, layering, palette handling
│   ├── spryteo-trace/        # contour extraction (Suzuki-Abe, marching squares)
│   ├── spryteo-fit/          # polygon simplification, corner detection, Bezier fitting
│   ├── spryteo-geom/         # primitive recognition, geometry utils, hashing
│   ├── spryteo-stroke/       # centerline tracing: skeleton, graph, Eulerian paths
│   ├── spryteo-semantic/     # SAM/ONNX segmentation, grouping heuristics (feature-gated)
│   ├── spryteo-svg/          # SceneGraph → SVG emit, optimizer, meta sidecar
│   ├── spryteo-cli/          # clap binary
│   └── spryteo-api/          # axum server
├── bindings/
│   ├── node/                 # napi-rs addon → npm "spryteo"
│   └── wasm/                 # wasm-bindgen → npm "@spryteo/wasm" + browser demo
├── testdata/                 # golden corpus (see §6)
└── fuzz/
```

- Edition 2024, MSRV pinned. `spryteo-semantic` behind a cargo feature (`semantic`) — it pulls ONNX runtime and must never bloat the WASM/icon path.
- Error handling: `thiserror` per crate, one public `SpryteoError` at the core boundary. **No panics across FFI** — catch_unwind at napi/wasm/C boundaries.
- Parallelism: `rayon` per-layer and per-contour, feature-gated off for WASM.

## 3. Stage-by-stage specification

### 3.1 Decode & normalize (`spryteo-raster`)

- Formats: PNG, JPEG, GIF (first frame; animated-GIF → animated-SVG is a v2 idea, out of scope), WebP, BMP. Crates: `image`, `zune-jpeg` if perf demands.
- Sniff by magic bytes, never extension/Content-Type.
- EXIF orientation applied; ICC profile → sRGB conversion (`lcms2` or `moxcms`); 16-bit → 8-bit with correct rounding.
- Alpha: un-premultiply if needed. Options: `alpha_mode = keep | matte(color) | threshold(0..255)`. Default: keep, treat alpha as a soft mask in quantization.
- **Decompression-bomb guards**: max input bytes (default 8MB lib / configurable), max pixel count (default 16MP), max dimension (8192). Reject before allocation, not after.
- `max_trace_dimension` (default 1024 for photos, none for icons): downscale with Lanczos3 before tracing; emit coordinates scaled back to original viewBox.
- SVG input: passthrough mode — parse (`usvg`), re-group/re-ID/optimize only. Don't rasterize-and-retrace vector input.

### 3.2 Input classification

Auto mode picks the pipeline profile; every heuristic overridable via `--mode`.

- **pixel-art**: dimension ≤ 128 AND unique colors ≤ 64 AND no anti-aliasing detected (edge pixels are hard transitions). → No smoothing; trace exact pixel boundaries with corner-preserving fit; optional `pixel_art_style = crisp | smoothed`.
- **icon**: unique colors after 1% noise floor ≤ 32, or alpha channel with flat fills. → Few-color quantization, aggressive primitive recognition.
- **line-art**: after adaptive threshold, ink ratio < 25% and stroke-width histogram is unimodal. → Candidate for centerline mode (§3.9); default still outline unless `--stroke`.
- **photo**: everything else. → Layered quantization + gradient detection.

### 3.3 Preprocessing

- Bilateral filter (edge-preserving) — σ_color/σ_space derived from image size; skipped for pixel-art.
- JPEG deblocking pass when input was JPEG and quality estimate < 90.
- Despeckle at the region level happens later (turdsize, §3.5) — do NOT median-filter icons.
- Optional `remove_background`: flood-fill from the four corners with tolerance; if ≥ 3 corners agree on a color, mark as background layer (emitted as none or as a rect, `background = drop | keep | rect`).
- Optional `autocrop` to content bbox (post background detection), viewBox adjusted.

### 3.4 Quantization (`spryteo-quant`)

- Color space: **CIELAB** for all clustering distance math (perceptual). Convert once, operate on Lab, keep sRGB for output.
- Icons: exact histogram if unique colors ≤ `max_colors`; else k-means (k-means++ init, **fixed seed** for determinism) or Wu quantizer for speed.
- Photos: hierarchical layering, two modes (study vtracer's implementation — MIT, safe to read):
  - `layering = stacked` (default): layers painted bottom-up, each layer's region includes everything above it. Fewer sliver artifacts, cleaner boundaries, natural z-order.
  - `layering = cutout`: exact disjoint regions. Smaller output, worse edges.
- `colors = auto | N (2..=64)`: auto via elbow on within-cluster error (deterministic tie-break).
- `palette = [hex,...]`: user-supplied palette override (brand colors) — nearest-Lab assignment, no clustering.
- Alpha-aware: pixels with alpha < threshold excluded from clustering; soft alpha edges preserved into the layer masks for §3.5 subpixel work.

### 3.5 Contour extraction (`spryteo-trace`)

- Per layer: connected-component labeling (8-connectivity for ink, 4 for holes — or configurable), then **Suzuki–Abe** border following producing the full parent/child hole hierarchy. Fill-rule: emit `fill-rule="evenodd"` with holes as reversed subpaths *of the same path element* (a letter "O" is one path, two subpaths — holes are the one legitimate use of subpaths).
- **Anti-aliasing / subpixel boundaries** — this is a make-or-break detail the naive plan misses. Anti-aliased edges lie *between* pixels; tracing the binary mask gives staircase noise that curve fitting then fights. Approach: run **marching squares with linear interpolation** on the layer's soft mask (coverage 0..1) at iso-level 0.5 → contour vertices at subpixel positions. Fall back to pixel-boundary tracing (Potrace-style) only for hard-edged inputs (pixel-art mode).
- Turn policy for 4-way saddle ambiguities (Potrace's `turnpolicy`): `minority` default, options black/white/left/right/majority.
- Despeckle: drop regions with area < `turdsize` px² (default 2; scaled by downscale factor).

### 3.6 Polygon simplification & corner detection (`spryteo-fit`)

Reimplement Potrace's algorithm **from the published paper** (Selinger 2003, "Potrace: a polygon-based tracing algorithm") — the paper is free to use; the C source is **GPL, do not read or port it** if the project ships MIT/Apache (§10).

- Stage 1 — optimal polygon: for each contour, compute the set of "straight" subpaths (a subpath is straight if all its points lie within 0.5px of some line, with the sign-constraint refinement from the paper). Find the polygon with the **fewest vertices**, tie-broken by least squared deviation, via the paper's cyclic shortest-path formulation.
- Stage 2 — corner detection: at each polygon vertex compute α from adjacent segments; `alphamax` (default 1.0, range 0..1.34) decides corner (sharp vertex kept) vs smooth (curve through it). Expose as `smoothness`.
- Not Douglas–Peucker. DP is the shortcut that produces the "blobby or jagged, pick one" failure mode; the straightness-cost formulation is the differentiator.

### 3.7 Bezier fitting

- Between corners: constrained least-squares cubic Bezier fit over the subpixel contour points, **G1 continuity** (shared tangent direction) enforced at smooth joins.
- Curve optimization pass (Potrace's `opttolerance`, default 0.2): greedily merge consecutive Beziers into one where the merged curve stays within tolerance. This is where node counts drop 30–50%.
- `tolerance` (px, default 0.5 icons / 1.0 photos): global fit-error budget. The single most important user-facing quality knob.
- Numeric hygiene: all coordinates finite, clamped to viewBox + margin; property-tested (§6).

### 3.8 Primitive recognition (`spryteo-geom`)

After fitting, attempt promotion of each closed path (order matters — most specific first):
- Circle: all curve points within tolerance of best-fit circle (Kåsa/Taubin fit) → `<circle>`.
- Ellipse (axis-aligned + rotated) → `<ellipse>`.
- Rectangle / rounded-rect: 4 straight edges + 90° corners (± tolerance), corner arcs equal radius → `<rect rx=…>`.
- Straight-line polyline / polygon → `<path>` with `L` only (or `<polygon>`).
- Arcs within paths: consecutive Beziers approximating a circular arc → `A` segments (flag: `arcs = on|off`, some animation tooling hates arcs — default off, emit metadata noting detected arcs instead).
- Primitives matter doubly here: they're semantically meaningful for animators (`<circle>` can be animated via `r`) and they shrink output.

### 3.9 Centerline / stroke mode (`spryteo-stroke`) — the "single continuous path" promise

Outline tracing turns a drawn line into a filled sausage (two edges). Draw-on animation (`stroke-dasharray`) needs true **stroked centerlines**. Separate mode: `--stroke` / `stroke: true`, auto-suggested when classifier says line-art.

Pipeline:
1. Binarize (adaptive threshold, Sauvola) → ink mask.
2. Skeletonize: distance-transform-guided thinning (Zhang–Suen acceptable v1; medial-axis from the distance transform is the quality upgrade). Record distance value at each skeleton pixel = local half-width.
3. Prune spurs shorter than `spur_factor × local_width` (default 1.5).
4. Build the stroke graph: nodes at endpoints/junctions, edges are pixel chains.
5. Path traversal: if the graph has 0 or 2 odd-degree nodes → single **Eulerian path** (Hierholzer) = literally one continuous path. Otherwise: minimal set of open paths via greedy odd-node pairing (full Chinese Postman is v2). Report path count in meta.
6. Smooth each chain (§3.6–3.7 machinery on open curves), emit `<path fill="none" stroke=… stroke-width=…>` with round caps/joins.
7. Stroke width: median of local widths per edge; `stroke_width = auto | uniform | variable` (variable = split path where width changes > 30%; true variable-width needs outline fallback per segment).
8. Always emit `pathLength="100"` in stroke mode (§3.12).

### 3.10 Gradient detection (photo mode)

Prevents the banding failure mode. Per quantized region (or per pair of adjacent same-hue layers):
- Fit a linear ramp in Lab over the region's original pixels (least squares on position → color). If residual < threshold → replace flat fill with `<linearGradient>` (2-stop; 3-stop if residual profile demands).
- Radial: fit color-vs-distance from centroid; same residual test → `<radialGradient>`.
- Merge adjacent quantization layers that a single gradient explains (this actively reduces layer count).
- Flag: `gradients = auto | off` (default auto in photo mode, off in icon mode). Mesh gradients: out of scope (poor renderer support).

### 3.11 Semantic grouping (`spryteo-semantic`, feature-gated)

- Default grouping (no ML, always available): connected component → `<g>`; nested containment (shape fully inside another) → child group; z-order from layer stacking. This alone beats every off-the-shelf tool.
- ML tier (opt-in): **MobileSAM / FastSAM via `ort` (ONNX Runtime)**, CPU-viable (~40MB encoder). Masks are computed on the *original raster before quantization*; each mask becomes a grouping boundary — color regions are assigned to the object whose mask covers them ≥ 60%. Result: "sun" and "sky" separate even at similar colors.
- Models are never bundled: `spryteo models pull sam` downloads to XDG cache; CLI/API degrade gracefully without them. Optional remote inference (`--semantic remote --endpoint …`) for the hosted API later.
- Optional labeling pass (v2): lightweight tagger to name groups (`#sun`, `#sky`) instead of hashes.

### 3.12 Animateability layer (SceneGraph rules)

The layer that justifies the product. Non-negotiables:
- **Never boolean-merge distinct shapes** sharing a fill. One shape = one `<path>`, always.
- **Stable IDs**: `blake3(rounded geometry + fill + z-index)` → first 8 hex chars, prefixed (`s-3fa9c21b`). Deterministic; re-running on a slightly edited source keeps unchanged shapes' IDs identical. Collisions get `-2` suffix. `id_style = hash | sequential | none`.
- Groups: `<g id=…>` per connected component / semantic object; z-order = paint order, never reordered.
- Transforms: shape's centroid offset expressed as `transform="translate(x y)"` with local coordinates around origin (`transform_origin = baked | centroid`, default centroid) — so `rotate()`/`scale()` in CSS behave sanely without `transform-box` gymnastics.
- `pathLength="100"` on all stroke paths (draw-on animations become `stroke-dasharray: 100`).
- **Metadata sidecar** (JSON, also returned by API/lib): every ID with bbox, centroid, area, fill, group tree, z-order, suggested draw order (topological: back-to-front, big-to-small), stats (nodes, paths, bytes). This is the agent interface.
- Optional emitted presets (`--css draw|fade|pop`): a small CSS block targeting the generated IDs. Marketing gold, trivial to implement last.

### 3.13 SVG emit & optimize (`spryteo-svg`)

- Own emitter (determinism + control), not a generic XML lib's pretty-printer.
- `precision` decimal places (default 2; 1 for icons ≤ 48px viewBox after scaling analysis), relative path commands where shorter, collapsed command repetition, `viewBox` always, no width/height by default (`--dimensions` to include).
- Built-in SVGO-equivalents only where lossless *and* group-safe: numeric precision, path data minification, remove empty groups (except ID'd ones), dedupe gradient defs. **No path merging across groups. Ever.**
- Output modes: `svg` (minified), `svg-pretty`, `jsx` (React component export — string transform of attributes, cheap feature, high perceived value).
- `sanitize`: output is generated, never echoed from input — no script/foreignObject can appear by construction. Assert in tests anyway.

## 4. The single options surface

One `ConvertOptions` struct, mirrored 1:1 in CLI flags, npm options object, API JSON body, and MCP tool schema. Defined once in `spryteo-core`, serde-serializable, versioned:

```rust
pub struct ConvertOptions {
    pub mode: Mode,                  // Auto | Icon | PixelArt | LineArt | Photo
    pub stroke: bool,                // centerline mode (§3.9)
    pub colors: ColorSpec,           // Auto | N(u8) | Palette(Vec<Rgb>)
    pub layering: Layering,          // Stacked | Cutout
    pub tolerance: f32,              // curve fit budget, px
    pub smoothness: f32,             // alphamax mapping, 0..1
    pub turdsize: u32,               // min region area px²
    pub gradients: Tri,              // Auto | On | Off
    pub grouping: Grouping,          // Component | Semantic | Flat
    pub id_style: IdStyle,           // Hash | Sequential | None
    pub transform_origin: TOrigin,   // Centroid | Baked
    pub precision: u8,               // decimals
    pub max_trace_dimension: Option<u32>,
    pub background: Background,      // Keep | Drop | Rect
    pub alpha_mode: AlphaMode,
    pub arcs: bool,
    pub output: OutputFormat,        // Svg | SvgPretty | Jsx
    pub emit_css: Option<Preset>,    // Draw | Fade | Pop
    // limits (enforced in core, not just at edges):
    pub max_pixels: u64, pub max_input_bytes: u64, pub timeout_ms: Option<u64>,
}
```

Result: `ConvertResult { svg: String, meta: Meta }` everywhere. Cancellation token threaded through stages (API timeouts must abort mid-trace, not after).

## 5. Surfaces

### 5.1 CLI (`spryteo-cli`)
- `spryteo convert in.png -o out.svg [--mode icon] [--stroke] [--colors 8] [--tolerance 0.5] [--group semantic] [--json meta.json] …` — every ConvertOptions field as a flag.
- `spryteo convert 'icons/*.png' --out-dir svg/` batch (rayon), `--watch`, stdin/stdout piping (`-` convention) for agent/script use.
- `spryteo inspect out.svg` — print meta from an SVG we generated (IDs embedded as a data attribute or re-derived).
- `spryteo models pull|ls|rm` — semantic models.
- Exit codes: 0 ok, 1 invalid input, 2 limits exceeded, 3 internal. `--quiet`/`--json` for machine consumption.

### 5.2 npm package `spryteo` (bindings/node)
- napi-rs addon; prebuilt binaries per platform (darwin-arm64/x64, linux-x64/arm64 gnu+musl, win32-x64) via CI matrix, published as `optionalDependencies` (the esbuild/swc pattern). Postinstall-free.
- WASM fallback (`@spryteo/wasm`) auto-selected when no native binary matches.
- API: `convert(input: Buffer | Uint8Array, options?): Promise<{ svg: string, meta: Meta }>` + `convertSync`. TypeScript types generated from the Rust structs (napi derives + a check that they stay in sync with serde JSON).

### 5.3 WASM browser build (bindings/wasm)
- wasm-bindgen, no rayon (single-thread; wasm threads behind a flag later), `semantic` feature off. Target < 800KB gzipped.
- **Powers the hero demo for real**: the site's drop zone runs actual conversion client-side — zero server cost, instant, works offline. This replaces the `convertIcon()` stub in `site/src/components/ClientScript.astro` (stub already structured for this swap). Downscale to ≤ 512px in the demo for latency.

### 5.4 HTTP API (`spryteo-api`)
- axum: `POST /v1/convert` (multipart or raw body + query/JSON options) → `{ svg, meta }`; `GET /v1/health`; content sniffing, per-request limits, timeout via cancellation token, structured errors `{ code, message }`.
- Hosting reality check: the site is static Astro on Vercel. Options, in order:
  1. **v1**: no server API at all — WASM in the browser covers the demo; npm/CLI cover developers. Ship this first.
  2. **v1.5**: Vercel Node function running the WASM build at `/api/convert` for programmatic users. **Vercel body limit ≈ 4.5MB — the site's current 8MB client cap must drop to 4MB, or uploads go via Vercel Blob presigned URLs.** (Flagged: `ClientScript.astro` `MAX_SIZE_BYTES` currently 8MB.)
  3. **v2**: dedicated container (Fly/Railway) for native perf + semantic models; rate limiting + API keys only at this stage.

### 5.5 MCP server
- Thin wrapper over the core: tools `convert_image` (path/base64 + options → svg + meta) and `inspect_svg`. Determinism + meta sidecar make results reproducible and referenceable by ID in agent workflows. Ship after npm package (it's ~a day of work on top of it).

## 6. Quality, testing, evaluation

- **Golden corpus** (`testdata/`): ~30 icons (Feather, Material, Noto emoji rasterized at 24/48/512px), 10 logos, 10 pixel-art sprites, 10 line drawings, 15 photos (portraits, landscapes, gradients, flat-design illustrations). Each with committed expected SVG.
- **Render-back metric**: rasterize output via `resvg` at source resolution, compare to input. Gates: SSIM ≥ 0.92 icons, ≥ 0.85 photos, ≥ 0.90 line-art (against binarized input). Plus budgets per fixture: max nodes, max paths, max bytes.
- **Determinism test**: every fixture converted twice (and once with `--threads 1` vs N) → byte-identical.
- **Property tests** (proptest): random small rasters → output parses (usvg), all coords finite and in-bounds, fill-rule hole correctness (point-in-polygon sampling), no empty paths.
- **Fuzzing**: cargo-fuzz on decode + full pipeline with malformed/adversarial images (you already run this pattern in crush).
- **Stage golden tests**: serialized IRs per stage for 5 canonical fixtures, so a regression pinpoints its stage.
- CI: fmt, clippy (deny warnings), tests, corpus gates, wasm + napi builds on every PR. Corpus gate failures print SSIM/node-count diffs.

## 7. Security & robustness

- Decompression bombs: byte/pixel/dimension caps before allocation (§3.1). Timeouts as cancellation, enforced inside the pipeline.
- Magic-byte sniffing only; multipart filenames untrusted and unused.
- API: no URL-fetch feature in v1 (SSRF surface); if added, allowlist schemes + deny private ranges.
- Generated-only output guarantee (no input bytes ever copied into SVG except color values) — asserted in tests.
- FFI boundaries: catch_unwind, never abort the host process (Node/browser).

## 8. Performance targets (4-core x86, release)

| Case | Target |
|---|---|
| 256² icon, ≤ 8 colors | < 50 ms |
| 1024² photo, 12 layers | < 1.5 s |
| 2048² photo (auto-downscaled to 1024) | < 2 s |
| WASM, 512² icon (browser demo) | < 400 ms |
| Peak memory | < 6× decoded image bytes |

Benchmarks in CI (criterion) with regression alerts, not hard gates.

## 9. Phased delivery

Each phase ends with something shippable. Estimates assume one experienced Rust dev, focused.

**Phase 0 — Foundations (≈ 1 week)**
Monorepo restructure (site → `site/`, git init if not done), crate skeletons, IR types + ConvertOptions in core, CI, resvg render-back harness, golden corpus collected and committed.
*Gate: `cargo test` green, corpus harness runs end-to-end on a stub pipeline.*

**Phase 1 — Icon mode, excellent (≈ 3–4 weeks)**
Decode/normalize (§3.1), classifier v1 (§3.2), few-color quantization (§3.4 icon path), Suzuki-Abe + marching-squares subpixel contours (§3.5), the Potrace-class polygon/corner/Bezier stack (§3.6–3.7) — *this is the hard month; budget for it* — basic primitives (circle/rect) (§3.8), grouping-by-component + stable IDs + emit (§3.12–3.13), CLI (§5.1).
*Gate: full icon corpus passes SSIM/node/determinism gates; output visibly beats vtracer defaults on the corpus (side-by-side page generated by CI).*

**Phase 2 — Stroke mode (≈ 2 weeks)**
§3.9 complete: skeleton, prune, graph, Eulerian traversal, width estimation, `pathLength`. Pixel-art profile polish. `--css draw` preset.
*Gate: line-art corpus ≥ 0.90; a hand-drawn signature converts to ≤ 3 continuous paths and draw-animates correctly in a browser.*

**Phase 3 — Ship the surfaces (≈ 2 weeks)**
napi-rs npm package with prebuilt binaries (§5.2), WASM build (§5.3), **hero demo wired to real WASM conversion** (replace `convertIcon` stub), docs page updated with real API reference, JSX output mode. Optional: Vercel function `/api/convert` (fix the 8MB→4MB cap mismatch or move to Blob uploads).
*Gate: `npm install spryteo && node -e "…convert…"` works on macOS/Linux/Windows CI; dropping a PNG on spryteo.vercel.app returns a real SVG.*

**Phase 4 — Photo mode (≈ 3–4 weeks)**
Layered quantization stacked/cutout (§3.4), gradient detection (§3.10), bilateral/deblock preprocessing tuned, auto color-count, palette override, performance pass (rayon, downscale path).
*Gate: photo corpus ≥ 0.85 SSIM within byte budgets; gradient fixtures produce `<linearGradient>` not banding.*

**Phase 5 — Semantic tier + agents (≈ 3 weeks)**
`ort` + MobileSAM integration, model pull UX, mask-guided grouping (§3.11), meta sidecar finalized as stable v1 schema, MCP server (§5.5), `spryteo inspect`.
*Gate: photo of sun+sky groups into separately-animateable `<g>`s; MCP tool drives a full convert from Claude.*

**Phase 6 — Hardening & v1.0 (≈ 2 weeks)**
Fuzz campaign, perf regression suite, error-message audit, docs (every option documented with before/after images), semver commitment on ConvertOptions + meta schema, LICENSE audit, v1.0 tags npm+crates.io.

Total: ~4 months solo. Phases 1–3 (~2 months) already deliver the differentiated product the landing page sells.

## 10. Licensing constraints

- Ship MIT OR Apache-2.0 dual (site says "free & open source").
- **Potrace C source is GPL-2.0: do not read, copy, or port it.** Implement from Selinger's paper only (algorithms aren't copyrightable; code is). Keep a NOTICE documenting clean-room provenance.
- vtracer: MIT — safe to read and learn from. `image`, `resvg`/`usvg`, `rayon`, `axum`, napi-rs: MIT/Apache, fine. ONNX Runtime: MIT. MobileSAM weights: Apache-2.0 (verify at integration time). `lcms2` crate wraps LGPL lcms — either dynamic-link stance or use a pure-Rust ICC crate (`moxcms`, Apache) to stay clean.

## 11. Immediate site touchpoints (already in this repo)

- `site/src/components/ClientScript.astro` → `convertIcon(file)` stub is the WASM/API integration point (Phase 3).
- `MAX_SIZE_BYTES` 8MB there vs Vercel's 4.5MB function limit — resolve in Phase 3 (§5.4).
- Hero copy "single continuous paths" is honest only after Phase 2 — keep it (it's the roadmap), but the live demo should run icon mode until then.
- `github` links across the site are placeholders — point them at the engine repo when Phase 0 lands.
