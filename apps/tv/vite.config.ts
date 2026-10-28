import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Run from refactor/ (`npm run tv:dev`). Tauri expects a fixed port and must not have the terminal
// cleared under it. 1421, next to the old launcher's 1420, so both can run while the new one is
// tried out.
export default defineConfig({
  root: 'apps/tv',
  plugins: [react()],
  clearScreen: false,
  build: { outDir: 'dist', emptyOutDir: true },
  server: {
    port: 1421,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
});
