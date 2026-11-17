# Xynoos HyperCore — three website explorations

Independent, buildless HTML/CSS/JavaScript concepts. No runtime framework, analytics, external font service, host telemetry or command execution. Each folder can be served on its own. All fonts and visual assets are local.

## Open the running previews

| Concept | Direction | Codespaces private preview |
| --- | --- | --- |
| A / Command | Midnight and cyan; custom contour X; simulated CLI explorer | https://humble-space-tribble-v6rp7jg554jw3xw7w-4101.app.github.dev/ |
| B / Studio | Studio night edition: graphite, warm vermilion, luminous layered plate; preserved editorial structure | https://humble-space-tribble-v6rp7jg554jw3xw7w-4102.app.github.dev/ |
| C / Aura Systems | Indigo engineering lab; interactive topology; pressure and stale-data simulation | https://humble-space-tribble-v6rp7jg554jw3xw7w-4103.app.github.dev/ |

These URLs were returned by `gh codespace ports`, not constructed as assumed deployment URLs. They are temporary and private: sign in as the Codespaces owner. Port visibility has not been changed. Authenticated HTTP requests to the returned URLs verified their HTML and referenced assets against local files; see `http-verification.json`.

The current session runs detached Python static servers on 4101–4103. After a Codespace restart, from this directory run:

```sh
python3 serve.py
```

If those ports are already in use, retain the existing servers. Local addresses are `http://localhost:4101/`, `http://localhost:4102/`, and `http://localhost:4103/`. In Codespaces, open **Ports**, choose **Forward a Port**, add each port, leave visibility **Private**, then choose **Open in Browser**. If forwarding is lost, repeat those steps. Public visibility requires operator approval.

## Files and behavior

Each `concept-*` directory contains `index.html`, `styles.css`, `script.js`, and `assets/`. Assets include the original repository `logo.svg`, an X favicon adapted from that identity, self-hosted Manrope and Space Grotesk fonts, and their SIL Open Font Licenses. Original programmatic diagrams are inline SVG and CSS so the illustrations stay sharp and responsive.

- A: Observe / Simulate / Recovery command selector with replay. All terminal output is illustrative, not captured engine output. Motion is limited to transforms, opacity and a small SVG instrument.
- B: Studio night edition preserves the original editorial typography, serif accent, vermilion identity and section order, now with graphite glass surfaces, an interactive exploded plate, subtle pointer depth, motion controls, visual research notes and richer architectural glyphs. The original light Studio is preserved at `concept-b/backup-original/` and in `backups/concept-b-studio-original.zip`.
- C: Memory/CPU sliders, three scenario presets, stale-telemetry toggle, reset, selectable topology nodes and five distinct recommendation states. The thresholds are a pedagogical browser model, not a reproduction of Rust policy logic or real Linux PSI percentages.

All sites include keyboard-operable native controls, skip links, semantic landmarks, expanded-state navigation, visible focus, reduced-motion styles and mobile/tablet breakpoints. Content remains visible without JavaScript; interactions require it.

## Content evidence

Claims follow the checkout's README and roadmap at commit `7094074`. Evidence links are pinned to that commit. No desktop-wide improvement is claimed; physical mutation is disabled in the MVP, active ZRAM tuning is not implemented, and production readiness is explicitly denied. Owned-memory findings are scoped experiments with tradeoffs.

Design references studied for product storytelling and interaction patterns, without copying layouts or artwork: [Linear](https://linear.app/), [Langflow](https://www.langflow.org/), [Vercel](https://vercel.com/), [Hermes](https://hermes-agent.nousresearch.com/).

## Verification and limits

- JavaScript syntax: `node --check` passes for all three scripts.
- HTML: `html-validate:standard` passes for all three documents. Heading-level/style-policy checks are excluded; semantic HTML checks are retained.
- 65 DOM assertions pass using jsdom: navigation, internal anchors, labels, terminal modes/replay, architecture layers, scenario recommendations, slider readouts, stale input refusal, topology selection, reset and reduced-motion stylesheet presence.
- HTTP: local and authenticated Codespaces HTML/CSS/JS/favicon/font requests return 200 and byte-match their source files. Internal fragment links resolve; duplicate IDs are absent.
- Source-level refinement: larger touch targets, clearer lab annotations, footer wrapping, mobile architecture detail sizing and corrected control semantics.
- Per the user's later instruction, no Playwright or browser rendering was used. Desktop/mobile visual appearance, actual browser layout overflow, focus appearance and motion smoothness await human review. Responsive CSS is provided for 375, 768 and 1440 px; these are not claimed as rendered viewport tests.

The original engine worktree and its branch remain unchanged. Work is isolated at `/workspaces/xynoos-web-experiments`, on `website/three-concepts`. Nothing has been merged, pushed or deployed to production.

To repeat the non-browser interaction checks with jsdom installed outside the source tree:

```sh
npm install --prefix /tmp/xynoos-qa jsdom html-validate
NODE_PATH=/tmp/xynoos-qa/node_modules node checks/interactions.cjs
/tmp/xynoos-qa/node_modules/.bin/html-validate --config checks/htmlvalidate.json concept-{a,b,c}/index.html
python3 checks/verify_http.py
```

The HTTP check requires GitHub CLI authentication for the current private Codespace. It reads the token into memory and never prints or writes it.
