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
