# Review plan and proposed issues

These are proposed issues, grouped to avoid fragmenting the safety case. They are not claims of completed validation.

## PR organization

- **Repository presentation**: README, branding, roadmap and this backlog form one reviewable documentation commit. It depends on the current branch's MVP documentation; port it to a clean branch from main with links adjusted if reviewing separately.
- **PR #14: draft**: its accumulated control, MVP, benchmark and packaging work needs a refreshed description and scope review. Do not merge as a single unreviewed change. Preserve the branch and evidence.
- **Follow-up split**: extract coherent commits onto branches from main in dependency order: guarded control and safety evidence; bounded observe/simulation MVP; benchmark harness and evidence; cooperative owned-memory and laptop experiments. Some commits are interdependent and need careful replay and tests, not a blind cherry-pick. Keep physical control and experiments draft until their gates are met. Previously merged PRs #9–13 provide foundations. Presentation can be reviewed soon once its target branch and links are settled.

## Proposed issue: Establish a physical control validation safety case

**Problem:** Integrated physical kernel mutation remains unverified and disabled. Earlier disposable tests and fixtures do not prove the complete path. **Why it matters:** incorrect writes can affect workloads. **Evidence:** [mutation audit](reports/TASK-018E-MUTATION-SINK-AUDIT.md). **Risk:** unintended control changes or unsupported claims. **Next:** independently review boundaries and design a disposable, explicitly authorized protocol before considering admission. Labels: `safety`, `validation`.

## Proposed issue: Prove delegation and lifecycle cleanup assumptions

**Problem:** Exclusive delegation and cleanup across concurrent actors and failures require further evidence. **Why:** ownership is necessary for safe restoration. **Evidence:** [safety hardening](018e-2d-safety-hardening.md). **Risk:** races or unresolved resources. **Next:** document assumptions and test identity changes, interruptions and cleanup on representative systems. Labels: `safety`, `linux`.

## Proposed issue: Design durable physical recovery

**Problem:** Original controller state is held in memory and physical crash recovery is not implemented. **Why:** a crash can prevent restoration. **Evidence:** [MVP safety](MVP-SAFETY-RECOVERY.md). **Risk:** unresolved state after process or host failure. **Next:** specify durable, identity-bound recovery and independent failure tests before enabling writes. Labels: `safety`, `recovery`.

## Proposed issue: Qualify telemetry and recovery across hosts

**Problem:** Missing, stale or incompatible signals and incomplete cleanup evidence limit decisions. **Why:** policy must not mistake unknown for safe. **Evidence:** [telemetry reliability](reports/TASK-015A-TELEMETRY-RELIABILITY-REPORT.md). **Risk:** false confidence or unnecessary refusal. **Next:** build a host/permission matrix and verify conservative behavior and diagnostics. Labels: `telemetry`, `validation`.

## Proposed issue: Replicate representative workload measurements

**Problem:** Codespaces experiments and synthetic or owned workloads do not establish laptop or desktop benefit. **Why:** tradeoffs vary with workload and hardware. **Evidence:** [laptop results](reports/PHASE4-LAPTOP-RESULTS.md), [owned performance](reports/PHASE4-OWNED-PERFORMANCE.md). **Risk:** misleading generalization. **Next:** agree on representative workloads, controls, raw evidence and independent replication; report negative outcomes. Labels: `benchmark`, `research`.

## Proposed issue: Make packages and documentation reproducible

**Problem:** Multiple experimental handoffs and archives complicate provenance and onboarding. **Why:** reviewers need to reproduce an identified source build. **Evidence:** [quickstart](MVP-QUICKSTART.md) and package reports. **Risk:** stale instructions or mismatched binaries. **Next:** document supported targets, source provenance, checksums and release procedure; audit links after PR split. Labels: `documentation`, `packaging`.

## Proposed issue: Define product discovery after validation

**Problem:** Website, showcase and desktop task manager are ideas without a validated product scope. **Why:** presentation should follow demonstrated user value. **Evidence:** [roadmap](ROADMAP.md). **Risk:** implying capabilities that do not exist. **Next:** gather user needs and prototype only after a credible core and clear support boundaries exist. Labels: `product`, `future`.
