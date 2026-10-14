# 2026-10-12 — Configured measurement launcher

## What happened
Reconciled existing init/boot-config/launch-profile sources and specified P01 measurement launcher without writing implementation. Updated phase01, overview and readiness; added measurement-launcher-contract.md.

## Decisions
- Supersede proposed new opt-in fixture feature with existing measurement-only autoload.toml, structured args and reviewed init→bench→bench-probe edges. Current normal boot choices are configuration-owned.
- Keep shell authority/policy unchanged and denied-launch logs intact. No ABI, feature or lock release.
- One scenario per isolated image; restart=never,150second host observation watchdog, exact leg counters/termination/provenance. Private config is proposed, not booted.
- Existing probes pinCPU0; online twoCPU and maximum-of-eight latency cannot certify aggregate SMP quota or production p99. Separate resource and P08 placement evidence remains necessary.

## Lessons
Default cargo test inherited bare-metal target and failed E0463. Explicit host target corrected configuration without source changes: cargo test -p cellos-boot-config --lib --locked --offline --target x86_64-unknown-linux-gnu —18passed. Parser suite and TOML syntax validation do not prove proposed runtime configuration.

## Verification
26documents/295local links/no missing paths; acyclic10pending phases;38retained reciprocal locks/adoptionrows;50ACowners;3negative controls rejected. Proposed TOML syntax/selection validated; workload/runtime NOT_RUN under configured route.

## Next steps
Package isolated config image and exercise existing init-owned launch before resource qualification. Exact resource sampling/counts/attribution and approved ABI checkpoint proposals remain pending. No commit.
