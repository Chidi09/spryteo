import { defineConfig } from 'astro/config';
import vercel from '@astrojs/vercel';

import sitemap from '@astrojs/sitemap';

export default defineConfig({
  site: 'https://spryteo.dev',
  integrations: [sitemap()],
  adapter: vercel(),
});
