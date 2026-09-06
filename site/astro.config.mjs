import { defineConfig } from 'astro/config';
import vercel from '@astrojs/vercel';

import sitemap from '@astrojs/sitemap';

export default defineConfig({
  // Keep in step with PUBLIC_SITE_URL in src/lib/seo.ts -- the sitemap is
  // generated from this value.
  site: process.env.PUBLIC_SITE_URL ?? 'https://spryteo.vercel.app',
  integrations: [sitemap()],
  adapter: vercel(),
});
