import { defineConfig } from 'vite'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  publicDir: fileURLToPath(new URL('./public', import.meta.url)),
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022' },
  server: {
    host: '127.0.0.1',
    port: 4178,
    strictPort: true,
    proxy: { '/updates/latest.json': { target: 'https://api.tktiku.cn', changeOrigin: true } },
  },
  preview: { host: '127.0.0.1', port: 4178, strictPort: true },
})
