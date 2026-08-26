# Phase 04 — VirtIO Sound Driver Cell

## Context Links
- Plan: [plan.md](plan.md) · Prereq: [phase-00](phase-00-prerequisites.md)
- Source: `kernel/src/task/drivers/virtio_sound.rs` (118)
- Syscall today: `AudioPlay=218` (`libs/api/src/syscall.rs:192`)

## Overview
- **Priority:** P3 (lowest; parallel after Phase 00).
- **Status:** resolved — YAGNI delete (2026-06-24)
- **Risk:** LOW.
- **Description:** No consumer of `AudioPlay=218` exists in any Cell — grep confirms zero uses across `cells/`. Decision: **do NOT build a Cell for dead code**. `virtio_sound.rs` scheduled for deletion in Phase 08. `AudioPlay` syscall stub stays in ABI (returns 0 = no device) until a real audio Cell warrants it.

## Key Insights
- **DECISION GATE (Step 1):** grep for any consumer of `AudioPlay=218` / `virtio_sound`. If only test/demo code uses it (or nothing), this phase **collapses to a delete** in Phase 08 — do NOT build a Cell for dead code (YAGNI). If a real audio app/Cell consumes it, build the Cell.
- If migrated: same pattern as blk — claim MMIO/BAR, drive the VirtIO sound tx queue, serve an `AudioPlay`-equivalent IPC.

## Requirements
### Functional (only if a consumer exists)
1. `cells/drivers/virtio-sound/`: claim MMIO, init sound device, serve audio-play IPC (PCM buffer via grant).
2. Kernel `AudioPlay=218` forwards to the Cell when registered; kernel `virtio_sound.rs` retires (Phase 08).

### Non-Functional
- `#![forbid(unsafe_code)]` except MMIO/DMA island; Law 2 owned PCM buffers.

## Architecture
```
audio app → AudioPlay(grant PCM) [kernel] → forward IPC → sound Cell → tx virtqueue → speaker
```
(Or, if no consumer: this phase = "confirm dead, delete in P08", ~0.5 day.)

## Related Code Files
**Create (if migrating):** `cells/drivers/virtio-sound/` (Cargo.toml, build.rs, src/main.rs, src/dispatch.rs).
**Modify:**
- `kernel/src/task/syscall.rs` — `AudioPlay` forward when Cell registered.
- `kernel/src/task/drivers/driver_cell.rs` — `SOUND_DRIVER_CELL` AtomicUsize.
- `kernel/src/loader.rs` — `/bin/virtio-sound` cap grant.
- `cells/tools/init/src/main.rs` — spawn (optional cell; skip if not present).
- `gen_disk.ps1` + root Cargo.toml.

## Implementation Steps
1. **Grep consumers of AudioPlay/virtio_sound.** If none → mark for delete in Phase 08, close this phase.
2. (If consumer) Scaffold from nvme template.
3. Port `virtio_sound.rs` init + tx into the Cell.
4. Audio-play IPC: PCM via grant buffer (Law 2 owned).
5. Init claim + register `SOUND_DRIVER_CELL`.
6. Kernel `AudioPlay` forward.
7. init spawn (optional) + gen_disk + member.
8. Audio smoke test (if a test app exists).

## Todo List
- [ ] Grep AudioPlay/virtio_sound consumers → migrate-or-delete decision
- [ ] (if migrate) Scaffold cell
- [ ] Port init + tx
- [ ] PCM grant IPC
- [ ] Init register
- [ ] Kernel AudioPlay forward
- [ ] init spawn + gen_disk + member
- [ ] Audio smoke test

## Success Criteria
- [ ] If migrated: audio plays through the Cell; kernel `virtio_sound` static unused; disabling Cell falls back.
- [ ] If dead: documented as no-consumer; scheduled for delete in Phase 08; no Cell built.

## Risk Assessment
| Risk | L | I | Mitigation |
|------|---|---|-----------|
| Building a Cell for dead code | Med | Low | Step-1 consumer grep gates the whole phase (YAGNI) |
| No QEMU sound device in test harness | Med | Low | Treat as optional cell; init skips if device absent |

## Security Considerations
- Sound MMIO/DMA scoped to the sound device BDF; PCM buffers are grants, not raw pointers.

## Next Steps
- Phase 08 deletes `virtio_sound.rs` (either after Cell migration, or as confirmed dead code).
