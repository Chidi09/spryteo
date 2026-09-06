import { defineConfig } from 'astro/config';
import vercel from '@astrojs/vercel';

import sitemap from '@astrojs/sitemap';

export default defineConfig({
  // Keep in step with PUBLIC_SITE_URL in src/lib/seo.ts -- the sitemap is
  // generated from this value.
  site: process.env.PUBLIC_SITE_URL ?? 'https://spryteo.vercel.app',
  integrations: [sitemap()],
  // @astrojs/sitemap emits sitemap-index.xml, but /sitemap.xml is the path
  // crawlers and submission tools try first. Point it at the index rather
  // than letting them find a 404.
  redirects: {
    '/sitemap.xml': '/sitemap-index.xml',
  },
  adapter: vercel(),
});
