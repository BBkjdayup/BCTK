import { defineConfig } from 'vite'
import { fileURLToPath, URL } from 'node:url'
import vue from '@vitejs/plugin-vue'

// The console is published at the root of its own origin.
export default defineConfig({
  plugins: [vue()],
  root: fileURLToPath(new URL('./admin', import.meta.url)),
  base: '/',
  publicDir: fileURLToPath(new URL('./public', import.meta.url)),
  build: {
    outDir: fileURLToPath(new URL('./dist-admin', import.meta.url)),
    emptyOutDir: true,
    target: 'es2022',
  },
  preview: {
    host: '127.0.0.1', port: 4178, strictPort: true,
    proxy: { '/admin-api/': { target: 'http://127.0.0.1:8080' } },
    headers: {
      'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff',
      'X-Frame-Options': 'DENY', 'Referrer-Policy': 'no-referrer',
      'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
    },
  },
})
