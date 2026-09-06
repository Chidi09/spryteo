---
title: Why structured SVG matters
description: Flat tracers emit disconnected path soup; Spryteo generates a nested group tree with content-hashed IDs and native geometric primitives.
pubDate: 2026-09-05
topic: output
---

## The flat path trap

When you run a standard vectorizer on a raster icon, the resulting SVG markup usually looks like an opaque wall of path data.

Conventional tracers approach vectorization as a pixel-fitting problem. They detect color boundaries, follow edges, and emit paths. In most engines, the output takes one of two shapes: either a flat sequence of hundreds of disjoint `<path>` elements placed directly inside the `<svg>` root, or a single massive path per color that stitches together disconnected regions using jump commands.

Here is an illustrative snippet showing the structure commonly produced by flat tracers:

```xml
<!-- Illustrative shape of conventional flat tracer output -->
<svg viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
  <path fill="#2b2d42" d="M12.4 15.2 C12.4 ... Z M45.1 18.3 C45.1 ... Z" />
  <path fill="#e63946" d="M32.0 10.1 C24.1 10.1 17.8 16.4 17.8 24.3 C17.8 ... Z" />
</svg>
```

This markup renders fine in an `<img>` tag. But if you try to build an interface around it, clear problems appear:

1. **No semantic hierarchy.** The shapes have no containers. The markup contains only unrelated path tags.
2. **Merged subpaths.** If two disconnected shapes share a fill color, they are combined into a single `d="..."` string. You cannot select or animate one without affecting the other.
3. **Approximated geometry.** Circular contours are traced as sequences of cubic Bezier curves (`C` commands).
4. **No stable identity.** The elements have no IDs, or they have sequential numbers like `path-1` and `path-2` that shift on the next export.

You cannot attach CSS animations, apply transitions, or manipulate individual components through the DOM.

## The structured alternative

Spryteo treats SVG as a scene graph rather than an arbitrary stream of drawing instructions.

Instead of dumping flat paths into the root element, the engine builds a hierarchical `<g>` tree based on spatial relationships and connected components. When one shape is geometrically enclosed by another, Spryteo treats the outer shape as a parent container and places the inner shape inside a child group.

Here is an illustrative snippet showing the structure emitted by Spryteo:

```xml
<!-- Illustrative shape of Spryteo structured output -->
<svg viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
  <g id="g-s-0">
    <circle id="s-a1b2c3d4" cx="32" cy="32" r="24" fill="#2b2d42" />
    <g id="g-s-1">
      <circle id="s-e5f60718" cx="32" cy="32" r="14" fill="#edf2f4" />
      <path id="s-89ab4c2d" d="M28 26 L36 32 L28 38 Z" fill="#e63946" />
    </g>
  </g>
</svg>
```

The difference is immediate. The inner shapes live inside a logical group (`#g-s-1`) nested under the outer container (`#g-s-0`). Every element has an addressable identity and clear parentage.

Writing an animation in CSS requires no manual surgery on the SVG:

```css
#g-s-1 {
  transform-origin: 32px 32px;
  transition: transform 200ms ease-out;
}

svg:hover #g-s-1 {
  transform: scale(1.1);
}
```

Because hierarchy exists in markup, CSS selectors target logical components, and transforms apply cleanly to groups.

## Stable IDs: content-hashed versus sequential

Any project that styles or animates SVG elements in production needs element IDs. But how those IDs are generated determines whether your code is maintainable.

Most tools that offer an ID option generate sequential counters: `#path-1`, `#path-2`, `#path-3`.

Sequential IDs fail under real-world version control. If you remove a speck of noise, or if quantization separates one extra tone, the sequential counter renumbers every subsequent path in the document. What was `#path-4` becomes `#path-5`. Any CSS rule, animation handle, or UI test selector referencing `#path-4` targets the wrong shape.

Spryteo avoids this by using content-derived IDs by default (`--id-style hash`).

Each shape's ID is generated using a `blake3` hash calculated over:
- Canonicalized path commands and coordinates (rounded to 3 decimal places, with `-0.0` normalized to `0.0`).
- The recognized primitive type and its parameters.
- The fill paint or stroke definition.

The hash is formatted as `s-` followed by 8 hexadecimal characters, such as `id="s-a1b2c3d4"`.

This design guarantees that an ID survives external changes. If you add, remove, or reorder other shapes, an untouched shape retains its exact ID. It also survives coordinate noise below 0.0005 units. An ID only changes when the shape itself changes: if its geometry moves, its fill color changes, or it is promoted to a different primitive.

If an asset contains multiple identical shapes, Spryteo separates them deterministically by appending `-2`, `-3`, and so on in document order. Adding a fourth star will never renumber the first three.

## Primitive recognition

One of the largest sources of bloat in traced vector files is the absence of primitive geometry.

In a flat tracer, a circle is traced as four cubic Bezier curves with eight control points. If you need to make that circle larger in code, you cannot simply update a radius; you must recompute control points for every curve segment.

Spryteo includes a geometric recognition stage in its pipeline (`spryteo-geom`):

- **Circles and ellipses:** Contours whose points lie within the tolerance budget of a fitted circle or ellipse are promoted to native `<circle>` or `<ellipse>` elements.
- **Rectangles and rounded rects:** Contours with four straight edges and right-angled corners are emitted as `<rect>` elements, preserving corner radii via `rx`.
- **Straight-run recovery:** Before fitting curves, Spryteo performs a chord-straightness check (using a 0.5px tolerance budget). If points between two corners lie along a straight line, it emits a plain `LineTo` (`L`) command and collapses consecutive collinear runs, rather than bending straight edges into curves.
- **True hole subpaths:** For compound shapes like the letter "O", Spryteo uses Suzuki-Abe border following to establish a parent/child contour hierarchy. It emits a single path with an inner subpath and `fill-rule="evenodd"`. Holes are true cutouts, not opaque shapes painted on top.

Real geometric primitives make the SVG smaller, easier to read, and simpler to animate. You can animate `<circle r="...">` or `<rect width="...">` directly via CSS without touching path data strings.

## The metadata sidecar

Every Spryteo surface returns both the SVG string and a JSON metadata sidecar (`{ svg, meta }`).

The sidecar provides a programmatic description of the scene graph:

```json
{
  "schema_version": 1,
  "stats": {
    "node_count": 3,
    "path_count": 1,
    "byte_count": 412
  },
  "nodes": [
    {
      "id": "s-a1b2c3d4",
      "shape": "circle",
      "bbox": [8.0, 8.0, 56.0, 56.0],
      "centroid": [32.0, 32.0],
      "area": 1809.56,
      "fill": "#2b2d42",
      "group": "g-s-0",
      "z_order": 0,
      "suggested_draw_order": 0
    }
  ]
}
```

The sidecar carries properties that are expensive to compute after the fact: exact bounding boxes, centroids, areas, and resolved paints.

Notice the distinction between `z_order` and `suggested_draw_order`. The `z_order` represents paint order: what covers what on the canvas. The `suggested_draw_order` is a reveal sequence designed for animation. It sorts shapes by nesting depth first, then by descending area, using paint order as a tiebreak.

Animation libraries and agents can inspect this metadata to calculate coordinate offsets or sequence staggered entrances without parsing XML.

## Summary

A vector image should be an editable, interactive asset. By structuring paths into a logical `<g>` tree, assigning stable content-hashed IDs, and recovering true geometric primitives, Spryteo turns vectorization into a tool for building software components.

Spryteo is open source (dual MIT / Apache-2.0) and pre-1.0, with all crates and npm packages currently at version 0.0.1. You can explore the CLI and options in the [documentation](/docs) or review the source code on [GitHub](https://github.com/chidi09/spryteo).
