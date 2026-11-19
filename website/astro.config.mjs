import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://samuelindra123.github.io",
  base: "/xynoos-hypercore/",
  trailingSlash: "always",
  output: "static",
  build: {
    format: "directory",
    inlineStylesheets: "never",
  },
});
