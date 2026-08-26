# Phase 03 — `pkg upgrade` + `.prev` Rollback

## Context Links
- Plan: [plan.md](plan.md) · Depends on [Phase 02](phase-02-pkg-installer-builtin.md)
- Supervisor seam: `.agents/260712-0800-supervisory-cell-migration/plan.md`; `service::SUPERVISOR=11`
- SAS spawn semantics: `libs/ostd/src/syscall.rs:263-323` (spawn reads `/bin` fresh each time)

## Overview
- **Priority:** P2 · **Status:** pending
- Replace an installed cell with a new version, keeping one backup (`.prev`) for cheap rollback.
  Deliberately does **not** live-migrate running instances — that is the Supervisory Cell's job; this
  phase documents the seam.

## Key Insights (verified)
- **No RENAME op** (`libs/api/src/services/ipc.rs` — no `Rename`). Upgrade cannot be an atomic swap. Use
  copy + overwrite; accept non-atomicity because spawn-time sig verify rejects a torn binary
  (`loader.rs:114-127`, fail-closed) and `.prev` enables recovery.
- **Running-instance semantics (SAS):** overwriting `/bin/<name>` does **not** affect an already-running
  instance — its code was copied into memory at spawn. The new code applies on the **next** spawn. So
  `pkg upgrade` needs no cell-stop in v1; it just changes what future spawns load.
- Live hot-swap of a running instance to new code is orchestrated by the **Supervisory Cell** (hotswap),
  not the package manager. Kernel keeps only freeze/resume/kill mechanism (spec 15 §3.2).

## Requirements
**Functional**
- `pkg upgrade <name> <src> [-y]` — sanity + cap review of `<src>` → copy current `/bin/<name>` to
  `/bin/<name>.prev` (overwriting any older `.prev`) → write new bytes to `/bin/<name>`.
- `pkg rollback <name>` — copy `/bin/<name>.prev` back to `/bin/<name>` (refuse if no `.prev`).
- `pkg list` continues to hide `.prev`.
- **Doc only:** `pkg upgrade --hotswap <name>` is recognized and prints "hot-swap handoff to supervisor
  not implemented in v1; restart the cell to apply" — the seam, not the implementation.

**Non-functional:** `#![forbid(unsafe_code)]`; extends `cmd_pkg.rs` (keep under 200 LOC — split a
`pkg_upgrade.rs` sibling if it grows).

## Architecture / Data Flow
```
pkg upgrade foo /mnt/sd/foo-v2
  1 read + sanity + cap-review new bytes           (P02 helpers)
  2 backup:  read /bin/foo  ── write /bin/foo.prev  (copy; overwrites old .prev)
  3 apply:   write /bin/foo  (new bytes)            (P01 gated write; overwrite)
  4 report;  running instances unaffected until respawn

pkg rollback foo
  1 read /bin/foo.prev  (err if absent)
  2 write /bin/foo  (= .prev bytes)
```
Ordering rationale: back up **before** overwriting; the destructive step (3) is a full write of complete
new bytes — a crash between 2 and 3 leaves `foo` intact + a `.prev`; a crash during 3 leaves a torn
`foo` that won't spawn, recoverable via `.prev`.

## Related Code Files
**Modify:** `cells/tools/shell/src/cmd_pkg.rs` — add `upgrade`, `rollback` sub-commands; `list` filter
already excludes `.prev` (verify). `commands.rs` help line.
**Create (optional):** `cells/tools/shell/src/pkg_upgrade.rs` if `cmd_pkg.rs` exceeds 200 LOC.
**Read/verify:** P01 write + `Unlink`; `cmd_fs.rs` read helpers.

## Implementation Steps
1. `copy_cell(src_path, dst_path)` helper: read via VFS → write via P01 gated write (grant for large).
2. `upgrade`: P02 sanity/cap-review on new src → `copy_cell("/bin/<name>", "/bin/<name>.prev")` →
   write new bytes to `/bin/<name>`.
3. `rollback`: verify `.prev` exists (`Stat`) → `copy_cell(".prev", name)`.
4. `--hotswap` recognizer that prints the seam message (no supervisor call in v1).
5. Ensure `pkg list` filters `*.prev`.

## Todo List
- [ ] `copy_cell` helper
- [ ] `upgrade` (backup → apply)
- [ ] `rollback` (restore from `.prev`)
- [ ] `--hotswap` seam message
- [ ] `list` filters `.prev`
- [ ] Integration test: install v1 → upgrade v2 → rollback v1

## Success Criteria
- **Oracle (boot, QEMU riscv64):** build two versions of a probe cell that print distinct strings.
  `pkg install -y /mnt/sd/probe-v1` → `probe` prints v1. `pkg upgrade -y probe /mnt/sd/probe-v2` →
  `probe` prints v2 (fresh spawn). `pkg rollback probe` → `probe` prints v1 again.
- `pkg rollback` with no `.prev` prints a clear error, changes nothing.
- `pkg list` does not show `probe.prev`.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Non-atomic upgrade (no rename) leaves torn binary | Med×Med | Backup-before-overwrite; spawn-gate rejects torn binary; `.prev` recovery |
| User expects running instance to update live | Med×Low | Explicit message + docs: restart to apply; hot-swap = supervisor (seam) |
| `.prev` doubles cell-store usage | Low×Low | One `.prev` per cell only; overwrite old; `remove` also deletes `.prev` |
| Cap set widened silently on upgrade | Low×Med | `upgrade` re-runs cap review + prompt; kernel policy `∩` still applies at spawn |

## Security Considerations
- `upgrade` re-displays the new package's requested capabilities (they may differ from the installed
  version) and re-prompts — an upgrade must not silently escalate privilege. Enforcement remains the
  kernel spawn-gate + `/POLICY.BIN` intersection (`loader.rs:262-269`).
- Rollback restoring a `.prev` re-subjects it to the spawn-gate; a `.prev` whose fleet key was rotated
  out will fail to spawn (acceptable, fail-closed).

## Next Steps
Seam handoff to `.agents/260712-0800-supervisory-cell-migration/` for live hot-swap (future). Feeds P04
(HTTP `pkg install`/`upgrade` from URL reuse `copy_cell` + install path).
