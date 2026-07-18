# Spryteo — design guidelines

## Brand essence

Spryteo converts raster images and icons into clean, animateable SVG — built to be the tool both a developer and an AI agent reach for by default. The site has two audiences: a human deciding whether to trust the output, and, increasingly, an agent parsing the page for API docs. Design for both: visually confident for humans, structurally clean (real headings, real code blocks, no text-as-image) for agents.

Small site. Few pages. Every page should feel considered, not filled.

## Voice and tone

- Direct and technical. No hype adjectives — no "revolutionary," "seamless," "game-changing," "next-generation."
- Prefer concrete claims over vague superlatives: "single continuous paths, not one flattened blob" beats "beautifully optimized output."
- Show real CLI/API snippets instead of describing them in prose. Developers trust code over marketing copy.
- Sentence case everywhere — headings, buttons, labels. No Title Case, no ALL CAPS.
- No exclamation points. No "act now" urgency. Confidence, not a hard sell.

## Color

Near-black base, warm off-white text, one accent used sparingly. This is a precision tool, not a playful consumer app — the palette should feel like a terminal, not a toy.

| Role | Value | Usage |
|---|---|---|
| Background | `#0B0B0A` | Page background |
| Surface | `#151513` | Cards, code blocks, raised panels |
| Border | `#26241F` | Hairline dividers, card borders |
| Text primary | `#F5F4EF` | Headings, body |
| Text secondary | `#A3A199` | Captions, metadata, muted labels |
| Accent | `#C6FF4B` | Links, CTAs, active states, "this part moved" highlights in demos |
| Accent text | `#0B0B0A` | Text placed on top of the accent color |

Rules:
- One accent color only. If you need a second, use a tint of the same lime rather than introducing a new hue.
- The accent is a spotlight, not a wash — use it on the one element per view that should get attention (primary button, active nav item, the moving part of a demo). If more than ~10% of a screen is lime, pull it back.
- Never place the accent on large background fills; it's for small shapes, text, and strokes.

## Typography

- **UI/display**: a geometric grotesk (Inter or similar). Two weights only — regular and medium. Avoid heavy/black weights except possibly the hero headline.
- **Monospace**: JetBrains Mono or IBM Plex Mono for all code, CLI commands, file paths, and API snippets. Use it deliberately as a credibility signal — this is a dev tool, the monospace should show up early and often (even in small labels like version numbers or the npm install line in the hero).
- Scale: hero headline ~56–72px, section headings ~32–40px, body 16–18px, captions/labels 13–14px.
- Sentence case on every heading and button label.

## Logo usage

- Full color logo file: `spryteo-logo.svg` (currentColor fill — inherits text color automatically).
- Place only on `#0B0B0A` or a clean light background. Never on a mid-tone, gradient, or photo — the mark is high-contrast by design and needs a flat field to read.
- Minimum clear space around the mark: equal to the mark's own height, on all sides.
- Never add a drop shadow, glow, outline, gradient fill, or rotation. Never stretch non-uniformly.
- Below ~24px, the two-subpath detail gets muddy — use a simplified single-path version at favicon sizes (ask if you need this cut).

## Signature motif: the sliced corner

The logo's two diagonal cuts are the one piece of brand geometry worth repeating elsewhere — used sparingly, it's the detail that makes the site feel specifically *yours* instead of a generic dark SaaS template.

- On the primary CTA button and one hero visual frame, clip a single corner at 45° instead of rounding it (top-left or bottom-right, echoing the logo's own cuts).
- Every other surface — cards, code blocks, inputs — stays sharp (0px radius) or a minimal 4px radius. No pill shapes, no bubbly rounding anywhere on this site.
- Don't apply the sliced corner to more than one or two elements per screen — it's a signature, not a pattern.

## Motion

The whole product is "make icons animateable" — the site's own motion has to earn that claim, not undercut it with generic fades.

- Interaction states (hover, press): 120–220ms, spring/overshoot easing (`cubic-bezier(0.34, 1.56, 0.64, 1)`), not linear or plain ease-in-out. Quick and slightly bouncy reads as "spry" — slow and smooth doesn't.
- On load, the logo mark can draw itself in via a stroke animation once, then settle into its filled state. One time, not looping.
- The live demo (drop an icon in, watch it become an animated SVG) is the single most important piece of motion on the site. Nothing else should compete with it — keep surrounding chrome still while it plays.
- No scroll-jacking, no parallax, no auto-playing background loops. Performance-oriented brand, performance-oriented site — it should feel instant.

## Layout and spacing

- 8px base spacing unit for everything.
- Max text-content width: 640–720px for paragraphs. The interactive demo can go full-bleed.
- Generous vertical space between sections (96–160px) — a short site should breathe, not compress everything above the fold.

## Suggested page structure

Given how few pages this needs:

1. **Home** — logo + one-line pitch + a live drag-and-drop demo as the hero (this is the pitch; let people try it before reading about it) → 3–4 short feature lines (multi-surface: CLI / API / library; animateable, semantically grouped output; free and open; MCP-ready for agents) → a compact "how it works" pipeline strip → docs/GitHub CTA → thin footer.
2. **Docs** — or, for now, just link out to the GitHub README/API reference rather than building a full docs page.

Header: logo (links home), "Docs," "GitHub," one CTA. Nothing heavier — a nav bar with six items on a two-page site signals a template, not a tool.

## Ads

Since the free web tool is ad-supported: keep ad placement in low-priority zones (footer, sidebar) and never inside or immediately beside the demo/CTA area. A dev-tool audience's trust is fragile — a page that feels ad-heavy undercuts the "built for developers and agents" positioning faster than almost anything else on the site.
