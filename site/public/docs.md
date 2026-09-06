# Spryteo Documentation

Spryteo converts raster images and icons into clean, animateable SVG —
traced, quantized, curve-fit, and semantically grouped, entirely offline.
Install it, then reach for whichever surface fits your workflow.

## Ways to use it

Same engine, same output, four entry points:

- **CLI** (terminal) — one binary, point it at a file, get an SVG back.
- **Library** (node · npm) — native addon (napi-rs), same engine, no subprocess.
- **Browser** (wasm) — runs client-side, no upload, no server round-trip.
- **MCP server** (agents) — two tools agents can call mid-conversation.

## Install

Requires Node 18 or newer for the npm-distributed CLI/library/WASM builds.
The Rust crates build with any current stable toolchain if you're working
from source.

```
$ npm install -g spryteo
# or, as a project dependency
$ npm install spryteo
```

## CLI

Point it at a raster file. It writes a grouped, animateable SVG next to the
source, plus an optional JSON metadata sidecar.

```
$ spryteo convert ./icon.png -o ./icon.svg --mode icon --colors 8
  traced 1 path · 3 groups · 2.1 kb → ./icon.svg
```

Full flag reference:

| Flag | Description |
|---|---|
| `-o, --output <path>` | Output SVG path. Required. |
| `--mode <auto\|icon\|pixel-art\|line-art\|photo>` | Override input classification. Default: auto. |
| `--stroke` | Run the centerline tracer instead of fill-mode outlining. |
| `--css <draw\|fade\|pop>` | Bake one of the built-in CSS animation presets into the SVG. |
| `--colors <n>` | Target palette size for quantization — a ceiling, not a guarantee: visually indistinguishable clusters are merged after quantization, so the final count can be lower. |
| `--layering <stacked\|cutout>` | Photo-mode layer composition. |
| `--gradients <auto\|on\|off>` | Gradient-fill detection. auto only tries it in photo mode. |
| `--tolerance <px>` | Curve-fit error budget, in pixels. Lower = more path points. |
| `--smoothness <n>` | Corner-preservation strength for the Bezier fit. |
| `--turdsize <px²>` | Discard regions smaller than this area (noise suppression). |
| `--precision <n>` | Decimal places in emitted path coordinates. |
| `--pretty` | Pretty-print the SVG instead of minifying it. |
| `--jsx` | Emit a React JSX component instead of plain SVG. |
| `--current-color` | Monochrome output inherits colour via currentColor. |
| `--background <keep\|drop\|rect>` | Background treatment: keep as a layer, drop it, or emit as `<rect>`. |
| `--alpha-mode <keep\|matte:#rrggbb\|threshold:0-255>` | Alpha handling: preserve, matte against a colour, or hard-cut at threshold. |
| `--grouping <component\|semantic\|flat>` | Grouping strategy: connected component, ML mask-guided, or no grouping. |
| `--id-style <hash\|sequential\|none>` | ID generation: content-derived hash, sequential counter, or omit. |
| `--transform-origin <centroid\|baked>` | Transform origin placement — centroid offset or baked into coordinates. |
| `--arcs` | Emit circular-arc path commands where detected (off by default). |
| `--max-trace-dimension <n>` | Downscale inputs larger than this dimension before tracing. |
| `--max-pixels <n>` | Maximum pixel count accepted (decompression-bomb guard). |
| `--max-input-bytes <n>` | Maximum input file size in bytes. |
| `--timeout-ms <n>` | Abort conversion after this many milliseconds. |
| `--json <path>` | Write the metadata sidecar (nodes, bboxes, groups, stats) as JSON. |

A second subcommand, `spryteo inspect`, prints a human-readable summary of
an SVG produced by `convert` — node/path counts, group tree, and (with
`--meta`) centroids, bboxes and colours read back from the JSON sidecar.

```
$ spryteo inspect ./icon.svg --meta ./icon.json
  4 nodes · 2 groups · viewBox 0 0 64 64
  g-s-0
    #s-a1b2c3d4  circle  fill #e63946  area 812.4  bbox [12.0,12.0 52.0,52.0]
```

## Modes

`auto` (the default) picks one of these for you; every heuristic is
overridable with `--mode`.

| Mode | Description |
|---|---|
| `auto` | Picks a profile from the image itself — dimensions, unique-colour count, edge hardness, ink ratio. |
| `icon` | Few unique colours (≤32 after a 1% noise floor) or a flat-fill alpha channel. Aggressive primitive recognition — circles and rects come out as `<circle>`/`<rect>`, not path soup. |
| `pixel-art` | ≤128px, ≤64 unique colours, hard-edged (no anti-aliasing). Traces exact pixel boundaries with corner-preserving fit instead of smoothing them away. |
| `line-art` | Ink drawings binarize into exactly two layers — paper and ink — using a global Otsu threshold for solid strokes plus a capped Sauvola local threshold that rescues faint thin lines. A duotone validation gate falls back to full colour quantization when the image isn't genuinely two-tone. Pairs with `--stroke` for centerline output. |
| `photo` | Everything else. K-means colour quantization, optional gradient detection, bilateral filtering + JPEG deblocking, and automatic downscaling above 1600px so a 4K photo doesn't blow the time budget. |

## Choosing a mode

Given an image in front of you, use this guide to select the right `--mode`:

| Mode | Input assumptions | When to choose | Wrong mode symptom |
|---|---|---|---|
| `auto` | General input. Computes dimensions, unique colour count (1% noise floor), edge hardness, and ink ratio to select a profile automatically. | Default for initial runs or automated batch pipelines where image types vary. | Faint paper grain or scanner tints on ink art can trigger photo or icon mode instead of line-art; anti-aliased icon borders may classify as photo. |
| `icon` | Flat fills, few unique colours (≤32 after 1% noise floor), or flat fills over an alpha channel. Runs primitive recognition for `<circle>`, `<ellipse>`, and `<rect>`. | UI icons, vector logos, geometric badges, flat illustrations, and pictograms. | Running on photos produces severe posterization and color banding. If photo mode is mistakenly used on an icon, outlines become fuzzy, node counts bloat, and primitive recognition is skipped. |
| `pixel-art` | Low resolution (≤128px), ≤64 unique colours, sharp transitions with zero anti-aliasing. | Retro game sprites, pixel icons, 8-bit / 16-bit graphics, and favicon pixel grids. | Running on smooth icons or photos produces blocky, jagged pixel staircases. If auto or icon mode is run on pixel art, curve smoothing rounds sharp corners into distorted, pill-shaped blobs. |
| `line-art` | Two-tone ink drawing on paper (ink ratio < 25%). Binarizes with Otsu + Sauvola thresholding. Pairs with `--stroke` for centerline tracing. | Scanned sketches, architectural diagrams, technical schematics, wireframes, and handwritten signatures. | Running on multi-colour images triggers duotone validation fallback to k-means or crushes subtle hues into high contrast. Tracing line art with icon mode creates doubled hollow outlines instead of single stroked centerlines. |
| `photo` | Continuous-tone imagery, photographic palettes, subtle gradients, and complex real-world textures. | Photographs, paintings, renders with lighting gradients, and complex textured art. | Running on icons creates stacked layers with edge slivers and no primitive shapes. Running icon mode on a photo creates harsh color banding across gradual transitions. |

## How it works

Every surface runs the same eleven-stage pipeline over the same Rust engine
— no surface has its own shortcuts or its own bugs.

| Stage | What happens |
|---|---|
| `decode` | PNG/JPEG/GIF/WebP/BMP → raw RGBA. EXIF orientation applied, embedded ICC profiles converted to sRGB via a pure-Rust colour stack (no LGPL dependency). |
| `classify` | Picks auto's mode: dimensions, unique-colour count (with a 1% noise floor), edge hardness, and ink ratio decide icon vs. pixel-art vs. line-art vs. photo. |
| `downscale` | Inputs above `--max-trace-dimension` (1600px default trigger, 1024px working size) are resampled with Lanczos3 before tracing; output coordinates still land on the original viewBox. |
| `preprocess` | Mode-specific cleanup: bilateral filtering and JPEG-deblocking for photos, Otsu + capped Sauvola local thresholding with a Lab-space duotone validation gate for line-art. |
| `quantize` | K-means colour reduction to the target palette (seeded kmeans++, deterministic) or the line-art binarizer's two-layer paper/ink split. |
| `apply_background_policy` | Decides whether the base layer is kept, dropped, or flattened into a `<rect>` per `--background`. |
| `extract_contours` | Suzuki-Abe border following over a marching-squares field builds a parent/child hole hierarchy per connected region — a letter "O" becomes one path with a true hole subpath, not two overlapping fills. |
| `fit_contours` | Simplifies each contour, detects corners, then fits Bezier segments — with a chord-straightness pre-check that emits a plain LineTo (and merges consecutive collinear runs) instead of curve-fitting edges that are already straight. |
| `build_fills` | Resolves fill colours/gradients per shape and promotes near-circular or near-rectangular contours to real `<circle>`/`<ellipse>`/`<rect>` primitives instead of path soup. |
| `build_scene_graph` | Groups shapes into a `<g>` tree (containment-order by default, or mask-guided) and assigns stable content-derived or sequential IDs. |
| `emit_svg` | Serializes the scene graph — minified or `--pretty`, JSX component or plain markup — plus the JSON metadata sidecar if requested. |

A few of the pipeline's sharper edges, worth knowing if you're pushing
tricky input through it:

### Line-art binarization

Ink drawings first get a global Otsu threshold for solid strokes, then a
capped Sauvola local threshold (computed over integral images, so it's
`O(n)` not `O(n·window²)`) to rescue faint thin lines the global threshold
would drop. Before committing to two layers, a duotone validation gate
projects every pixel onto the ink↔paper axis in Lab space — if more than
2% of pixels fall too far off that axis, the image isn't actually two-tone
and the pipeline falls back to full k-means colour quantization instead of
flattening a colour photo into a black-and-white mess.

### Straight-run recovery

Contour fitting doesn't force every edge through a cubic Bezier. Before
fitting a run between two detected corners, a chord-straightness check
(0.5px tolerance) tests whether every point on the run already lies within
tolerance of the straight line between its endpoints — if so, it emits a
plain `LineTo` instead of a curve, and a merge pass afterward collapses
consecutive collinear `LineTo`s into one. A traced square stays four
straight edges at any smoothness setting, not four near-straight cubics
with rounding error baked in.

### True holes, correct paint order

Contour extraction (Suzuki-Abe border following over a marching-squares
field) builds a real parent/child hole hierarchy per region. A letter "O"
or a donut shape becomes one path with an inner subpath and
`fill-rule="evenodd"` — not two overlapping opaque fills where the hole is
really just a same-colour circle painted on top. Containment nesting also
drives paint order in the scene graph, so a shape enclosed by another
paints after its container rather than being silently occluded.

## Determinism & testing

The same input and options produce byte-identical SVG on every run, on
every machine. That's an engineering constraint, not a happy accident: no
`HashMap` iteration reaches an output path anywhere in the pipeline, every
sort is stable, and colour quantization's k-means++ seeding is fixed
rather than time- or thread-seeded. You can verify it yourself — run the
same conversion twice and diff the bytes.

Correctness is gated by a golden-corpus test harness
(`spryteo-cli/tests/corpus.rs`) that renders every fixture's output back
to a raster and compares it against the source with SSIM, alongside
byte-budget and node-budget ceilings per fixture. Per-category SSIM gates:
icons and pixel-art 0.92, line-art 0.90, photos 0.85, real-world mixed
images 0.80. Goldens are regenerated with
`UPDATE_GOLDENS=1 cargo test -p spryteo-cli --release --test corpus` and
every regeneration is diffed by hand before it's trusted — an aggregate
similarity score passing is necessary, never sufficient, so nothing gets
accepted on the metric alone without a human looking at the rendered
output.

## Library (Node)

A native addon built with napi-rs — the real Rust engine in-process, no
subprocess or WASM overhead. `convert()` is async; `convertSync()` blocks.
Options are a JSON string merged against sensible defaults, so you only
need to specify what you're overriding.

```js
import { convert } from 'spryteo'

const { svg, meta } = await convert(
  buffer,
  JSON.stringify({ mode: 'icon', colors: 8, css: 'draw' })
)

// meta.nodes[i] = { id, bbox, centroid, area, fill, group, zOrder }
console.log(meta.stats) // { nodeCount, pathCount, byteCount }
```

## Browser (WASM)

The same engine compiled to WebAssembly — this is what powers the live
demo on the homepage. Everything happens client-side; no image ever
leaves the browser.

The WebAssembly build is not on npm yet. Build it from the repository with
`wasm-pack build bindings/wasm --target web --release`, then import the
generated `bindings/wasm/pkg` package:

```js
import init, { convert_default } from 'spryteo-wasm'

await init('/spryteo_wasm_bg.wasm')
const bytes = new Uint8Array(await file.arrayBuffer())
const { svg, meta } = convert_default(bytes)
```

Use `convert(bytes, optionsJson)` instead of `convert_default` to pass the
same JSON options object as the Node and CLI surfaces.

## MCP server

Register Spryteo as an MCP server so AI agents can convert raster images and inspect vector output mid-conversation.

### 1. Install

```bash
cargo install spryteo-mcp
```

### 2. Configure

Add the server to your client configuration:

```json
{
  "mcpServers": {
    "spryteo": {
      "command": "spryteo-mcp"
    }
  }
}
```

Config file locations:

- **Claude Desktop**:
  - macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
  - Windows: `%APPDATA%\Claude\claude_desktop_config.json`
  - Linux: `~/.config/Claude/claude_desktop_config.json`
- **Claude Code**:
  - Global: `~/.claude.json`
  - Project: `.claude.json`
  - One-line alternative (no manual JSON editing):
    ```bash
    claude mcp add spryteo -- spryteo-mcp
    ```
- **Cursor**:
  - Global: `~/.cursor/mcp.json`
  - Project: `.cursor/mcp.json`
- **VS Code**:
  - Workspace: `.vscode/mcp.json` (or user configuration via MCP extension settings)

### 3. Verify

Check that the server registered:

- **Claude Desktop**: Open a conversation and click the hammer icon; verify `convert_image` and `inspect_svg` appear under `spryteo`.
- **Claude Code**: Run `claude mcp list`; verify `spryteo` is listed with tools enabled.
- **Cursor / VS Code**: Open the agent panel; verify `convert_image` and `inspect_svg` appear in the active tool list.

### 4. Troubleshoot

- **Command not found (`spryteo-mcp`)**: Cargo's bin directory is not on your `PATH`. It is `~/.cargo/bin` by default; export it in your shell profile (`export PATH="$HOME/.cargo/bin:$PATH"`), or set `"command"` in your configuration file to the absolute path of `spryteo-mcp`.
- **Server registers but tools do not appear**: The client loads configuration only on startup. Completely quit and restart your editor or client application.
- **No Rust toolchain (`cargo: command not found`)**: Install Rust from https://rustup.rs, open a new shell so `~/.cargo/bin` is on `PATH`, then re-run `cargo install spryteo-mcp`.

### Available tools

| Tool | Arguments | Returns |
|---|---|---|
| `convert_image` | `image_base64` (string, required): Base64-encoded raw image bytes (PNG, JPEG, GIF, WebP, BMP), with or without `data:...;base64,` prefix.<br>`options` (object, optional): Partial `ConvertOptions` JSON object (e.g. `{"mode": "icon", "colors": 8, "stroke": true}`). Omitted fields fall back to `ConvertOptions::default()`. | Serialized `ConvertResult` JSON string containing `svg` (rendered SVG markup) and `meta` (metadata sidecar object). |
| `inspect_svg` | `meta_json` (string, required): The serialized `meta` JSON sidecar string returned by `convert_image` (not the raw SVG markup). | Human-readable Markdown inspection summary showing schema version, total node count, path count, byte count, and a table of individual nodes (ID, group, shape, paint, bounding box, centroid, area, outline length, suggested draw order). |

## HTTP API

**Planned, not yet shipped.** `spryteo-api` exists in the workspace as a
stub — until it lands, use the CLI, library, WASM build, or MCP server
above.

## Options reference

Every surface (CLI flags, library/WASM/MCP JSON options) shares the same
underlying option set.

### Colour & layering

`colors` targets a palette size for quantization — a ceiling, not a
guarantee, since visually indistinguishable clusters are merged after
quantization. `layering` (photo mode)
chooses between `stacked` layers or `cutout` composition. `gradients`
(`auto | on | off`) detects smooth colour gradients and emits an SVG
`<linearGradient>`/`<radialGradient>` fill instead of a flat colour where
the residual fits within a Lab-space tolerance — auto only attempts it in
photo mode, since icons are almost always flat fills.

### Geometry

`tolerance` is the curve-fit error budget in pixels — lower produces more
path points and tighter fidelity. `smoothness` controls corner-preservation
strength during Bezier fitting. `turdsize` discards regions below a
minimum area (denoising). `arcs` allows emitting native SVG arc commands
for near-circular/near-rectangular contours instead of Bezier
approximations — this is what makes a traced circle come out as a real
`<circle>` element.

### Stroke / centerline mode

Setting `stroke: true` (or `--stroke`) bypasses colour quantization
entirely and runs a dedicated centerline pipeline: binarize → skeletonize
→ build a stroke graph → prune spurious branches → traverse → smooth.
Traversal looks for a true Eulerian path through the stroke graph via
Hierholzer's algorithm — a graph with exactly two odd-degree nodes gets a
virtual edge added between them to close it into a circuit first; graphs
with more than two odd-degree junctions (genuine branching strokes) fall
back to a greedy walk that emits separate paths per branch rather than
one continuous stroke. Each path's width is the per-stroke median
(variable-width splitting along a single stroke is not yet implemented).
Output paths get `pathLength="100"` normalized for animation and are
emitted with `fill="none"`.

### Semantic grouping

Shapes are grouped into a `<g>` tree two ways: containment-based (default
— a shape nested inside another's bounding region becomes its child
group under the smallest-area shape that fully contains it) and
mask-guided, where you supply your own segmentation masks (e.g. from a
model you're already running) and Spryteo groups traced contours by
mask-overlap coverage. Spryteo does not bundle or run an ONNX/SAM model
itself — mask-guided grouping is bring-your-own-masks by design:
`spryteo-semantic` implements the grouping and assignment algorithm only,
not segmentation inference.

### Stable IDs

Every shape gets a content-derived ID (`s-<8 hex chars>`, a blake3 hash of
the shape's geometry and fill) via `id_style`. Re-running on a slightly
edited source keeps unchanged shapes' IDs identical — safe to reference a
shape by ID across re-exports.

### CSS presets

Three built-in animation presets bakeable via `css` / `--css`: `draw`
(stroke-dashoffset reveal), `fade` (staggered opacity), `pop` (staggered
scale-in). Or ignore them and animate the grouped output yourself —
that's the point of shipping real groups instead of one flattened path.

### Decode-time correctness

EXIF orientation is read and applied automatically — a photo shot
sideways decodes upright, not rotated. Embedded ICC colour profiles are
converted to sRGB before any other processing (via a pure-Rust
colour-management stack, no LGPL dependency). Photos over 1600px on the
long edge are automatically downscaled to a 1024px working size with
Lanczos3 resampling before tracing, so a 4K upload doesn't blow the time
budget — coordinates are still emitted against the correct output
viewBox.

### Limits

Decompression-bomb guards reject oversized input before allocating: max
input bytes (8MB default), max pixel count (16MP default), and a hard
8192px dimension ceiling — all configurable, all enforced before the full
decode where possible.

## Recipes

Complete, runnable examples for common vectorization tasks:

- **Vectorize a flat corporate logo into a clean 4-colour SVG:**
  ```bash
  spryteo convert logo.png -o logo.svg --mode icon --colors 4
  ```

- **Vectorize an icon with centroid transform origins and bake in a CSS pop animation:**
  ```bash
  spryteo convert icon.png -o icon.svg --mode icon --css pop --transform-origin centroid
  ```

- **Trace a retro pixel-art sprite preserving exact pixel corners without smoothing:**
  ```bash
  spryteo convert player.png -o player.svg --mode pixel-art
  ```

- **Trace an ink drawing or signature into single continuous stroked paths:**
  ```bash
  spryteo convert signature.png -o signature.svg --mode line-art --stroke
  ```

- **Vectorize a photograph into 16 stacked colour layers with gradient detection:**
  ```bash
  spryteo convert photo.jpg -o photo.svg --mode photo --colors 16 --layering stacked --gradients auto
  ```

- **Emit a reusable React JSX component with camelCase attributes:**
  ```bash
  spryteo convert badge.png -o Badge.jsx --mode icon --jsx
  ```

- **Split a raster icon contact sheet into individual SVGs on a 24px canonical grid:**
  ```bash
  spryteo sheet sheet.png -o ./icons/ --canonical-size 24 --name-prefix icon
  ```

- **Vectorize a monochrome glyph inheriting parent CSS color via currentColor:**
  ```bash
  spryteo convert glyph.png -o glyph.svg --mode icon --current-color
  ```

- **Export an SVG with a JSON metadata sidecar for programmatic inspection:**
  ```bash
  spryteo convert mark.png -o mark.svg --mode icon --json mark.json
  ```

## Output & metadata

Every surface returns the same shape: an `svg` string and a `meta`
sidecar built for programmatic and agent consumption, not just human
eyeballing. `meta.nodes` is a flat list — one entry per shape — each with
a stable `id`, `bbox`, `centroid`, `area`, resolved `fill`, its `group`
key into the SVG's `<g id=...>` tree, `zOrder`, and a
`suggestedDrawOrder` for animation sequencing. `meta.stats` gives
node/path/byte counts for a quick sanity check without walking the tree.

## Output anatomy

Spryteo produces structured vector scenes designed for CSS styling, animation, and programmatic inspection.

### Group tree

The emitted SVG organizes paths into a semantic `<g>` hierarchy:

- **Component grouping (`--grouping component`, default)**: Connected visual regions become groups (`<g id="g-s-...">`). Shapes nested entirely inside another shape's bounding contour (containment order) become child `<g>` elements under the enclosing shape.
- **Semantic mask grouping (`--grouping semantic`)**: External segmentation masks group shapes under labeled containers (`<g id="g-mask-<id>">`).
- **Flat structure (`--grouping flat`)**: Groups are omitted and all shapes render directly under `<svg>`.
- **Paint order**: Sibling groups and nodes follow document order (z-order), so background containers render first and nested details render on top.

### ID scheme

Every shape element and group receives a deterministic identifier controlled by `--id-style`:

- **Hash (`--id-style hash`, default)**: IDs format as `s-<8 hex chars>`, computed using `blake3` over the shape's canonicalized coordinates (rounded to 3 decimals, `-0.0` normalized to `0.0`), primitive parameters, and fill color. Identical duplicate shapes append `-2`, `-3` in document order. Unaltered shapes maintain identical IDs across re-runs.
- **Sequential (`--id-style sequential`)**: Shapes are numbered monotonically in paint order (`s-0`, `s-1`, `s-2`).
- **None (`--id-style none`)**: Omits `id` attributes entirely from elements and groups.
- Group IDs prefix the opening node ID (`g-s-a1b2c3d4` or `g-s-0`) or the semantic mask ID (`g-mask-<id>`).

### Metadata sidecar fields

When written via `--json <path>` or returned by the Node library, WASM build, or MCP server, the `meta` object contains:

| Field | Meaning |
|---|---|
| `schema_version` | Metadata schema version (currently `1`). Pre-versioning sidecars deserialize as `0`. |
| `stats.node_count` | Total number of shape nodes in the scene. |
| `stats.path_count` | Total number of vector paths emitted. |
| `stats.byte_count` | Byte size of the serialized SVG markup. |
| `current_color_applied` | Boolean indicating whether monochrome fill was mapped to `fill="currentColor"`. |
| `groups` | Mirrors the emitted `<g>` element hierarchy with `id`, `nodes` (IDs), and nested `groups`. |
| `nodes[].id` | Content-derived stable identifier (e.g. `s-a1b2c3d4`). |
| `nodes[].bbox` | Exact axis-aligned bounding box `[x_min, y_min, x_max, y_max]`. |
| `nodes[].centroid` | Center of mass `(cx, cy)` computed by Green's theorem curve integrals. |
| `nodes[].area` | Surface area in square user units. |
| `nodes[].fill` | Representative sRGB colour `{ r, g, b }` for quick identification. |
| `nodes[].paint` | Lossless paint definition: solid colour, `currentColor` with fallback, or linear/radial gradient with stops and coordinate vectors. |
| `nodes[].stroke` | Lossless stroke paint and width for centerline paths (`null` for filled shapes). |
| `nodes[].shape` | Emitted element kind: `path`, `circle`, `ellipse`, `rect`, or `arc`. |
| `nodes[].closed` | Whether the outline closes back on itself (`false` for open centerline strokes). |
| `nodes[].path_length` | Total outline length in user units (normalizes with `pathLength="100"` in stroke mode). |
| `nodes[].group` | ID of the immediate parent `<g>` element. |
| `nodes[].group_path` | Ancestor group IDs from root down to immediate parent. |
| `nodes[].z_order` | Paint order index (0 = bottom layer, painted first). |
| `nodes[].suggested_draw_order` | Reveal sequence index for entrance animations (ranks nesting depth first, descending area second). |

## Troubleshooting

Common symptoms, root causes, and CLI fixes:

| Symptom | Cause | Fix |
|---|---|---|
| Output file size too large / too many nodes | Curve-fit tolerance is too strict, noise suppression area is too low, or colour count is too high. | Increase `--tolerance` (e.g. `--tolerance 1.0`), increase `--turdsize` (e.g. `--turdsize 8`) to drop speckle noise, or reduce `--colors`. |
| Fine details or small shapes lost as noise | The `--turdsize` area threshold exceeds the pixel area of small dots or punctuation, or downscaling discarded them. | Decrease `--turdsize` (e.g. `--turdsize 1` or `--turdsize 0`), or increase `--max-trace-dimension` to prevent downscaling. |
| Logo or line drawing traced as hollow outlines instead of filled shapes or strokes | Image was traced in default fill mode instead of centerline tracing. | Pass `--stroke` (typically with `--mode line-art`) to run the skeletonizing centerline tracer instead of contour outlining. |
| Colours merged together that should remain distinct | `--colors` target is too low, or k-means clustering in Lab space merged visually close hues. | Increase `--colors` (e.g. `--colors 16`), or specify exact brand colours with `--palette '#hex1,#hex2,#hex3'`. |
| Slow conversion on a large photograph | High-resolution images require extensive contour extraction across multiple quantized layers. | Lower working dimension with `--max-trace-dimension 1024` (or `800`), use `--layering cutout`, or lower `--colors`. |

## Crate architecture

The engine is a Rust workspace of small, single-purpose crates — every
surface (CLI, Node addon, WASM, MCP) links the same ones, so there's
exactly one place each algorithm lives.

| Crate | Purpose |
|---|---|
| `spryteo-core` | Shared IR types, ConvertOptions, pipeline orchestration, errors |
| `spryteo-raster` | Decode (PNG/JPEG/GIF/WebP/BMP), EXIF orientation, ICC→sRGB, downscale, preprocess |
| `spryteo-quant` | Input classification, colour quantization, line-art binarization, background detection |
| `spryteo-trace` | Contour extraction (Suzuki-Abe, marching squares), hole hierarchy |
| `spryteo-fit` | Polygon simplification, corner detection, straight-run recovery, Bezier curve fitting |
| `spryteo-geom` | Primitive recognition (circle, ellipse, rect, arc), geometry hashing |
| `spryteo-stroke` | Centerline tracing: binarization, skeletonization, Eulerian path traversal |
| `spryteo-semantic` | Mask-overlap grouping heuristics for externally-supplied segmentation masks |
| `spryteo-svg` | Scene graph construction, SVG emission, metadata sidecar |
| `spryteo-cli` | CLI binary (clap), pipeline wiring, error reporting |
| `spryteo-mcp` | MCP server exposing convert_image and inspect_svg as agent tools |
| `spryteo-api` | HTTP API stub (axum), planned not yet shipped |

## Built with

Rust · WebAssembly · napi-rs / Node · TypeScript · MCP · blake3 ·
MIT / Apache-2.0

---

Full reference lives in the GitHub README: https://github.com/chidi09/spryteo
