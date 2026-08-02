export { config as default }

import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

import { createKoreanTutorTurnVoiceSessionMiddleware } from '@ai/language-tutor/korean/createKoreanTutorTurnVoiceSessionMiddleware.ts'

const config = defineConfig({
  plugins: [
    svelte(),
    {
      name: 'tutor-voice-session',
      configureServer(server) {
        server.middlewares.use(createKoreanTutorTurnVoiceSessionMiddleware())
      },
      configurePreviewServer(server) {
        server.middlewares.use(createKoreanTutorTurnVoiceSessionMiddleware())
      },
    },
  ],
  resolve: {
    alias: {
      '#tutor': new URL('.', import.meta.url).pathname,
    },
  },
})
