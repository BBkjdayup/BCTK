import { defineConfig } from 'vite'
import { fileURLToPath, URL } from 'node:url'
import { readFileSync } from 'node:fs'
import vue from '@vitejs/plugin-vue'

const { version } = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))

export default defineConfig({
  plugins: [vue(), {
    name: 'desktop-release-version',
    transformIndexHtml: (html) => html.replaceAll('__DESKTOP_VERSION__', version),
  }],
  root: fileURLToPath(new URL('.', import.meta.url)),
  publicDir: fileURLToPath(new URL('./public', import.meta.url)),
  build: {
    outDir: 'dist', emptyOutDir: true, target: 'es2022',
    rollupOptions: { input: {
      main: fileURLToPath(new URL('./index.html', import.meta.url)),
    } },
  },
  server: {
    host: '127.0.0.1',
    port: 4178,
    strictPort: true,
    proxy: {
      '/updates/latest.json': { target: 'https://api.tktiku.cn', changeOrigin: true },
    },
  },
  preview: {
    host: '127.0.0.1', port: 4178, strictPort: true,
    headers: {
      'X-Content-Type-Options': 'nosniff', 'X-Frame-Options': 'DENY',
      'Referrer-Policy': 'no-referrer', 'Cache-Control': 'no-store',
      'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
    },
  },
})
