import { configDefaults, defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import Components from "unplugin-vue-components/vite";
import { ElementPlusResolver } from "unplugin-vue-components/resolvers";
import packageInfo from "./package.json";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    vue(),
    Components({
      dts: false,
      resolvers: [ElementPlusResolver({
        importStyle: process.env.VITEST ? false : "css",
        directives: true,
      })],
    }),
  ],
  define: {
    __APP_VERSION__: JSON.stringify(packageInfo.version),
  },
  test: {
    environment: "jsdom",
    clearMocks: true,
    // Source handoff verification creates complete repositories below `.codex-tmp`.
    // They must never be discovered as a second copy of this project's tests.
    exclude: [...configDefaults.exclude, "**/.codex-tmp/**"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
