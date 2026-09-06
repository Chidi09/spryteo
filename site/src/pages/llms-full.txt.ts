// /llms-full.txt -- the brief plus the complete documentation, in one fetch.
//
// The llms.txt convention pairs a short index with a full-text companion so a
// model can take either the summary or everything without crawling. The docs
// are inlined from the same Markdown the site serves at /docs.md, so there is
// one source of truth for the prose.

import type { APIRoute } from 'astro';
import docs from '../../public/docs.md?raw';
import { full } from '../lib/llms';

export const GET: APIRoute = () =>
  new Response(full(docs), {
    headers: { 'content-type': 'text/plain; charset=utf-8' },
  });
