// Page copy that is rendered *and* described in structured data.
//
// These live here rather than inside the components so the FAQPage and HowTo
// JSON-LD are generated from the exact strings on the page. Structured data
// that disagrees with the visible content is a manual-action risk, and the
// only reliable way to keep them in step is to have one copy.

export interface QA {
  q: string;
  a: string;
}

export const faqs: QA[] = [
  {
    q: 'Is Spryteo free?',
    a: 'Yes. Spryteo is fully open source and dual-licensed under MIT / Apache-2.0 -- free for personal and commercial use, no account, no usage limits enforced by us.',
  },
  {
    q: 'Do my images get uploaded anywhere?',
    a: 'No. The CLI, Node library, and MCP server run entirely on your machine. The browser demo runs the same engine compiled to WebAssembly, client-side -- your image never leaves the tab.',
  },
  {
    q: 'What image formats does it accept?',
    a: 'PNG, JPEG, GIF (first frame), WebP, and BMP as input. Output is always SVG. Format is detected by magic bytes, never by file extension.',
  },
  {
    q: 'How is this different from other raster-to-SVG tools?',
    a: 'Most tracers give you a single flattened <path> per colour region, or one giant path for the whole image. Spryteo groups shapes into a semantic <g> tree and gives every shape a stable, content-hashed ID -- so the output is actually structured enough to select and animate individual parts, not just display as a static image.',
  },
  {
    q: 'Does it handle photos, or just icons?',
    a: 'Both. Icon mode targets flat-colour logos and UI icons with aggressive primitive recognition (circles, rects). Photo mode runs colour quantization, optional gradient detection, and automatically downscales very large photos before tracing so conversion stays fast.',
  },
  {
    q: 'Can I use it in CI or a build pipeline?',
    a: "Yes -- the CLI is a single static binary with no network calls, so it drops into any CI runner or build step. The Node library works the same way if you'd rather call it from JavaScript/TypeScript directly.",
  },
  {
    q: 'What is the MCP server for?',
    a: 'Model Context Protocol lets AI agents (like Claude) call tools mid-conversation. spryteo-mcp exposes convert_image and inspect_svg as MCP tools, so an agent can vectorize an icon or inspect an SVG structure without you leaving the chat.',
  },
  {
    q: 'How does an AI agent use Spryteo to extract an SVG?',
    a: 'Install the MCP server with `npm install -g spryteo-mcp` and register it with your agent. The agent then calls convert_image with a path or base64 image and gets back SVG markup plus a metadata sidecar describing the groups and node counts, all computed locally -- no image is sent to a third-party API.',
  },
  {
    q: 'What version is Spryteo, and is the API stable?',
    a: 'Everything is deliberately pre-1.0 and versioned together at 0.0.1. The Rust engine crates are published on crates.io (spryteo-core, spryteo-cli, spryteo-trace, spryteo-quant and the rest), and the npm distribution carries the same version. Breaking changes are still possible; SEMVER.md in the repository records the exact compatibility commitments on ConvertOptions and the metadata sidecar.',
  },
  {
    q: 'How does Spryteo handle image transparency and alpha channels?',
    a: 'By default, Spryteo preserves transparency as a soft mask during quantization with `--alpha-mode keep`. You can also composite transparent regions over a solid background with `--alpha-mode matte:#rrggbb`, or apply a hard binary threshold with `--alpha-mode threshold:128`. Transparent pixels below the threshold are dropped from tracing entirely.',
  },
  {
    q: 'Can I use the generated SVGs in commercial projects?',
    a: 'Yes. Spryteo is dual-licensed under MIT and Apache-2.0, placing no restrictions on how you use, distribute, or sell the generated SVG files. The output belongs to you, and no attribution or license notices are injected into the SVG markup.',
  },
  {
    q: 'How do I animate individual elements in the output SVG?',
    a: "Every shape receives a stable content-hashed ID such as `#s-a1b2c3d4`, and groups mirror the visual hierarchy with IDs like `#g-s-a1b2c3d4`. You can target these IDs directly with CSS keyframes, transitions, or JavaScript animation libraries without manual path splitting. Setting `--transform-origin centroid` places local coordinates at each shape's center of mass so rotations and scales behave predictably.",
  },
  {
    q: 'What information does the metadata sidecar provide?',
    a: 'The JSON metadata sidecar records precise geometry for every shape, including bounding boxes, centroids, surface areas, paint definitions, and outline lengths. It also provides a suggested draw order for sequencing entrance animations and mirrors the full `<g>` group hierarchy. This lets scripts and AI agents inspect or manipulate the vector scene without re-parsing raw SVG path strings.',
  },
  {
    q: 'What are the input dimension and file size limits?',
    a: 'Default security limits reject input files larger than 8MB, images with more than 16 megapixels, or dimensions exceeding 8192px before allocating memory. In photo mode, images wider than 1600px are downscaled to a 1024px working size using Lanczos3 resampling while preserving the original coordinate space. All limits can be customized via `--max-input-bytes`, `--max-pixels`, and `--max-trace-dimension`.',
  },
  {
    q: 'How does Spryteo output compare to manual hand-tracing?',
    a: 'Manual vectorization by a designer yields minimal Bezier curves with intentional semantic groupings that automated algorithms cannot fully match. Spryteo approximates this by promoting detected circles and rectangles to native SVG primitives, removing collinear straight runs, and grouping nested shapes by geometric containment. For line drawings, centerline mode extracts single stroked paths rather than doubled outline loops.',
  },
  {
    q: 'What does determinism mean for my build pipeline?',
    a: 'Given identical input bytes and command-line options, Spryteo generates byte-identical SVG markup on every run and across all supported operating systems. The engine uses fixed-seed k-means clustering, stable sorting algorithms, and deterministic coordinate rounding to eliminate floating-point drift. This guarantees reproducible builds and prevents noisy, spurious git diffs in your repositories.',
  },
  {
    q: 'Can I lock the output to a specific brand color palette?',
    a: "Pass `--palette` with a comma-separated list of hex colors, such as `--palette '#112233,#445566,#ffffff'`, or repeat the flag for each color. When a palette is specified, quantization clustering is bypassed and every pixel is mapped to the nearest provided color in CIELAB color space. This overrides the `--colors` count ceiling.",
  },
  {
    q: 'How do I report a bad conversion or visual artifact?',
    a: 'Open an issue on GitHub at https://github.com/chidi09/spryteo with the source image, the exact command or options used, and the resulting SVG. If the image cannot be shared publicly, specify the input format, dimensions, color count, and a description of the failure mode. Reproducible test cases help expand our golden test corpus.',
  },
  {
    q: 'How do I pin a specific version of Spryteo?',
    a: 'For global CLI or MCP usage, specify the exact version during installation with `npm install -g spryteo@0.0.1` or `npm install -g spryteo-mcp@0.0.1`. In Node projects, pin `"spryteo": "0.0.1"` without caret or tilde prefixes in your package.json. In Rust workspaces, specify `spryteo-core = "=0.0.1"` in your Cargo.toml dependencies.',
  },
];

export interface Step {
  n: string;
  title: string;
  desc: string;
}

export const pipelineSteps: Step[] = [
  { n: '01', title: 'Ingest', desc: 'Decode the raster at native resolution.' },
  { n: '02', title: 'Trace', desc: 'Fit edges to continuous Bézier paths.' },
  { n: '03', title: 'Group', desc: 'Cluster paths into semantic groups.' },
  { n: '04', title: 'Optimize', desc: 'Merge, simplify, drop redundant nodes.' },
  { n: '05', title: 'Emit', desc: 'Write clean, animateable SVG.' },
];
