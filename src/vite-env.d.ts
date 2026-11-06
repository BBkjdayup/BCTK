/// <reference types="vite/client" />

declare const __APP_VERSION__: string;
declare const __BROWSER_PREVIEW__: boolean;

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
