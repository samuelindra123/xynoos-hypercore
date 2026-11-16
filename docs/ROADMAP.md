# Roadmap

This is a research roadmap, not a delivery schedule. Progress depends on evidence and review.

## Phase 1 — Core foundations

Established Rust workspace, transactional and memory research, tests, and recorded component reports. These foundations do not establish production readiness.

## Phase 2 — Safety and telemetry

Read-only Linux telemetry, qualified observations and fail-closed control research exist. Continue examining freshness, unavailable data, delegation, cleanup and recovery across hosts.

## Phase 3 — Experimental policy and MVP

Observation, simulation and bounded cooperative owned-data experiments exist. Keep physical admission locked while the lifecycle and safety case remain incomplete.

## Phase 4 — Physical validation and honest measurement

First establish exclusive ownership, durable recovery and safe cleanup, then independently review a narrowly scoped validation protocol. Measure representative workloads with preserved raw evidence and report regressions. Physical mutation and desktop-wide gains remain unverified; no rollout is implied.

## Phase 5 — Productization

If evidence supports it, develop packaging, documentation and a beta with explicit support boundaries. A website, showcase and desktop task manager are future possibilities, contingent on a useful validated core.

## Phase 6 — Advanced optimization research

Consider further optimization only where measurements identify a real bottleneck and a bounded intervention has a credible safety and evaluation plan.
