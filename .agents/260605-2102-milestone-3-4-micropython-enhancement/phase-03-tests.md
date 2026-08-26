# Phase 03 — Integration Tests

**Priority:** P1 · **Status:** pending · **Effort:** ~1h · **Depends:** Phase 02

## Context Links
- Boot/run harness: project `run.ps1` / QEMU (CLAUDE.md "Quick Command Reference")
- Comparable Lua VFS tests (mirror naming/placement): search `cells/runtimes/lua` test artifacts
- Test strategy spec: `docs/specs/10-testing.md`

## Overview
Validate the migrated MicroPython VFS end-to-end in a booted ViCell image. Tests run as
`python -c "..."` commands from the shell against the live VFS service cell.

## Key Insights
- The cell parks after `-c` (main.rs:88) — output is captured from serial before/at park.
- VFS RamFS is the target store (not VIFS1); `/tmp` and `/bin` exist at boot per prior milestones.
- "Verify functionally, not by file existence" — build + boot + run, do not claim done on compile alone.

## Requirements
**Functional test matrix**

| # | Command | Expected | Exercises |
|---|---------|----------|-----------|
| T1 | `python -c "import vfs; vfs.write('/tmp/t.txt','hi'); print(vfs.read('/tmp/t.txt'))"` | `hi` | write + read round-trip |
| T2 | `python -c "import vfs; print(vfs.stat('/bin'))"` | `(0, True)` (size,is_dir) | stat dir |
| T3 | `python -c "import vfs; print(vfs.listdir('/bin'))"` | list incl. `f:python`/`f:lua` etc. | listdir parse |
| T4 | write `/tmp/test.py` then `python /tmp/test.py` | script output | script-mode path (main.rs:92) |
| T5 | `python -c "import vfs; vfs.write('/tmp/r.txt','x'); print(vfs.remove('/tmp/r.txt')); print(vfs.read('/tmp/r.txt'))"` | `True` then `None` | remove (Unlink) |
| T6 | `python -c "import vfs; vfs.write('/tmp/a.txt','ab'); vfs.append('/tmp/a.txt','cd'); print(vfs.read('/tmp/a.txt'))"` | `abcd` | append |

**Non-functional**
- Each test boots to shell, runs one command, confirms serial output. No mocks, no stubbed VFS.

## Implementation Steps
1. Build full image (`cargo build --release` + image packer / `run.ps1`).
2. Boot QEMU; at shell run T1–T6 in sequence (or scripted via shell input).
3. Capture serial; assert each expected output substring.
4. For T4: `python -c "import vfs; vfs.write('/tmp/test.py','print(1+1)')"` then `python /tmp/test.py` → `2`.
5. Record results; on failure capture the failing VfsResponse (add a temporary debug print in bridge if needed, remove after).

## Todo
- [ ] Build + boot image
- [ ] T1 write/read round-trip → `hi`
- [ ] T2 stat → `(0, True)`
- [ ] T3 listdir → non-empty list
- [ ] T4 script mode → `2`
- [ ] T5 remove → `True` then `None`
- [ ] T6 append → `abcd`

## Success Criteria
- All 6 tests pass against a live booted image.
- No raw-opcode regressions: `vfs.read` returns content written by `vfs.write` (proves typed IPC both ways).
- Existing vnet Python tests still pass (no collateral damage from removed `ViCell_net_*` externs in modvfs.c).

## Risk Assessment
- **R1 — stat size for dir (LOW×LOW):** VFS may report dir size != 0. Mitigation: assert `is_dir==True`,
  treat size as informational if it differs from 0.
- **R2 — listdir truncation >30 entries (LOW×MED):** 512B reply cap. Mitigation: test `/bin` (small); document cap.
- **R3 — append after chunk boundary (LOW×MED):** only exercised with >400B in T6-extended. Mitigation: add a
  >400B write case if time permits to validate Write+Append chunk seam.

## Security Considerations
- Tests write only under `/tmp`; no privileged paths. Remove test files (`vfs.remove`) to keep image clean.

## Next Steps
- On green: update `docs/project-changelog.md` (Milestone 3.4 complete) and `docs/development-roadmap.md`.
- Delegate to `code-reviewer` per primary workflow before merge.
