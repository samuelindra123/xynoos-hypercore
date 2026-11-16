# Xynoos Studio

Selected Concept B Studio, packaged as a framework-free project site for:

https://samuelindra123.github.io/xynoos-hypercore/

The night-edition editorial composition, self-hosted typography, layered SVG hero, pointer depth, architecture explorer, visual research notes and six-scene CPU/RAM/conceptual-GPU motion study are preserved. Only release metadata, repository evidence links, precise capability wording and containment of decorative viewport glow were adjusted. Original Concept B backups remain in the experimental worktree and are not published.

This website is an illustration of experimental Linux resource intelligence, not a stable product or host-monitoring interface. It does not execute the Rust engine, read hardware or modify the host. GPU integration is a future illustration. No desktop-wide benefit or production readiness is claimed; physical kernel mutation is disabled for the illustrated control path.

## Local verification

From the repository root, using Node.js 24 or later:

```sh
npm ci --prefix .github/website-checks
npm test --prefix .github/website-checks
node .github/website-checks/preview.cjs
```

Open http://localhost:4174/xynoos-hypercore/. The preview deliberately requires the project prefix, so broken root-relative references cannot silently pass.

Optional rendered checks:

```sh
.github/website-checks/node_modules/.bin/playwright install --with-deps chromium
npm run browser --prefix .github/website-checks
```

Checks cover HTML structure, CSS parsing, JavaScript syntax, asset bytes over HTTP, fragment targets, SEO, repository evidence links, architecture layers, navigation, film controls, finite playback and reduced motion. Browser checks cover 375, 768 and 1440 px, loaded fonts, overflow, JavaScript/resource errors and real controls. Screenshots are written to ignored `.github/website-checks/artifacts/`.

## Deployment

`.github/workflows/studio-pages.yml` validates website-only PRs. A push to `main` uploads only `website/` as a Pages artifact and deploys through the `github-pages` environment. PRs never deploy. Action versions are pinned; validation has only `contents: read`, while the deployment job has `pages: write` and `id-token: write`.

One-time repository setting: **Settings → Pages → Build and deployment → Source: GitHub Actions**. If a token cannot enable Pages (HTTP 403), a repository administrator must select this setting, then run **Actions → Xynoos Studio Pages → Run workflow → main**, or re-run the failed deployment. A workflow file alone does not enable Pages.

No custom domain, server, database, authentication, external runtime API or frontend build is required. Fonts are local and include their SIL Open Font Licenses. The repository's GPL-3.0-only license applies to project source. The official branding in `assets/branding/` is preserved unchanged.

`robots.txt` is included at the project path; GitHub Pages project sites cannot control the domain-root `/robots.txt`. The canonical URL and sitemap use the actual project URL.
