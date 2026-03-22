// @ts-check
import { defineConfig } from 'astro/config';
import react from '@astrojs/react';
import tailwindcss from '@tailwindcss/vite';

// https://astro.build/config
export default defineConfig({
  // Static output for embedding in the Rust binary
  output: 'static',

  // Admin dashboard is served at /_/ (matching Pocketbase convention)
  base: '/_/',

  // Build output goes to dist/ which gets embedded via rust-embed or include_dir
  outDir: 'dist',

  integrations: [react()],

  vite: {
    plugins: [tailwindcss()],
  },
});
