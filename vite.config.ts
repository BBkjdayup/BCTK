import { configDefaults, defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import Components from "unplugin-vue-components/vite";
import { ElementPlusResolver } from "unplugin-vue-components/resolvers";
import packageInfo from "./package.json";
import type { Plugin } from "vite";

function rejectPreviewBackend(): Plugin {
  return {
    name: "desktop-excludes-preview-backend",
    generateBundle(_options, bundle) {
      for (const entry of Object.values(bundle)) {
        if (entry.type === "chunk" && Object.keys(entry.modules).some((id) => id.replaceAll("\\", "/").endsWith("/src/services/mockBackend.ts"))) {
          this.error("Desktop builds must not include the browser preview backend.");
        }
      }
    },
  };
}

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async ({ command, mode }) => ({
  plugins: [
    vue(),
    Components({
      dts: false,
      resolvers: [ElementPlusResolver({
        importStyle: process.env.VITEST ? false : "css",
        directives: true,
      })],
    }),
    ...(command === "build" && mode !== "browser" ? [rejectPreviewBackend()] : []),
  ],
  define: {
    __APP_VERSION__: JSON.stringify(packageInfo.version),
    __BROWSER_PREVIEW__: JSON.stringify(command === "serve" || mode === "browser"),
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
