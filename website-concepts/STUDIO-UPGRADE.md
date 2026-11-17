# Studio / Night edition

This upgrades the existing Concept B, preserving its headline, editorial section order, vermilion identity, serif accents, architecture selector and factual research content.

Visual changes: graphite atmosphere with warm lighting and subtle grid/noise; a custom layered SVG instrument built from the original exploded X; translucent observation and safety panels; inspectable compact/expanded layers; event-driven pointer depth; restrained animated signal paths; richer architecture glyphs; visual research-note plates; sculptural technology stack; a roadmap trajectory; and a large outlined CTA emblem. No new runtime dependencies or GPU canvas engine.

Interactions: original architecture layers, compact/expand diagram, motion pause/resume, mobile menu and Escape handling. Device reduced-motion preference takes precedence. Pointer depth is disabled for touch and reduced motion. Reading progress is passive and frame-throttled; there is no permanent JavaScript rendering loop.

Original preserved before edits:

- `backups/concept-b-studio-original/` — exact file copy
- `backups/concept-b-studio-original.zip` — portable archive
- `backups/concept-b-studio-original.sha256.json` — file checksums
- `concept-b/backup-original/` — comparison copy served by the same preview

Active URL: https://humble-space-tribble-v6rp7jg554jw3xw7w-4102.app.github.dev/

Original comparison: https://humble-space-tribble-v6rp7jg554jw3xw7w-4102.app.github.dev/backup-original/

Both remain private Codespaces previews. No visibility change or production deployment.

Validation: HTML structure validation, JavaScript syntax validation, original DOM interaction suite and 29 additional assertions for Studio's compact/expand state, architectural visual state, manual motion controls and device reduced-motion behavior. Authenticated HTTP verification compares preview responses with local files; original backup checksums are verified. See `studio-upgrade-verification.json`.

Per the user's preference, no Playwright or rendered browser review was performed. CSS includes desktop, tablet and mobile compositions; actual visual appearance and browser layout still need human review. No claim of rendered viewport validation is made.

Repeat the focused interaction check:

```sh
NODE_PATH=/tmp/xynoos-qa/node_modules node checks/studio-upgrade.cjs
```

Restore, if needed: copy `index.html`, `styles.css`, `script.js`, and `assets/` from the preserved original into `concept-b/`. Preserve the backup folder while restoring.

## Illustrated system film

Added a new motion study after the design-principle introduction, with a hero shortcut to `#system-film`. It retains the Studio palette and editorial hierarchy.

Six scenes: overview, CPU die, RAM banks, conceptual GPU cutaway, policy reasoning and locked safety gate. Original inline SVG illustrates a layered motherboard, CPU pins and compute cores, RAM banks, GPU fans and parallel lanes. Camera transforms move into components; CPU and GPU covers reveal stylized internal structures. The GPU scene explicitly describes future context, not implemented GPU integration.

Playback is a finite sequence of six 4.8-second scenes. Play/pause, replay/restart and direct scene selection work without a framework. Selecting a scene pauses playback. Autoplay occurs only once, when visible and motion is allowed. Leaving the viewport, hiding the document, system reduced-motion preferences and Studio's global motion pause all stop playback. Screen-reader live announcements are suppressed during autoplay; manual scene selection announces the description.

This is conceptual illustration, not a screen recording of the Rust engine, a measurement, or a simulation of actual GPU policy. It never reads or modifies the host.

Source snapshot before the addition: `backups/studio-night-before-system-film/` contains the three previous night-edition source files; existing font assets remain unchanged. The original light Studio backup is also retained untouched.

Verification: 66 focused DOM assertions cover scene selection, automatic progression, finite completion, replay, pause, global motion stop, reduced motion and capability disclosures. Original interaction checks still pass. No browser rendering was used, following the user's preference.

```sh
NODE_PATH=/tmp/xynoos-qa/node_modules node checks/studio-film.cjs
```
