# Phase 03 — Fix spi-demo test wiring + docs + real-board note

## Context Links
- Plan: [plan.md](plan.md) · Depends on: Phase 02 (`SPI loopback RX OK` probe)
- Test: `tests/integration/tests/periph-i2c-spi.rs`
- Init: `cells/tools/init/src/main.rs`
- Spec: `docs/specs/13-peripherals.md` §3, §9

## Overview
- **Priority:** P2 · **Status:** pending · **Effort:** S (~50 LOC)
- Fix the broken assumption that the SPI integration test passes today, extend it to assert the new RX-path probe, and reconcile docs with the enhanced (Mode 0-3 + loopback) SPI.

## Key Insights (the wiring bug)
- `periph-i2c-spi.rs:77` claims "spi-demo is spawned best-effort by init after all supervised services" and `wait_for("[spi-demo] SPI TX OK", 60)`.
- **Reality:** `init` NEVER spawns `spi-demo`. `cells/tools/init/src/main.rs:216` is a *comment* listing on-demand shell demos; `grep` confirms no `sys_spawn_from_path("/bin/spi-demo")` anywhere. Init's stated philosophy (`main.rs:213-215`): "demos should not pollute boot output" → run from shell.
- Therefore `aarch64_spi_demo_tx` would **time out (fail)** whenever prerequisites are present; it currently only "passes" via the skip guard (`ci_guard`, `periph-i2c-spi.rs:63`) when `disk_arm_virt.img`/QEMU are absent. The test is green by absence, not by success.
- Fix must honor the on-demand philosophy. Use the proven shell-drive barrier pattern (memory: `test-harness-wait-for-race`): `wait_for("ViCell >")` → `send_line("spi-demo")` → `wait_for("[spi-demo] SPI ... OK")`. Do NOT rely on auto-spawn.

## Requirements
- **Functional:** integration test drives `spi-demo` from the shell and asserts BOTH `[spi-demo] SPI TX OK` (Phase 01) and `[spi-demo] SPI loopback RX OK` (Phase 02); no reliance on init auto-spawn. Test still skips gracefully (green) when prerequisites absent.
- **Non-functional:** no change to init boot output philosophy; docs reflect Mode 0-3 + loopback.

## Architecture / Data flow (test)
```
QemuRunner::boot_aarch64_with_disk(kernel, disk)
   → wait_for("ViCell >", BOOT_TIMEOUT)      // shell ready barrier
   → send_line("spi-demo")                    // on-demand launch
   → wait_for("[spi-demo] SPI TX OK")         // Phase 01 TX path
   → wait_for("[spi-demo] SPI loopback RX OK") // Phase 02 RX path
```

## Related Code Files
- **Modify** `tests/integration/tests/periph-i2c-spi.rs` — replace the auto-spawn assumption with shell-drive; add the loopback RX assertion. Apply the same shell-drive fix to `aarch64_i2c_sensor_demo_banner` if it shares the stale assumption (verify against init at implementation time).
- **Modify** `docs/specs/13-peripherals.md` — §3 line 71: `ViSpi` "Mode 0" → "Mode 0-3"; §9 item 8: note software loopback added + RX path now covered; header status line SPI note.
- **Modify** `docs/project-changelog.md` + `docs/project-roadmap.md` — SPI Mode 0-3 + loopback entry (via docs-manager if preferred).
- **Note only (no code):** Hypha P4 tool-peripheral could later expose an `spi.transfer` verb over the `ViSpi` rlib — record the seam in the spec "See Also"/future section; do not implement.

## Implementation Steps
1. Confirm at implementation time whether any test-image init variant auto-spawns demos (grep `gen_disk`/init features); if none, proceed with shell-drive (expected).
2. Rewrite `aarch64_spi_demo_tx`: boot → `wait_for("ViCell >")` → `send_line("spi-demo")` → assert TX probe → assert loopback RX probe. Consider renaming to `aarch64_spi_demo_tx_and_loopback` or add a second `#[test]` sharing a boot helper.
3. Verify `spi-demo` is embedded (`format-disk-arm.ps1:38,93`) so `/bin/spi-demo` exists for the shell — already true; assert in the test's prerequisite note.
4. Update spec §3/§9 + changelog/roadmap.
5. Add the Hypha P4 SPI-verb hook note.

## Todo List
- [ ] Test drives spi-demo from shell (no auto-spawn dependency)
- [ ] Test asserts TX probe AND loopback RX probe
- [ ] Graceful skip preserved when prerequisites absent
- [ ] spec §3 line 71 Mode 0 → Mode 0-3
- [ ] spec §9 item 8 loopback + RX coverage noted
- [ ] changelog + roadmap updated
- [ ] Hypha P4 SPI-verb hook noted (no code)

## Success Criteria
- **Done =** `cargo test -p vicell-integration-tests periph_i2c_spi` (or harness equivalent) with disk+QEMU present: shell launches `spi-demo`, both TX and loopback RX probes observed; without prerequisites, test skips green.
- **Test oracle:** the integration test itself — now it actually exercises the demo instead of skip-passing.

## Risk Assessment
- **R1 (Med) — shell not ready / prompt string drift.** `wait_for("ViCell >")` depends on the exact prompt (memory: prompt is `ViCell >`). *Mitigation:* reuse the established prompt-barrier constant from other integration tests; if the shell prompt differs on aarch64, capture it first.
- **R2 (Low) — send_line race (memory: `test-harness-wait-for-race`).** `wait_for` is whole-buffer contains with no cursor; a `wait_for` immediately after `send_line` can be a no-op barrier. *Mitigation:* assert on the demo's own output lines (`SPI TX OK`), which appear only after the command runs — a real barrier, not an echo.
- **R3 (Low) — i2c sensor-demo test shares the same stale auto-spawn bug.** *Mitigation:* fix it in the same pass if confirmed; otherwise leave a TODO — out of SPI scope but cheap.

## Security Considerations
None — test/docs only.

## Rollback
Test + docs only; revert file edits. No runtime/ABI impact.

## Next Steps / Open Questions
- **Q:** Should `spi-demo` be auto-spawned in a dedicated CI test image (so tests need no shell interaction), matching how `silo-test`/`vfs-test` are spawned in the CI region (`init/src/main.rs:~209`)? **Recommend NOT** for demos (pollutes console, contradicts init philosophy); shell-drive is the right fit. Flag for user if they prefer CI-image auto-spawn.
- Real-board follow-up (separate task): validate `BitBangSpi` Mode 0-3 against a physical MCP3008/BME280 on an SBC with a real SPI slave — the only way to catch physical edge-timing bugs the software loopback cannot. Out of scope here.
</content>
