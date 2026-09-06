// Generates the Open Graph cards in public/og/ from src/lib/og-cards.js.
//
// Run with `npm run og` after changing a card's copy; the PNGs are committed,
// so nothing renders at build time or at request time. That is deliberate:
// @vercel/og cannot run here (its published tarball omits the hb.wasm its Node
// build loads at import, so the route 500s wherever it is deployed), and a
// social card that only exists if a serverless function succeeds is a card
// that silently disappears the first time that function fails.
//
// satori lays the tree out and emits SVG; resvg rasterises it. Those are the
// same two libraries @vercel/og wraps, minus the broken packaging.

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import satori from 'satori';
import { Resvg } from '@resvg/resvg-js';
import { cards } from '../src/lib/og-cards.js';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..', 'public', 'og');

// Kept in step with src/styles/tokens.css by hand -- satori has no CSS engine,
// so these cannot be read from the stylesheet.
const BG = '#0B0B0A';
const SURFACE = '#151513';
const BORDER = '#26241F';
const TEXT = '#F5F4EF';
const MUTED = '#A3A199';
const ACCENT = '#C6FF4B';

const fonts = [
  { name: 'Inter', weight: 400, style: 'normal', data: readFileSync(join(here, 'fonts', 'Inter-Regular.ttf')) },
  { name: 'Inter', weight: 600, style: 'normal', data: readFileSync(join(here, 'fonts', 'Inter-SemiBold.ttf')) },
];

// satori treats an empty children array as "more than one child" and demands an
// explicit display, so a leaf node must pass no children at all.
const div = (style, children) => ({
  type: 'div',
  props: Array.isArray(children) && children.length === 0 ? { style } : { style, children },
});

function card({ eyebrow, title, kicker }) {
  return div(
    {
      width: '1200px',
      height: '630px',
      display: 'flex',
      flexDirection: 'column',
      justifyContent: 'space-between',
      background: BG,
      padding: '72px',
      fontFamily: 'Inter',
    },
    [
      div({ display: 'flex', alignItems: 'center', gap: '16px' }, [
        div({ width: '44px', height: '6px', background: ACCENT }),
        div({ color: MUTED, fontSize: '26px', letterSpacing: '0.08em' }, eyebrow.toUpperCase()),
      ]),
      div({ display: 'flex', flexDirection: 'column', gap: '24px' }, [
        div(
          {
            color: TEXT,
            fontSize: title.length > 40 ? '68px' : '84px',
            fontWeight: 600,
            lineHeight: 1.05,
            letterSpacing: '-0.03em',
          },
          title,
        ),
        div({ color: MUTED, fontSize: '30px', lineHeight: 1.4, maxWidth: '940px' }, kicker),
      ]),
      div(
        {
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderTop: `1px solid ${BORDER}`,
          paddingTop: '32px',
        },
        [
          div({ color: ACCENT, fontSize: '30px', fontWeight: 600, letterSpacing: '-0.02em' }, 'Spryteo'),
          div(
            { display: 'flex', gap: '12px' },
            ['CLI', 'Node', 'WASM', 'MCP'].map((label) =>
              div(
                {
                  display: 'flex',
                  background: SURFACE,
                  border: `1px solid ${BORDER}`,
                  color: MUTED,
                  fontSize: '22px',
                  padding: '8px 16px',
                },
                label,
              ),
            ),
          ),
        ],
      ),
    ],
  );
}

mkdirSync(out, { recursive: true });

for (const [path, spec] of Object.entries(cards)) {
  const svg = await satori(card(spec), { width: 1200, height: 630, fonts });
  const png = new Resvg(svg, { fitTo: { mode: 'width', value: 1200 } }).render().asPng();
  const file = join(out, `${spec.slug}.png`);
  writeFileSync(file, png);
  console.log(`${path.padEnd(10)} -> public/og/${spec.slug}.png  ${(png.length / 1024).toFixed(1)} kB`);
}
