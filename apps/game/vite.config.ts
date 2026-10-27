import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

const repoRoot = fileURLToPath(new URL('../..', import.meta.url));

export default defineConfig({
  plugins: [react()],
  worker: { format: 'es' },
  // The worker imports scenarios/*.yaml and the wasm package from outside the app folder.
  server: { fs: { allow: [repoRoot] } },
  build: { target: 'es2023' },
});
