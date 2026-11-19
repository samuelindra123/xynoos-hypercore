import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://website-xybc.vercel.app",
  base: process.env.XYNOOS_PAGES ? "/xynoos-hypercore/" : "/",
  trailingSlash: "always",
  output: "static",
  build: {
    format: "directory",
    inlineStylesheets: "never",
  },
});
