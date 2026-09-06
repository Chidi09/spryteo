// Site-wide SEO facts and the JSON-LD graph built from them.
//
// Everything here is a single source of truth: the metadata in <head>, the
// Open Graph image, and the structured data all read the same constants, so
// they cannot drift apart.
//
// Structured data is emitted as one connected @graph rather than several
// standalone <script> blocks. Nodes reference each other by @id, which is
// what lets a crawler understand that the Organization publishes the
// WebSite, that the WebSite hosts this WebPage, and that the page is about
// the SoftwareApplication -- rather than seeing three unrelated objects.

// The live origin. Deployment is currently on the project's vercel.app
// host; point PUBLIC_SITE_URL at a custom domain and every canonical, OG
// URL, sitemap entry and JSON-LD @id follows from this one value.
import { cardFor } from './og-cards.js';

const ORIGIN = (import.meta.env.PUBLIC_SITE_URL ?? 'https://spryteo.vercel.app').replace(/\/+$/, '');

export const SITE = {
  url: ORIGIN,
  name: 'Spryteo',
  tagline: 'Raster to animateable SVG',
  description:
    'Spryteo is an open-source, local-first tool that converts raster images (PNG, JPEG, GIF, WebP, BMP) into clean, animateable, and semantically-grouped SVGs.',
  repo: 'https://github.com/chidi09/spryteo',
  npm: 'https://www.npmjs.com/package/spryteo',
  crates: 'https://crates.io/crates/spryteo-cli',
  license: 'https://spdx.org/licenses/MIT.html',
  locale: 'en_US',
  themeColor: '#0B0B0A',
  accent: '#C6FF4B',
} as const;

/** Stable @id anchors. Fragment ids keep every node addressable. */
export const ID = {
  org: `${SITE.url}/#organization`,
  site: `${SITE.url}/#website`,
  app: `${SITE.url}/#software`,
  source: `${SITE.url}/#sourcecode`,
  blog: `${SITE.url}/blog#blog`,
  page: (path: string) => `${canonical(path)}#webpage`,
} as const;

/** One canonical form per path: absolute, no trailing slash except root. */
export function canonical(pathname: string): string {
  if (!pathname || pathname === '/') return `${SITE.url}/`;
  const path = pathname.startsWith('/') ? pathname : `/${pathname}`;
  return `${SITE.url}${path.replace(/\/+$/, '')}`;
}

/**
 * The social card for a page: a static PNG under /og/, generated from
 * src/lib/og-cards.js by `npm run og` and committed.
 *
 * Cards used to be rendered per request by @vercel/og. They are not any more:
 * that package's published tarball omits the hb.wasm its Node build loads at
 * import time, so the route failed wherever it ran. Serving a committed file
 * also means a card cannot go missing because a function cold-started badly,
 * and every crawler gets it straight from the CDN.
 */
export function ogImage(pathname: string): string {
  return `${SITE.url}/og/${cardFor(pathname).slug}.png`;
}

export interface Crumb {
  name: string;
  path: string;
}

type Node = Record<string, unknown>;

function organization(): Node {
  return {
    '@type': 'Organization',
    '@id': ID.org,
    name: SITE.name,
    url: `${SITE.url}/`,
    logo: {
      '@type': 'ImageObject',
      url: `${SITE.url}/spryteo-logo.svg`,
      caption: `${SITE.name} logo`,
    },
    sameAs: [SITE.repo, SITE.npm, SITE.crates],
  };
}

function website(): Node {
  return {
    '@type': 'WebSite',
    '@id': ID.site,
    url: `${SITE.url}/`,
    name: SITE.name,
    description: SITE.description,
    publisher: { '@id': ID.org },
    inLanguage: 'en',
  };
}

/**
 * The product itself. `SoftwareApplication` is what makes a tool eligible for
 * rich results; the free-of-charge Offer is required for the "Free" label,
 * and featureList is the part an answer engine quotes when asked what the
 * tool can do.
 */
function softwareApplication(): Node {
  return {
    '@type': 'SoftwareApplication',
    '@id': ID.app,
    name: SITE.name,
    alternateName: 'Spryteo image vectorizer',
    url: `${SITE.url}/`,
    description: SITE.description,
    applicationCategory: 'DeveloperApplication',
    applicationSubCategory: 'Image vectorization / raster-to-SVG conversion',
    operatingSystem: 'Linux, macOS, Windows, Web browser',
    softwareRequirements: 'Node.js 18+ for the library and CLI; any modern browser for the WebAssembly build',
    downloadUrl: SITE.npm,
    installUrl: SITE.npm,
    sameAs: [SITE.repo, SITE.npm, SITE.crates],
    codeRepository: SITE.repo,
    license: SITE.license,
    isAccessibleForFree: true,
    offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
    featureList: [
      'Convert PNG, JPEG, GIF, WebP and BMP images to SVG',
      'Semantically grouped <g> tree instead of one flattened path',
      'Stable content-hashed IDs on every shape, so parts can be selected and animated',
      'Primitive recognition for icons: circles and rectangles instead of traced approximations',
      'Colour quantization and optional gradient detection for photographs',
      'Deterministic output -- identical input and options give byte-identical SVG',
      'Runs entirely offline; images are never uploaded',
      'Available as a CLI, a Node.js library, a WebAssembly browser build and an MCP server',
    ],
    publisher: { '@id': ID.org },
    mainEntityOfPage: { '@id': ID.site },
  };
}

/** The repository, declared as source code so the licence is machine-readable. */
function softwareSourceCode(): Node {
  return {
    '@type': 'SoftwareSourceCode',
    '@id': ID.source,
    name: `${SITE.name} source`,
    codeRepository: SITE.repo,
    programmingLanguage: ['Rust', 'TypeScript', 'WebAssembly'],
    license: SITE.license,
    about: { '@id': ID.app },
  };
}

function webPage(opts: {
  path: string;
  title: string;
  description: string;
  image: string;
  crumbs?: Crumb[];
}): Node {
  const url = canonical(opts.path);
  const node: Node = {
    '@type': 'WebPage',
    '@id': ID.page(opts.path),
    url,
    name: opts.title,
    description: opts.description,
    isPartOf: { '@id': ID.site },
    about: { '@id': ID.app },
    primaryImageOfPage: { '@type': 'ImageObject', url: opts.image },
    inLanguage: 'en',
  };
  if (opts.crumbs?.length) {
    node.breadcrumb = { '@id': `${url}#breadcrumb` };
  }
  return node;
}

function breadcrumbList(path: string, crumbs: Crumb[]): Node {
  return {
    '@type': 'BreadcrumbList',
    '@id': `${canonical(path)}#breadcrumb`,
    itemListElement: crumbs.map((c, i) => ({
      '@type': 'ListItem',
      position: i + 1,
      name: c.name,
      item: canonical(c.path),
    })),
  };
}

/** FAQPage built from the questions actually rendered on the page. */
export function faqPage(path: string, faqs: { q: string; a: string }[]): Node {
  return {
    '@type': 'FAQPage',
    '@id': `${canonical(path)}#faq`,
    mainEntity: faqs.map((f) => ({
      '@type': 'Question',
      name: f.q,
      acceptedAnswer: { '@type': 'Answer', text: f.a },
    })),
  };
}

/** HowTo built from the pipeline steps actually rendered on the page. */
export function howTo(
  path: string,
  opts: { name: string; description: string; steps: { title: string; desc: string }[] }
): Node {
  return {
    '@type': 'HowTo',
    '@id': `${canonical(path)}#howto`,
    name: opts.name,
    description: opts.description,
    totalTime: 'PT1M',
    tool: { '@id': ID.app },
    step: opts.steps.map((s, i) => ({
      '@type': 'HowToStep',
      position: i + 1,
      name: s.title,
      text: s.desc,
    })),
  };
}

/** Documentation, declared as such so it is eligible for the docs treatment. */
export function techArticle(opts: {
  path: string;
  headline: string;
  description: string;
  image: string;
}): Node {
  return {
    '@type': 'TechArticle',
    '@id': `${canonical(opts.path)}#article`,
    headline: opts.headline,
    description: opts.description,
    image: opts.image,
    about: { '@id': ID.app },
    isPartOf: { '@id': ID.site },
    author: { '@id': ID.org },
    publisher: { '@id': ID.org },
    inLanguage: 'en',
    proficiencyLevel: 'Beginner',
  };
}

export function blogPosting(opts: {
  path: string;
  headline: string;
  description: string;
  image: string;
  datePublished: string;
  dateModified?: string;
  section?: string;
}): Node {
  return {
    '@type': 'BlogPosting',
    '@id': `${canonical(opts.path)}#post`,
    headline: opts.headline,
    description: opts.description,
    image: opts.image,
    datePublished: opts.datePublished,
    dateModified: opts.dateModified ?? opts.datePublished,
    articleSection: opts.section,
    about: { '@id': ID.app },
    isPartOf: { '@id': ID.blog },
    author: { '@id': ID.org },
    publisher: { '@id': ID.org },
    mainEntityOfPage: { '@id': ID.page(opts.path) },
    inLanguage: 'en',
  };
}

/** The Blog node itself, for the listing page. */
export function blog(posts: { path: string; headline: string }[]): Node {
  return {
    '@type': 'Blog',
    '@id': ID.blog,
    name: `${SITE.name} blog`,
    description: 'Notes on vectorization, structured SVG output, and the ideas behind Spryteo.',
    url: canonical('/blog'),
    isPartOf: { '@id': ID.site },
    publisher: { '@id': ID.org },
    inLanguage: 'en',
    blogPost: posts.map((p) => ({
      '@type': 'BlogPosting',
      '@id': `${canonical(p.path)}#post`,
      headline: p.headline,
      url: canonical(p.path),
    })),
  };
}

/**
 * Assemble the page's structured data. The four site-level nodes are repeated
 * on every page on purpose: each page must stand alone, because a crawler may
 * only ever fetch this one URL.
 */
export function buildGraph(opts: {
  path: string;
  title: string;
  description: string;
  image: string;
  crumbs?: Crumb[];
  extra?: Node[];
}) {
  const nodes: Node[] = [
    organization(),
    website(),
    softwareApplication(),
    softwareSourceCode(),
    webPage(opts),
  ];
  if (opts.crumbs?.length) nodes.push(breadcrumbList(opts.path, opts.crumbs));
  if (opts.extra?.length) nodes.push(...opts.extra);
  return { '@context': 'https://schema.org', '@graph': nodes };
}
