<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/spryteo-logo-dark.svg">
    <img src="assets/spryteo-logo-light.svg" width="140" alt="Spryteo logo">
  </picture>
</p>

<h1 align="center">Spryteo</h1>

<p align="center">Raster images in, clean animateable SVG out — entirely offline.</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue" alt="license">
  <img src="https://img.shields.io/badge/language-Rust-red" alt="language">
</p>

Spryteo decodes PNG, JPEG, GIF, WebP and BMP, classifies every image into
icon / pixel-art / line-art / photo, quantizes colours, extracts and fits
contours to Bezier curves and geometry primitives, and emits deterministic,
semantically grouped SVG. Every stage runs locally — no cloud dependency, no
network round-trip, no upload.

### Highlights

- **Deterministic output:** the same input and options produce byte-identical
  SVG on every run.
- **Shape-aware:** circles, ellipses and rounded rects are emitted as real
  `<circle>`/`<ellipse>`/`<rect>` primitives, not path soup.
- **True hole subpaths with `fill-rule="evenodd"`** — a letter "O" is one
  path, so nothing underneath gets painted over.
- **Line-art mode** binarizes with a global-Otsu + local-Sauvola hybrid and
  recovers straight edges as L commands: a 480px ink drawing converts to 18
  nodes / ~13 KB at SSIM 0.99.
- **Four surfaces, one engine:** CLI, Node native addon, browser WASM, MCP
  server.
- **Golden-corpus CI:** every fixture is gated on render-back SSIM, byte
  budget and node budget.

## Install

Requires Node 18 or newer for the npm-distributed CLI/library/WASM builds.

```
$ npm install -g spryteo
# or, as a project dependency
$ npm install spryteo
```

## Quick start

### CLI

```
$ spryteo convert ./icon.png -o ./icon.svg --mode icon --colors 8
  traced 1 path · 3 groups · 2.1 kb → ./icon.svg
```

```
$ spryteo inspect ./icon.svg --meta ./icon.json
  4 nodes · 2 groups · viewBox 0 0 64 64
  g-s-0
    #s-a1b2c3d4  circle  fill #e63946  area 812.4  bbox [12.0,12.0 52.0,52.0]
```

### Node

```js
import { convert } from 'spryteo'

const { svg, meta } = await convert(
  buffer,
  JSON.stringify({ mode: 'icon', colors: 8 })
)

console.log(meta.stats) // { nodeCount, pathCount, byteCount }
```

### Browser / WASM

```js
import init, { convert_default } from '@spryteo/wasm'

await init('/spryteo_wasm_bg.wasm')
const bytes = new Uint8Array(await file.arrayBuffer())
const { svg, meta } = convert_default(bytes)
```

Use `convert(bytes, optionsJson)` for non-default options like every other
surface.

### MCP server

```json
{
  "mcpServers": {
    "spryteo": { "command": "spryteo-mcp" }
  }
}
```

Agents call `convert_image` (base64 image → SVG + metadata) and
`inspect_svg` (metadata JSON → human-readable summary) mid-conversation.

## CLI reference

```
spryteo convert <input> -o <output> [options]
spryteo inspect <svg> [--meta <path>]
```

| Flag | Description |
|---|---|
| `-o, --output <path>` | Output SVG path. Required. |
| `--mode <auto\|icon\|pixel-art\|line-art\|photo>` | Override input classification. Default: auto. |
| `--stroke` | Run the centerline tracer instead of fill-mode outlining. |
| `--css <draw\|fade\|pop>` | Bake one of the built-in CSS animation presets into the SVG. |
| `--colors <n>` | Target palette size for quantization. |
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

The second subcommand, `spryteo inspect`, prints a human-readable summary
of an SVG produced by `convert` — node/path counts, group tree, and (with
`--meta`) centroids, bboxes and colours read back from the JSON sidecar.

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

## Architecture

| Crate | Purpose |
|---|---|
| `spryteo-core` | Shared IR types, ConvertOptions, pipeline orchestration, errors |
| `spryteo-raster` | Decode (PNG/JPEG/GIF/WebP/BMP), EXIF orientation, ICC→sRGB, downscale, preprocess |
| `spryteo-quant` | Input classification, colour quantization, layering, background detection |
| `spryteo-trace` | Contour extraction (Suzuki-Abe, marching squares), hole hierarchy |
| `spryteo-fit` | Polygon simplification, corner detection, Bezier curve fitting |
| `spryteo-geom` | Primitive recognition (circle, ellipse, rect, arc), geometry hashing |
| `spryteo-stroke` | Centerline tracing: binarization, skeletonization, Eulerian path traversal |
| `spryteo-semantic` | ML-guided grouping via ONNX (MobileSAM), feature-gated |
| `spryteo-svg` | Scene graph construction, SVG emission, metadata sidecar |
| `spryteo-cli` | CLI binary (clap), pipeline wiring, error reporting |
| `spryteo-mcp` | MCP server exposing convert_image and inspect_svg as agent tools |
| `spryteo-api` | HTTP API stub (axum), planned not yet shipped |

## Development

```
cargo build --workspace
cargo test --workspace --release
UPDATE_GOLDENS=1 cargo test -p spryteo-cli --release --test corpus
```

The golden corpus lives in `testdata/corpus/` with fixture categories for
icons, pixel-art, line-art, photos and real-world images. Every fixture is
gated on render-back SSIM, byte budget and node budget. Goldens are
regenerated with `UPDATE_GOLDENS=1`.

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the complete stage-by-stage specification
and phased delivery plan.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option.
