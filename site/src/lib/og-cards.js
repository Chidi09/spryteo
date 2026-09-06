// The social card for each page, keyed by pathname.
//
// Plain JavaScript rather than TypeScript because two very different callers
// import it: the Astro build (through Vite) and scripts/og.mjs (through plain
// Node, with no bundler and no import.meta.env). Keeping it dependency-free is
// what lets one manifest drive both the generated art and the meta tags.
//
// Cards are generated into public/og/ by `npm run og` and committed, so the
// deployment never depends on a build step or a runtime renderer.

/**
 * @typedef {object} OgCard
 * @property {string} slug     Basename of the PNG in public/og/.
 * @property {string} eyebrow  Small label above the title.
 * @property {string} title    Headline. Kept short enough to set large.
 * @property {string} kicker   One supporting line beneath the title.
 */

/** @type {Record<string, OgCard>} */
export const cards = {
  '/': {
    slug: 'home',
    eyebrow: 'open source',
    title: 'Raster to animateable SVG',
    kicker: 'Semantic groups and stable IDs, not one flattened path. Runs entirely on your machine.',
  },
  '/docs': {
    slug: 'docs',
    eyebrow: 'documentation',
    title: 'Every option, every surface',
    kicker: 'CLI, Node, WebAssembly and MCP -- one engine, one set of options, one output.',
  },
  '/blog': {
    slug: 'blog',
    eyebrow: 'writing',
    title: 'Notes on vectorization',
    kicker: 'Why structured SVG output is a different problem from tracing an outline.',
  },
  '/sheet': {
    slug: 'sheet',
    eyebrow: 'sheet mode',
    title: 'Split a contact sheet',
    kicker: 'Detect every icon on a sheet and export each one as its own SVG.',
  },
  '/privacy': {
    slug: 'privacy',
    eyebrow: 'privacy',
    title: 'Nothing leaves your machine',
    kicker: 'No upload, no account, no telemetry. The browser demo runs the engine in your tab.',
  },
  '/terms': {
    slug: 'terms',
    eyebrow: 'terms',
    title: 'Terms of use',
    kicker: 'Dual-licensed MIT / Apache-2.0. Free for personal and commercial work.',
  },
};

/** Trailing slashes are load-bearing in neither direction; normalise them away. */
export function cardKey(pathname) {
  const trimmed = pathname.replace(/\/+$/, '');
  return trimmed === '' ? '/' : trimmed;
}

/**
 * The card for a path. Individual posts share the blog card rather than each
 * getting bespoke art, so writing a post never requires regenerating images.
 */
export function cardFor(pathname) {
  const key = cardKey(pathname);
  if (cards[key]) return cards[key];
  if (key.startsWith('/blog/')) return cards['/blog'];
  return cards['/'];
}
