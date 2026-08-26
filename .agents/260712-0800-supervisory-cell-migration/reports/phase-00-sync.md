## Phase 00 Sync

- `plan.md`: phase 00 is `complete`; phase 01 is `complete`.
- `phase-00-cap-ceiling-preservation.md`: checklist updated to mark the Phase 00 items complete, close the runtime blocker, and record the SpawnCap probe.

### Observed

- `git status --short` shows the implementation edits plus the docs updates (`docs/project-roadmap.md`, `docs/project-changelog.md`, `docs/system-architecture.md`) and the two untracked support files (`cells/services/supervisor/src/transfer.rs`, `cells/tests/bench/src/scenarios/hotswap_supervisor.rs`).
- `git diff` confirms `PauseService = 422`, `SpawnReplacement = 421`, allowlist bits 49/57, `sys_spawn_replacement`, the live-Frozen ceiling recheck, one-shot ceiling consumption, scheduler cleanup, supervisor cutover away from `sys_spawn_from_path`, and fail-closed disk artifact packaging.

### Pending

- Phase 01 message-queue drain is complete.
- Phase 02+ remain queued behind the later phase gates.
