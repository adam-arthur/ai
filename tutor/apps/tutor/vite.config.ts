export { config as default }

import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

const config = defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      '#tutor': new URL('.', import.meta.url).pathname,
    },
  },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:3000',
    },
  },
})
