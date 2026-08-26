# Phase 02 — `pkg` Shell Built-in (install / remove / list / info)

## Context Links
- Plan: [plan.md](plan.md) · Depends on [Phase 01](phase-01-writable-cell-store.md)
- Manifest flags → cap names: `libs/api/src/abi/manifest.rs:23-69`
- Signing gate: `kernel/src/signing.rs`, `kernel/src/loader.rs:114-127`
- Shell dispatch: `cells/tools/shell/src/executor.rs:703-789`; file utils pattern `cmd_fs.rs`

## Overview
- **Priority:** P1 · **Status:** pending
- Add the user-facing `pkg` command. It reads a package (signed ELF) from a source VFS path, performs
  **advisory install-time checks** (structural sanity + capability review), and persists it to `/bin`
  via the P01 gated write. Also `remove`, `list`, `info`.

## Key Insights (verified)
- A capability-bearing cell **must** live in `/bin` — `loader.rs:156` denies privileged non-`/bin` cells.
  So `pkg install` writing to `/bin` is not a convenience; it is the only way an app with caps can run.
- The kernel is the enforcement point: a bad-sig ELF that lands in `/bin` **cannot spawn**
  (`CellSignatureFailed=22`, `loader.rs:114-127`). Install-time checks are therefore **advisory** — they
  make failures early and legible, they do not replace the kernel gate.
- `CellManifest::from_bytes` (`manifest.rs:132`) is in `libs/api` → callable from the shell cell to parse
  `__ViCell_manifest` and render requested caps (`BLOCK_IO/NETWORK/SPAWN/GPIO/UART/HYPERVISOR/PART_*`).
- Package source `/mnt/sd` is writable FAT32 (`manager.rs:34-38`); `/tmp`, `/data` also readable.
- No dependency resolution (SAS cells are self-contained). No install scripts (capability model forbids).

## Requirements
**Functional**
- `pkg install <src-path> [name]` — read pkg → sanity + cap review → confirm → write `/bin/<name>`
  (default `name` = source basename). Refuse protected-core names (delegated to P01 gate) with a clear
  message.
- `pkg remove <name>` — `Unlink /bin/<name>` (+ `.prev` if present).
- `pkg list` — enumerate `/bin` (filter `.prev`), one line per cell, optional cap flags.
- `pkg info <name|path>` — parse manifest, print requested capabilities in human names + ELF size + sig
  presence.

**Non-functional:** `#![forbid(unsafe_code)]`; new file `cmd_pkg.rs` under 200 LOC; ~5 LOC dispatch arm.

## Architecture / Data Flow
```
pkg install /mnt/sd/hello        (source)
  1 read bytes  ── VfsRequest::GetFile / ReadFileGrant ──▶ vfs (/mnt/sd)
  2 sanity      ── ELF magic 0x7F454C46 · has __ViCell_sig · has __ViCell_manifest
  3 cap review  ── CellManifest::from_bytes(manifest) → flag names → print, prompt (interactive)
  4 write       ── P01 gated Write /bin/hello  (chunked or grant)
  5 done        ── "installed; run `hello`" ;  spawn still gated by kernel sig verify
```
`pkg info` = steps 1–3. `pkg list` = ListDir `/bin` + per-entry step 3. `pkg remove` = `Unlink`.

## Related Code Files
**Create:** `cells/tools/shell/src/cmd_pkg.rs` (install/remove/list/info; reuses `cmd_fs.rs` helpers
`read_file_vfs`, `write_file`/`vfs_write_chunked`, `vfs_req_ok`).
**Modify:**
- `cells/tools/shell/src/executor.rs:703` — add `"pkg" => crate::cmd_pkg::cmd_pkg(make_parts(args)),`.
- `cells/tools/shell/src/main.rs` — `mod cmd_pkg;`.
- `cells/tools/shell/src/commands.rs:5-16` — add `pkg` to `help`.
**Read/verify:** `libs/api/src/abi/manifest.rs` (flag→name mapping), `cmd_fs.rs` (VFS helpers).

## Implementation Steps
1. `mod cmd_pkg;` + dispatch arm + help line.
2. `cmd_pkg(args)` sub-command router: `install|remove|list|info` (+ usage on none/unknown).
3. `read_pkg(src) -> Vec<u8>` via existing VFS read helpers (grant for large files).
4. `sanity(bytes) -> Result<(), &str>`: ELF magic; presence of `__ViCell_sig` (64 B) and
   `__ViCell_manifest` sections (parse ELF section table in-cell, or reuse an ostd helper if present).
5. `cap_summary(manifest_bytes) -> String`: `CellManifest::from_bytes` → join set flag names; "none
   (unprivileged)" if `flags==0`.
6. `install`: sanity → print cap summary → (interactive confirm unless `-y`) → P01 write → report.
7. `remove`/`list`/`info` per data flow.

## Todo List
- [ ] `cmd_pkg.rs` scaffold + router + help/dispatch wiring
- [ ] `read_pkg` (VFS read, grant for large)
- [ ] `sanity` (magic + section presence)
- [ ] `cap_summary` (manifest → names)
- [ ] `install` (+ `-y` non-interactive for test harness)
- [ ] `remove` / `list` / `info`
- [ ] Integration test in hardened suite

## Success Criteria
- **Oracle (boot, QEMU riscv64):** `pkg install -y /mnt/sd/hello.cell && hello` prints hello output.
- `pkg info hello` lists the requested capabilities by name; unprivileged cell shows "none".
- `pkg list` shows installed cells, hides `.prev`.
- **Boundary demo:** install a **tampered** copy (flip a byte after signing) → `pkg install` succeeds
  (advisory) but `exec`/spawn is **rejected** with `CellSignatureFailed` in the audit log. (If review
  adds optional userspace verify, install rejects earlier — document which.)
- `pkg remove hello` → `hello` no longer found.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Users think install-time = enforcement | Med×Med | Docs + the tamper oracle explicitly shows spawn-gate is the boundary |
| Large ELF exceeds 512 B IPC frame | Med×Med | Grant-based read/write path (P01); chunked fallback |
| Installed name shadowed by VIFS1 bootstrap | Low×Med | Protected-core denylist (P01) + `pkg` warns on shadow collision |
| Interactive prompt hangs the test harness | Med×Low | `-y` non-interactive flag for CI oracles |

## Security Considerations
- `pkg` must never execute anything from the package at install time (no hooks/scripts) — it only reads,
  inspects, and writes bytes. Any behavior comes only from a later, sig-gated spawn.
- Show the capability manifest to the user **before** writing so an over-privileged package is visible.

## Rollback
Remove the dispatch arm / feature-gate `cmd_pkg`; delete `cmd_pkg.rs`. No persistent state beyond files
in the cell-store (inert without a valid sig).

## Next Steps
Unblocks P03 (`upgrade`/`rollback` extend `cmd_pkg.rs`) and P04 (HTTP source reuses `install`).
