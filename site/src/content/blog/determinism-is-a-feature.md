---
title: Determinism is a feature
description: Spryteo guarantees byte-identical SVG output for identical inputs and options, enabling reviewable git diffs, cacheable builds, and strict CI tests.
pubDate: 2026-09-04
topic: engineering
---

## The problem with quiet non-determinism

Most graphics conversion tools treat output as visually acceptable if the rendered image looks right to human eyes. Under that standard, minor variations between runs are dismissed as harmless implementation details.

In practice, those minor variations create friction across modern development workflows.

If you run an ordinary image tracer on the same PNG file twice, you will often get two different SVG files. One run might list color layers in a slightly different order because the internal palette clusters were initialized with a random number generator. Another run might reorder path elements because the algorithm iterated over an unordered hash table whose bucket traversal order changes on every program execution. A third run might inject a creation timestamp into an XML comment or produce slight floating-point rounding discrepancies across different operating systems.

When a tool behaves this way, you cannot comfortably use it in an automated pipeline. Every time a build script runs, your version control system sees modified files. Pull requests fill with hundreds of lines of noise that obscure real design changes. Content-addressable build caches are invalidated needlessly. Automated tests that assert output properties become flaky.

In Spryteo, determinism is not an afterthought. It is a core engineering requirement: identical input bytes and identical options produce byte-identical SVG output on every run and every platform.

## Why byte determinism matters

Enforcing byte-level determinism unlocks three capabilities that non-deterministic tools cannot provide.

### Reviewable git diffs

When vector assets live in a software repository, code reviews should focus on intentional changes.

If a designer updates a raster logo by adjusting the curve of a lettermark, the resulting pull request should only show diff lines corresponding to that lettermark. In Spryteo, because shape ordering and element IDs are content-derived and stable, untouched shapes remain identical down to the byte. You do not get phantom diffs where fifty unchanged paths are reordered or renumbered. Reviewers can verify what actually changed.

### Cacheable builds

Modern build systems -- from Turborepo and Vite to Bazel and Nix -- rely on content hashing to determine whether a build step needs to run.

If an asset conversion step produces different bytes from identical inputs, every downstream task that depends on that asset must be re-executed. A non-deterministic vectorizer invalidates artifact caches, triggers unnecessary re-bundling, and slows down local builds and deployment pipelines. Deterministic output guarantees that if the input image and configuration flags have not changed, the resulting output hash remains identical.

### A testable golden corpus

A vectorization pipeline involves intricate geometric algorithms: Suzuki-Abe contour extraction, marching-squares interpolation, polygonal simplification, and Bezier curve fitting. Regressions can be subtle.

Because Spryteo is deterministic, its test suite can enforce strict golden-corpus verification (`spryteo-cli/tests/corpus.rs`). In CI, dozens of test fixtures spanning icons, pixel art, line drawings, and photos are converted and compared against committed baseline files.

The harness renders every generated SVG back to a raster using `resvg` and evaluates structural metrics:
- Structural similarity (SSIM) against the source raster (with thresholds of 0.92 for icons and pixel art, 0.90 for line art, and 0.85 for photos).
- Strict byte budgets per fixture.
- Strict node and path count ceilings per fixture.

Because the engine is deterministic, these tests are not probabilistic. They run in CI on Linux, macOS, and Windows without flakiness. If a refactoring alters an output path by a single byte or increases a node count, the build fails immediately and alerts the developer.

## What determinism forces on implementation

Achieving byte-identical output across diverse operating systems and compiler targets requires discipline throughout the codebase.

### No hash map iteration in output paths

In Rust, the standard `HashMap` uses randomized SipHash keys to protect against hash-collision denial-of-service attacks. A direct consequence is that iterating over a `HashMap` yields keys in an unpredictable order that changes between process executions.

In Spryteo, no `HashMap` iteration is ever allowed to reach an output path. Where key-value lookups must be serialized or traversed, the engine uses `BTreeMap` to enforce lexicographical ordering, or collects elements into vectors and sorts them with explicit, stable comparison keys before processing.

### Fixed random seeds

Color quantization often uses clustering techniques like k-means. Traditional implementations initialize cluster centroids by picking random pixels using system entropy or the current timestamp. This causes different runs to settle into different local minima, yielding slightly different palette assignments and layer boundaries.

Spryteo uses a deterministic variant of k-means++ initialization with a fixed seed. Given the same quantized image histogram and target color count, the clustering algorithm executes the exact same arithmetic steps and arrives at the exact same palette every time.

### Order-independent parallel reduction

To maintain fast conversion times, Spryteo uses Rayon to parallelize work across color layers and contour sets. Parallel execution introduces non-determinism if worker threads append results to a shared collection as they finish.

Spryteo structures parallel tasks so that results are reduced using associative, deterministic merge operations or indexed writes into pre-allocated slices. The sequence of layers and shapes in the scene graph is governed by topological containment and paint order, never by the thread that finished first.

### Numeric hygiene and coordinate formatting

Floating-point arithmetic can introduce subtle variations across platforms due to differing SIMD implementations or compiler optimization flags.

Spryteo mitigates this by canonicalizing coordinates during SVG emission. Floating-point values are rounded to a configurable precision (`--precision`, default 2 decimal places, or 1 decimal place for small icons). Furthermore, IEEE 754 negative zero (`-0.0`) is explicitly normalized to positive zero (`0.0`) before formatting. This prevents platform-specific sign discrepancies from appearing in the output text.

### No wall-clock timestamps or environment leakage

Spryteo emits only the SVG markup and the metadata required to represent the scene graph. The generator writes no timestamps, no environment variables, no machine hostnames, and no random UUIDs.

## Current status

Every component in the Spryteo workspace -- from `spryteo-core` and `spryteo-quant` to `spryteo-trace` and `spryteo-svg` -- is built under this determinism contract.

The project is pre-1.0. The Rust engine crates are published on crates.io at 0.0.2 and the npm distribution -- the `spryteo` CLI and Node addon -- is at 0.0.1, both under dual MIT / Apache-2.0 licenses. You can read more about our pipeline stages in the [documentation](/docs) or examine the test suite on [GitHub](https://github.com/chidi09/spryteo).
