import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

// The admin app has no server (ADR-010): it reads and writes the repo through the File
// System Access API. Fonts come from the game's public folder (doc 10 §10).
const gamePublic = fileURLToPath(new URL('../game/public', import.meta.url));

export default defineConfig({
  plugins: [react()],
  publicDir: gamePublic,
  server: { port: 5174 },
  worker: { format: 'es' },
  build: { target: 'es2023', chunkSizeWarningLimit: 4000 },
});
