# 2026-08-07 — Supervisor replacement cap ceiling

## What happened
Phase 00 added the Law-1-approved `SpawnReplacement(421)` path and committed it as
`817e9cea`. The code is complete, but refreshed-image QEMU evidence remains open.

## Decisions
- Use allowlist bit 57, separate from supervisor-operation bit 49, so older allowlists do not gain a new spawn surface.
- Publish and consume a one-shot frozen-task ceiling under `SCHEDULER -> SWAP_CEILINGS`; resume and every scheduler exit clear it.
- Intersect the frozen ceiling with the exact supervisor launch profile before manifest and operator policy checks.
- Keep legacy `HotSwap(400)` until later migration phases prove the Supervisor Cell path.

## Lessons
- A ceiling record is authority: it must remain atomically tied to a live `TaskState::Frozen`, not just a TID.
- Compile and three-architecture builds do not replace QEMU proof of the new userspace syscall path.
- The existing Supervisor Cell freezes before requesting snapshot state, so stateful migration currently degrades to cold replacement.

## Next steps
- Repair the pre-existing `gen_disk` blockers (`tetris.c`, missing `app-init`) and run refreshed-image `SpawnReplacement` E2E.
- Fix snapshot-before-hard-freeze ordering before claiming state-preserving Supervisor Cell hotswap.
- Keep Phase 01 blocked until the Phase 00 runtime criterion passes.
