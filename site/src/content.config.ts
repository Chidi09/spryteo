// The blog collection.
//
// Posts are plain Markdown under src/content/blog/. The schema is deliberately
// small and entirely required-or-defaulted: a post that forgets a field fails
// the build rather than rendering a page with a blank date or no description,
// which is the failure mode that quietly poisons structured data.

import { defineCollection, z } from 'astro:content';
import { glob } from 'astro/loaders';

const blog = defineCollection({
  loader: glob({ base: './src/content/blog', pattern: '**/*.md' }),
  schema: z.object({
    title: z.string(),
    // Shown on the card, in <meta name="description">, and in the JSON-LD.
    description: z.string(),
    pubDate: z.coerce.date(),
    updatedDate: z.coerce.date().optional(),
    // One-word section label, rendered above the title.
    topic: z.string().default('notes'),
    // Ordering hint for the listing; higher floats to the top within a date.
    featured: z.boolean().default(false),
    draft: z.boolean().default(false),
  }),
});

export const collections = { blog };
