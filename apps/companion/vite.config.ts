import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));

// The companion: the launcher's touch UI for the phones, tablets and old laptops at home. The home
// server serves the built files (crates/server, port 47960; `npm run companion:build` puts them in
// apps/companion/dist). `npm run companion:dev` serves them with hot reload on port 5175 instead
// and sends /api to a server running on this PC (`tvl-server run --dev`, port 47970). The proxy
// keeps the page's Host, which the server checks the page's Origin against.
export default defineConfig({
  root: here('.'),
  // JSX goes through src/compat/jsx-runtime.ts, which rewrites the inline styles old Safari
  // can't read.
  plugins: [react({ jsxImportSource: 'companion-jsx' })],
  resolve: {
    alias: {
      'companion-jsx': here('./src/compat'),
      // The TV app's code the companion shares (its api, the smart home's helpers, art).
      '@tv': here('../tv/src'),
    },
  },
  // The oldest iPad at home runs iOS 12.
  build: { target: ['safari12', 'ios12'], cssTarget: ['safari12', 'ios12'], outDir: here('./dist'), emptyOutDir: true },
  server: {
    host: true,
    port: 5175,
    strictPort: true,
    proxy: { '/api': { target: 'http://127.0.0.1:47970', ws: true } },
  },
});
