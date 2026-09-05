// Generated from the Rust `Meta` type by spryteo-core.
// Do not edit by hand: run `cargo test -p spryteo-core` to check it,
// and `UPDATE_SCHEMA=1 cargo test -p spryteo-core` to regenerate.

/**
 * Axis-aligned bounding box for a shape or group.
 */
export interface Bbox {
  x_max: number;
  x_min: number;
  y_max: number;
  y_min: number;
}

/**
 * A gradient stop representing a color at a specific offset.
 */
export interface GradientStop {
  color: Rgb;
  offset: number;
}

/**
 * One `<g>` in the metadata sidecar's group tree.
 *
 * Mirrors `Group`: `nodes` lists the ids painted directly by this group,
 * in paint order, and `groups` its nested child groups, which paint after
 * those nodes.
 */
export interface GroupMeta {
  /**
   * Nested child groups, painted after this group's own nodes.
   */
  groups?: GroupMeta[];
  id: string;
  /**
   * Ids of the nodes painted directly by this group, in paint order.
   */
  nodes: string[];
}

/**
 * Per-element metadata recorded in the sidecar.
 *
 * Fields added after schema version 0 all carry serde defaults, so a
 * sidecar written by an older build still deserializes — see
 * [`META_SCHEMA_VERSION`].
 */
export interface NodeMeta {
  area: number;
  bbox: Bbox;
  centroid: [number, number];
  /**
   * Whether the outline closes back on itself. Primitives always do;
   * centreline strokes generally do not.
   */
  closed?: boolean;
  /**
   * One representative colour, kept for consumers that only want to know
   * roughly what colour a shape is. [`NodeMeta::paint`] is the lossless
   * form; for a gradient this is the first stop.
   */
  fill?: Rgb | null;
  /**
   * The id of the node's immediate parent `<g>`.
   */
  group: string;
  /**
   * Ids of every enclosing group, outermost first, ending with
   * [`NodeMeta::group`]. Answers "which object is this part of" without
   * walking [`Meta::groups`].
   */
  group_path?: string[];
  id: string;
  /**
   * Fill paint without loss. `None` for a node that is stroked only.
   */
  paint?: PaintMeta | null;
  /**
   * Outline length in user units, following the shape as drawn.
   *
   * Stroke output declares `pathLength="100"`, so a dash offset expressed
   * as a percentage of this length maps straight onto the emitted
   * `stroke-dasharray` without rescaling.
   */
  path_length?: number;
  /**
   * The element this node is emitted as.
   */
  shape?: ShapeKind;
  /**
   * Stroke paint and width. `None` for a node that is filled only.
   */
  stroke?: StrokeMeta | null;
  /**
   * A reveal order for animation, which is not paint order — see
   * [`Meta::suggested_draw_order`](Meta) and the module docs on
   * `suggested_draw_order`.
   */
  suggested_draw_order: number;
  /**
   * Position in paint order: 0 paints first, and later entries paint over
   * earlier ones.
   */
  z_order: number;
}

/**
 * Paint on a node, recorded without loss.
 *
 * `NodeMeta::fill` reduces paint to a single representative colour, which
 * is all a thumbnail or a colour-swap needs but throws away everything a
 * gradient is. This keeps the stops and their geometry, so a consumer can
 * reproduce or retarget the paint without going back to the SVG.
 */
export type PaintMeta =
  | {
      color: Rgb;
      kind: "solid";
    }
  | {
      fallback: Rgb;
      kind: "current-color";
    }
  | {
      kind: "linear-gradient";
      stops: GradientStop[];
      x1: number;
      x2: number;
      y1: number;
      y2: number;
    }
  | {
      cx: number;
      cy: number;
      kind: "radial-gradient";
      r: number;
      stops: GradientStop[];
    };

/**
 * An sRGB 8-bit colour with red, green, and blue components.
 */
export interface Rgb {
  b: number;
  g: number;
  r: number;
}

/**
 * The SVG element a node is emitted as.
 *
 * A consumer that wants to animate a radius, or to know whether a shape
 * survived primitive detection, would otherwise have to parse the markup
 * to find out.
 */
export type ShapeKind = "path" | "circle" | "ellipse" | "rect" | "arc";

/**
 * Aggregate statistics for the entire conversion result.
 */
export interface Stats {
  byte_count: number;
  node_count: number;
  path_count: number;
}

/**
 * Stroke paint and width on a node. Absent for filled shapes.
 */
export interface StrokeMeta {
  paint: PaintMeta;
  /**
   * Stroke width in user units.
   */
  width: number;
}

/**
 * The metadata sidecar (§3.12) — a JSON-serialisable payload returned
 * alongside the SVG with per-node layout information and aggregate stats.
 *
 * This is the primary machine-oriented output that enables agentic
 * workflows: a consumer can inspect every shape's bounding box, centroid,
 * area, fill, group membership, and paint order without parsing SVG.
 */
export interface Meta {
  current_color_applied?: boolean;
  /**
   * The `<g>` tree exactly as emitted in the SVG (§3.12, #11).
   *
   * Same shape, same ids, same order as the markup, so a consumer can
   * bind to a whole object without parsing the SVG. Empty only for a
   * scene with no groups.
   */
  groups?: GroupMeta[];
  nodes: NodeMeta[];
  /**
   * The schema this payload was written against; see
   * [`META_SCHEMA_VERSION`]. Absent in pre-versioning sidecars, which
   * therefore read back as [`META_SCHEMA_UNVERSIONED`].
   */
  schema_version?: number;
  stats: Stats;
}

/**
 * The top-level result returned by every conversion surface.
 */
export interface ConvertResult {
  meta: Meta;
  svg: string;
}
