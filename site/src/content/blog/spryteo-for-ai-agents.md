---
title: Spryteo for AI agents
description: The Spryteo MCP server gives AI agents local image vectorization and structural inspection tools with zero network calls and full private execution.
pubDate: 2026-09-03
topic: agents
---

## Why agents need local vectorization

AI coding agents are frequently asked to build frontend components from visual assets. A developer working in a chat interface or an editor might provide a PNG icon, a raster logo, or an interface mockup, and ask the agent to create a styled, animated SVG component.

Without dedicated tools, an agent faces two poor options:

1. **Attempting to hand-code paths.** Large language models can write basic SVG shapes from scratch, but they cannot accurately trace an arbitrary raster image into complex Bezier curves purely through next-token prediction.
2. **Calling a hosted vectorizer API.** Routing an image through an external web service introduces API keys, per-image billing, network latency, and significant privacy risks when working with confidential brand assets or unreleased application designs.

Spryteo offers a third approach: a local Model Context Protocol (MCP) server that gives agents immediate access to our offline vectorization engine.

## The Spryteo MCP server

Model Context Protocol allows AI assistants -- including Claude Desktop, Claude Code, Cursor, and other MCP-compliant environments -- to invoke external tools mid-conversation.

Spryteo distributes an MCP server via npm under the package name `spryteo-mcp`.

To install it globally:

```bash
npm install -g spryteo-mcp
```

You can register it in your MCP configuration file:

```json
{
  "mcpServers": {
    "spryteo": { "command": "spryteo-mcp" }
  }
}
```

If you use Claude Code, you can register Spryteo with a single terminal command without manually editing a JSON configuration:

```bash
claude mcp add spryteo -- spryteo-mcp
```

Once registered, the server runs as a local background process communicating over standard input and output.

## The two tools

The `spryteo-mcp` server exposes two dedicated tools designed to fit an agent's reasoning loop:

### 1. `convert_image`

The `convert_image` tool takes base64-encoded image bytes and returns both the generated SVG markup and a structured metadata sidecar.

It accepts the same partial JSON configuration object supported by the CLI and library surfaces:

- `mode`: Override automatic image classification (`auto`, `icon`, `pixel-art`, `line-art`, or `photo`).
- `colors`: Specify target palette size for quantization.
- `stroke`: Run centerline tracing instead of outline tracing for line drawings.
- `css`: Bake built-in CSS animation presets (`draw`, `fade`, or `pop`).
- `grouping`: Choose between containment grouping (`component`), semantic grouping, or flat output.

The tool executes the full eleven-stage Rust pipeline locally on the machine hosting the agent. There are no network calls and no external dependencies.

### 2. `inspect_svg`

When an agent needs to understand the structure of an SVG it just generated, it passes the metadata JSON string from `convert_image` into `inspect_svg`.

Rather than forcing the model to parse raw XML or compute bounding boxes from cubic Bezier commands, `inspect_svg` returns a human-readable summary:
- Total node, path, and group counts.
- ViewBox dimensions.
- The hierarchical group tree.
- Per-node geometry, including shape types (`path`, `circle`, `rect`), resolved fill colors, bounding boxes, areas, and exact centroids.

This gives the agent the factual context it needs to answer questions, write accurate CSS rules, or design animations.

## Why local execution matters for private assets

When an AI agent assists with software development, it often handles sensitive data: proprietary application icons, unreleased logos, screenshots of internal dashboards, or confidential sketches.

Sending those assets to a third-party hosted vectorizer API exposes the user to data leakage risks and terms-of-service compliance issues. Many corporate environments explicitly prohibit transmitting user-supplied images across external network boundaries.

Spryteo solves this by running the entire conversion pipeline in-process on the local machine. Whether installed via the CLI, the Node library, or the MCP server, the Rust engine operates directly on the local CPU:
- No telemetry or image data is sent to external servers.
- No third-party accounts or API tokens are required.
- The tool functions in air-gapped environments and on offline development machines.

Because the tool runs locally, execution is fast and predictable. The agent is never blocked by remote rate limits, cold starts, or network timeouts.

## Why structured output enables agent reasoning

A traditional vectorizer outputs a flat collection of paths. If an agent receives an opaque string like `<path d="M14.2 18.5 C14.2 ... Z" />`, it cannot easily determine where specific visual elements reside. It cannot tell which path represents a button, where an icon's center of mass is, or how shapes relate spatially.

Spryteo's structured output changes how an agent interacts with vector graphics:

### 1. Addressable group hierarchies

Because Spryteo organizes shapes into a semantic `<g>` tree based on spatial containment, an agent can identify logical components. If the user asks the agent to "make the inner icon pulsate on hover," the agent can target the inner `<g id="g-s-1">` element rather than attempting to isolate individual path fragments.

### 2. Centroids and bounding boxes

To animate a shape using CSS transforms -- such as `transform: rotate(45deg)` -- the agent must specify a `transform-origin`. Without exact coordinates, CSS transforms rotate around the top-left corner of the entire canvas. Spryteo's metadata provides precomputed centroids for every node, allowing the agent to write exact `transform-origin: 24px 24px` rules immediately.

### 3. Stable IDs across re-runs

When an agent iterates on a design with a user -- for example, adjusting colors or trying different animation presets -- it may run `convert_image` multiple times. Because Spryteo generates stable, content-derived IDs (`s-<hash>`), unchanged elements maintain their identities. The CSS rules and JavaScript hooks the agent previously wrote do not break when an adjacent color is adjusted.

## Current status

The `spryteo-mcp` package and the underlying Rust crates are published at version 0.0.1 under dual MIT / Apache-2.0 licenses.

For complete tool schemas and usage details, refer to the [documentation](/docs) or read our machine-readable brief at [/llms.txt](/llms.txt). The repository is hosted on [GitHub](https://github.com/chidi09/spryteo).
