# Phase 07 — Integration Tests

## Context Links
- `tests/integration/tests/boot.rs` — existing QEMU boot + serial-assertion harness
- All of Phases 1-6.

## Overview
- **Priority:** P2
- **Status:** pending
- **Description:** Add QEMU-boot integration scenarios that drive the shell over serial
  and assert on output, covering every new/fixed feature. These verify the FINAL merged
  behavior — no mocks (per project rule: real boot, real VFS).

## Key Insights
- Existing harness is boot-based serial assertion in `boot.rs`. Follow its exact pattern
  (re-read it before adding) — driving input may be via a scripted command sequence or a
  test cell. Match whatever mechanism the current tests use; do not invent a new driver.
- Tests must boot the real kernel + real VFS image. Some features (cp large file) need
  test fixture files seeded into the disk image — check how the image is built
  (mkfat32.py / disk_v3.img per project memory) and whether tests can pre-seed files.
- Phase 7 is the gate: it should fail before Phases 1-6 land and pass after.

## Test Matrix
| # | Scenario | Command | Assert | Covers |
|---|----------|---------|--------|--------|
| 1 | pipe_builtin_chain | `ls /data \| grep test \| wc -l` | output is a number | P1+P2 |
| 2 | redirect_non_echo | `ls /bin > /tmp/ls_out.txt; vcat /tmp/ls_out.txt` | file contains ls output | P1 |
| 3 | redirect_append | `echo a > /tmp/r; echo b >> /tmp/r; vcat /tmp/r` | `a\nb` | P1 |
| 4 | tab_complete_command | send `l` then TAB | line/echo contains `ls` | P3 |
| 5 | script_with_pipe | `source /data/test.sh` (script: `echo hi \| wc -l`) | `1` | P1+P2 |
| 6 | find_files | `find /data` | lists files recursively | P4 |
| 7 | uniq_dedup | `cat /data/dup.txt \| uniq` | adjacent dups collapsed | P4 |
| 8 | cp_file | `cp /data/a.txt /data/b.txt; vcat /data/b.txt` | == a.txt content | P5 |
| 9 | mv_file | `mv /data/b.txt /data/c.txt; vcat /data/c.txt` | moved; b gone | P5 |
| 10 | kill_task | `<spawn bg cell> &; kill <tid>` then `ps` | tid absent | P6 |
| 11 | top_smoke | `top` then any key | shows PID/STATE/NAME header, exits | P6 |

## Data Flow (per test)
```
build image (seed fixtures: test.sh, dup.txt, a.txt) -> boot QEMU
  -> drive command(s) over serial/scripted input
  -> capture serial output -> assert substring/number/absence
  -> shutdown
```

## Related Code Files
- MODIFY: `tests/integration/tests/boot.rs` (add scenarios)
- CREATE (fixtures): seed `test.sh`, `dup.txt`, `a.txt` into the disk image build
  (e.g. via mkfat32.py input dir) — verify the image-build entrypoint first.

## Implementation Steps
1. Re-read `boot.rs` to learn the exact input-drive + assertion API.
2. Identify how to seed fixture files into the test disk image.
3. Add fixtures: `/data/test.sh`, `/data/dup.txt`, `/data/a.txt`.
4. Add scenarios 1-11 following the existing pattern.
5. Run the integration suite; iterate until green.

## Todo
- [ ] Re-read boot.rs harness pattern
- [ ] Seed fixtures (test.sh, dup.txt, a.txt) into disk image
- [ ] Scenario 1-3 (pipe + redirect + append)
- [ ] Scenario 4 (tab complete)
- [ ] Scenario 5 (script with pipe)
- [ ] Scenario 6-7 (find, uniq)
- [ ] Scenario 8-9 (cp, mv)
- [ ] Scenario 10-11 (kill, top smoke)
- [ ] Full suite green

## Success Criteria
- All 11 scenarios pass under QEMU boot.
- Suite fails if Phase 1 is reverted (proves it tests real behavior, not tautology).
- No mocks/fakes: every assertion is on real kernel+VFS serial output.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| Driving interactive TAB over serial is hard | M×M | If harness can't inject raw `0x09`, test completion via a unit test of `complete_builtin` instead, and keep scenario 4 as a best-effort serial test. |
| Fixture seeding not supported by image build | M×H | Step 2 discovery; if unsupported, create fixtures at runtime via `vwrite` in a test-setup command sequence. |
| Flaky timing (top refresh, bg spawn) | M×M | Assert on stable substrings (headers, names), add generous serial-read timeouts. |
| kill scenario needs a known cooperative target cell | M×M | Use an existing cell that handles shutdown msg (verify one exists in Phase 6 discovery); else add a tiny test cell. |

## Security Considerations
- Tests run in QEMU sandbox; no host filesystem writes beyond the disk image artifact.

## Next Steps
- On green: milestone 3.1+3.2 complete. Update `docs/development-roadmap.md` and
  `docs/project-changelog.md` (docs-manager).

## Unresolved Questions
- Exact serial input-injection API in boot.rs (resolve in step 1).
- Whether disk image build supports pre-seeded fixture files (resolve in step 2).
