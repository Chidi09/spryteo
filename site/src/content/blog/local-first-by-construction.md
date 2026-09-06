---
title: Local-first by construction
description: Spryteo enforces privacy by architecture rather than policy, executing raster-to-SVG conversion entirely offline inside your local process or browser.
pubDate: 2026-09-02
topic: privacy
---

## Architectural privacy versus policy promises

When you use a hosted vectorization service, privacy is treated as a matter of policy.

A service provider displays a privacy policy on its landing page. It promises that your uploaded images are encrypted in transit, that temporary files are deleted from storage buckets after twenty-four hours, and that employee access is restricted. You are asked to trust that those promises are configured correctly, that server logs do not retain raw payloads, and that cloud policies will not change over time.

For developers handling proprietary assets -- unreleased brand identities, enterprise UI screenshots, patent diagrams, or personal photos -- trusting a policy is often not good enough. The moment an image leaves your computer, you have lost physical control over the data.

Spryteo approaches privacy differently: privacy by construction.

Nothing is uploaded to an external server because Spryteo has no conversion server to upload to. The conversion engine is a modular Rust library that compiles directly into the tools you run on your own machine. Offline execution is not a setting you toggle; it is an architectural invariant of the system.

## Four offline surfaces

Spryteo provides four distinct surfaces. Every surface executes the exact same eleven-stage conversion pipeline locally, sharing the same core algorithms with zero network overhead:

### 1. The command-line interface

Distributed as a standalone executable via the `spryteo` npm package (or compiled directly from the `spryteo-cli` crate), the CLI runs directly on your machine. You pass an input file path and an output destination:

```bash
spryteo convert ./icon.png -o ./icon.svg --mode icon
```

The process opens the file on your disk, performs decoding, quantization, contour extraction, and curve fitting entirely in RAM, and writes the resulting SVG to disk. It opens no network sockets and makes no external API requests.

### 2. The native Node library

For JavaScript and TypeScript projects, `import { convert } from 'spryteo'` executes the engine in-process using native Node addons built with napi-rs. Conversion runs at native CPU speeds without spawning child processes. If you run Spryteo on a platform without a prebuilt native binary, the package automatically falls back to an embedded WebAssembly build, maintaining offline operation everywhere.

### 3. Client-side WebAssembly in the browser

The live interactive demo on the Spryteo website runs the complete engine compiled to WebAssembly via wasm-bindgen (`@spryteo/wasm`).

When you drag an image onto the browser demo, the file is read into a browser `Uint8Array` in memory. The WebAssembly module processes the image bytes entirely within your local browser tab and renders the resulting SVG directly to the DOM. Your image is never transmitted across the network. If you disconnect your computer from the internet, the demo continues to work without interruption.

### 4. The local MCP server

The `spryteo-mcp` binary runs as a local process communicating with your AI assistant over standard input and output. When an agent invokes `convert_image`, image data remains on the local filesystem and CPU.

## Hosted vectorizer APIs versus local execution

Contrasting local-first vectorization with hosted cloud APIs highlights four practical advantages:

### 1. Confidentiality

When working within an enterprise codebase or handling client assets under non-disclosure agreements, uploading data to third-party endpoints creates compliance risks. With a local-first engine, sensitive graphics never leave your local environment.

### 2. Reproducibility

Hosted APIs update on their own schedules. A remote service may alter its background segmentation model, adjust curve-fitting thresholds, or change its default quantization settings without notice. An asset converted today may look different when converted through the same API next month.

With Spryteo, the engine version is pinned in your project dependencies. Running a specific version guarantees identical output across developer machines and build environments over time.

### 3. Predictable cost

Cloud-based vectorization APIs charge per conversion, require account registrations, and enforce rate limits. Spryteo is fully open source and dual-licensed under MIT and Apache-2.0. There are no API keys, no subscription tiers, and no per-image fees. You can vectorize thousands of icons in a batch script without incurring usage charges.

### 4. Offline and CI reliability

Build pipelines should not fail because a third-party API is experiencing an outage or because a runner hit a network proxy error. Because Spryteo is self-contained, it drops cleanly into continuous integration runners, air-gapped workstations, and local build scripts.

## Current status

Spryteo is pre-1.0. The Rust engine crates (`spryteo-core`, `spryteo-raster`, `spryteo-quant`, `spryteo-trace`, and others) are available on crates.io at 0.0.2, alongside the `spryteo-cli` and `spryteo-mcp` binaries. The CLI and the native Node addon are also distributed on npm as `spryteo`, at 0.0.1.

You can review our command-line flags and options in the [documentation](/docs) or explore the source repository on [GitHub](https://github.com/chidi09/spryteo).
