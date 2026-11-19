# Xynoos Studio

Studio is a static Astro project for <https://samuelindra123.github.io/xynoos-hypercore/>. The editorial site, architecture explorer, video rendering of the original illustrated hardware study, and installation and research links come from the original Studio. The [framework-free source](../legacy/github-pages/) is archived byte for byte.

## Local build and checks

From the repository root, using Node.js 24.11.1 and npm. The lockfile pins Astro’s compiler to 0.5.2 because the 0.5.3 native binding exits with an illegal instruction on the Pentium Silver N5000 used for local validation.

```sh
npm ci --prefix website
npm run check --prefix website
npm run build --prefix website
npm ci --prefix .github/website-checks
npm test --prefix .github/website-checks
```

Run `node .github/website-checks/preview.cjs` to serve the build under `/xynoos-hypercore/`. The static checks use the generated HTML, CSS, JavaScript, and project-path HTTP responses. The DOM tests cover navigation, the architecture explorer, film controls, reduced motion, and clipboard fallback.

Run `npm run dev --prefix website` for live reload at <http://localhost:4321/>. Development serves from the domain root for convenient local browsing. Production builds retain the `/xynoos-hypercore/` base required by GitHub Pages. Run `npm run preview --prefix website` to inspect the production build at <http://localhost:4321/xynoos-hypercore/>.

Astro bundles the styles and event-driven interaction script. Local WOFF2 fonts retain their SIL Open Font Licenses in `public/fonts/`. The original social artwork is served as a JPEG from `public/media/`. The motion study uses its original interactive SVG artwork and scene controls. Inline SVG diagrams scale with the layout, and the hardware study's animation runs only while its interactive playback is active.

## Combined Pages artifact

`repository/pages.py` copies `website/dist/` to the artifact root and adds verified retained history under `/apt/`. It reserves `/apt/` and `/legacy/` and rejects Studio symlinks. Archive signing, fingerprints, and package checksums remain in the distribution pipeline. The Pages workflow uses the pinned toolchain and exact Studio revision for public byte verification.

The project-path `robots.txt` points to the canonical sitemap. Domain-root crawler policy is controlled by the GitHub Pages host.

## Website routes

Studio remains the homepage. Its Explore menu links to HyperCore, Downloads,
Documentation, Developers, Research, Changelog, Roadmap and About. The secondary
pages share navigation, typography and editorial components. Their route registry
in `src/lib/site.ts` also feeds the sitemap. Studio's interaction script loads on
the homepage only.
