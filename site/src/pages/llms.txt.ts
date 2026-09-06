// /llms.txt -- the plain-text brief for language models and answer engines.
//
// Generated rather than kept as a static file in public/, so the origin and
// the copy come from the same constants the rest of the site uses and cannot
// drift when the domain changes.

import type { APIRoute } from 'astro';
import { overview } from '../lib/llms';

export const GET: APIRoute = () =>
  new Response(overview(), {
    headers: { 'content-type': 'text/plain; charset=utf-8' },
  });
