# Tutor

A personal Korean voice tutor with a Rust server and a Svelte browser interface. The browser records a complete learner turn, then the server transcribes it, identifies useful corrections, generates the tutor's reply, and synthesizes its audio.

## Workspace

- `apps/tutor-server`: executable, configuration, and dependency wiring
- `apps/tutor`: Svelte UI and browser audio controls
- `crates/llm-core`: provider-neutral model traits and data types
- `crates/llm-openai`: OpenAI text, transcription, and speech REST adapter
- `crates/llm-google`: Gemini text REST adapter
- `crates/language-tutor`: Korean tutoring prompts and turn processing
- `crates/voice-session`: in-memory turn session lifecycle
- `crates/tutor-api`: HTTP/SSE routes and generated browser contracts
- `packages/browser-audio`: browser microphone, VAD, and PCM playback utilities

## Development

Copy `.env.example` to `.env` and set `OPENAI_API_KEY` and `GEMINI_API_KEY`. The server temporarily also recognizes the old `packages/llm/.env` location.

Run the backend and frontend in separate terminals:

```sh
npm run dev:server
npm run dev:web
```

Vite serves the UI and proxies `/api` to the Rust server at `127.0.0.1:3000`.

## Contracts and checks

Rust owns the browser API types. Regenerate TypeScript after changing a wire type:

```sh
npm run generate:types
```

Run all formatting, linting, tests, type checks, and the frontend production build with:

```sh
npm run check
```
