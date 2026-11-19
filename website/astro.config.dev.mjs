import { defineConfig } from "astro/config";
import productionConfig from "./astro.config.mjs";

export default defineConfig({
  ...productionConfig,
  base: "/",
});
