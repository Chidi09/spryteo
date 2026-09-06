---
title: The idea behind Spryteo
description: Raster tracers usually ask what an image looks like rather than what it is made of; Spryteo treats vectorization output as a structured document.
pubDate: 2026-09-06
topic: philosophy
---

## The question tracers ask

Most raster-to-SVG converters ask a single question: what does this image look like?

When an algorithm approaches vectorization from that angle, the goal is visual similarity. The tool decodes a raster, segments pixels into color regions, traces borders around those regions, and emits SVG elements that reproduce the original image when rendered. If the raster and the rendered vector match on screen, the conversion is considered complete.

That goal makes sense if your only requirement is displaying an image at high resolution. But working developers rarely vectorize an asset just to look at it. You vectorize an asset because you need to work with it in code. You want to adjust colors for dark mode, animate a spinning icon on hover, stagger the reveal of components in an illustration, or attach click handlers to specific parts of a diagram.

When you attempt those tasks with the output of a conventional tracer, you hit an engineering dead end. The image looks correct, but it has no structure. It answers what the image looks like, but it cannot tell you what the image is made of.

## What a missing tree costs

Consider a common task: you have a raster logo consisting of an outer circular shield, an inner mechanical gear, and a centered glyph.

When you feed this bitmap into an ordinary vectorizer, the output is typically a flat sequence of paths. Some tracers merge every path of the same color into a single giant `<path>` element with disconnected subpaths. Others emit hundreds of tiny, unlabelled path fragments ordered arbitrarily by internal scanline traversal or hash table iteration.

Now try to work with that output:

If you want the inner gear to rotate when a user hovers over the button, which SVG element do you target? There is no element named gear. There is no group that isolates the gear from the shield or the background. Even if you inspect the raw SVG markup and find a path that looks like part of the gear, you will often find that the gear's teeth were merged into the same path data as the text glyph because both shared the same hex color.

If you try to animate a rotation using CSS transforms, you run into another issue: where is the transform origin? Without structural knowledge of the shape's bounding box and centroid, CSS `transform-origin` defaults to the top-left corner of the entire SVG canvas. Your gear does not spin on its axle; it swings across the whole container unless you manually calculate coordinates and apply offset values.

If you want to recolor the outer shield, you face the same obstacle. You cannot apply a fill rule to a parent container. You must dissect path commands, separate coordinates, and rebuild the vector by hand.

A flattened bag of paths is an inert picture, not a software document.

## A document, not a picture

Spryteo is built on a different premise: vectorization output should be a document, not a picture.

In web development, we do not treat HTML as a raw bitmap of text and layout. We treat it as a tree of semantic nodes -- headers, sections, buttons, and lists. Each node has boundaries, relationships to parent and sibling nodes, and an identity that scripts and style sheets can hook into.

SVG is an XML document designed around the same tree model. It provides container groups (`<g>`), coordinate systems, geometric primitives (`<circle>`, `<rect>`, `<ellipse>`), and attributes for identifier binding and paint styling. Flattening an entire image into a random collection of unorganized paths discards the capabilities that make SVG useful in modern software.

Treating vectorization output as a document means every visual object present in the original raster should map to an addressable entity in the resulting markup. The vectorizer must analyze the image to discover relationships, infer geometric meaning, and preserve boundaries.

When you take that premise seriously, several architectural requirements follow immediately.

## What follows from taking structure seriously

Building a vectorizer that outputs a document rather than a picture changes every stage of the pipeline.

### Semantic grouping

A document requires hierarchy. Instead of placing all paths at the root of the document, Spryteo organizes shapes into a `<g>` tree.

The engine analyzes spatial relationships between detected shapes. When one shape is fully enclosed within the bounds of another, Spryteo treats the outer shape as a container and nests the inner shape inside a child group. Connected components that form distinct visual units are kept together.

Because shapes are grouped by containment and connectivity, you can select an entire composite object using a single group selector. You can scale it, translate it, or fade it out without affecting surrounding elements.

### Stable content-hashed IDs

If you intend to style or animate an SVG in production, you must reference its elements. Most tools assign arbitrary sequential numbers like `path-1`, `path-2`, and `path-3`.

Sequential IDs are fragile. If you make a minor modification to the source image -- such as cleaning up a stray pixel in the corner -- a sequential generator renumbers every subsequent path. The path that was previously `#path-2` becomes `#path-3`. Any CSS rule, JavaScript animation timeline, or test selector bound to `#path-2` immediately breaks.

Spryteo generates stable, content-derived IDs. Each shape's identifier is a `blake3` hash calculated from its canonicalized geometry and fill color, prefixed as `s-<hash>`. If a shape's geometry and color do not change between builds, its ID remains identical, even if neighboring shapes are added, removed, or reordered. Your styling hooks stay intact across re-conversions.

### Primitive recognition

A circle in a raster icon is not meant to be a collection of arbitrary cubic Bezier curves. It was drawn as a circle. Treating it as path soup wastes bytes and makes manipulation difficult.

Spryteo includes a geometric analysis stage. After extracting and simplifying contours, the engine tests closed shapes against geometric models. If a contour fits a circle within a strict tolerance budget, Spryteo promotes it to a native `<circle>` element with explicit `cx`, `cy`, and `r` attributes. The same recognition applies to ellipses and rounded rectangles.

A native `<circle>` element is smaller in markup, renders cleanly, and exposes attributes that you can animate directly in CSS or JavaScript. Changing the radius of a circle in an interactive widget becomes updating an `r` attribute, rather than recomputing Bezier control points.

### True hole hierarchies

Consider tracing a letter "O" or an icon of a donut. There is an outer boundary and an inner cutout.

In many simple tracers, holes are handled by emitting two separate opaque shapes: an outer dark circle, followed by an inner white circle painted on top. If you place that SVG over a dark background or a photo, the inner circle reveals its fake nature by remaining an opaque white blotch.

Spryteo uses Suzuki-Abe border following to construct a true parent/child hole hierarchy during contour extraction. When a shape contains an inner void, Spryteo emits a single path with an inner subpath and specifies `fill-rule="evenodd"`. The hole is a genuine cutout. Whatever background sits behind the SVG shines through the opening.

### Determinism

If an SVG is a document that lives in a version-controlled repository, its generation must be deterministic.

Running the vectorizer on the same file with the same options must produce the exact same bytes every time, on every operating system. Spryteo eliminates non-deterministic sources across the pipeline: no random seeds, no hash map iteration order reaching output strings, and consistent coordinate formatting. Git diffs stay clean and meaningful.

## The engineering cost

Delivering a structured document instead of a raw picture requires work that conventional outline tracers skip entirely.

A basic tracer runs an edge detector, connects edges into loops, fits curves, and dumps the result. Spryteo runs an eleven-stage pipeline implemented in Rust. It classifies the input image, normalizes colors in Lab space, follows borders while maintaining hole hierarchies, checks straight-run chord tolerances before fitting curves, detects geometric primitives, builds a scene graph, and emits a structured metadata sidecar alongside the SVG.

This extra processing takes engineering discipline and computational effort. But the result is an asset that works as a first-class citizen in a modern web codebase.

## Current status

Spryteo is open source and dual-licensed under MIT and Apache-2.0.

The project is pre-1.0, with all engine crates and packages versioned together at 0.0.1. The core algorithms live in modular Rust crates on crates.io (`spryteo-core`, `spryteo-trace`, `spryteo-fit`, `spryteo-geom`, and others), distributed on npm as `spryteo` for the CLI and Node library, and `spryteo-mcp` for AI agent integration.

You can read the complete option and flag specifications in our [documentation](/docs) or examine the repository directly on [GitHub](https://github.com/chidi09/spryteo).
