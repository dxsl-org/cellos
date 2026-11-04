# Diagnosis — `x86_64-boot` 7/9: the two Tier-2 production-refusal tests

**Verdict:** the test expectation is stale, and the kernel behaves as intended. Since `b5b52e6f4` (2026-10-09), a production-feature x86_64 kernel opens Tier-2 admission on purpose. The two tests still assert the pre-2026-10-09 posture ("refused until the switch ordering is qualified"). A second, newer layer of drift hides this. Since the Cellbox cutover (`5043e2cf5`/`e053cdca6`, 2026-10-29), the witness image has had no `/bin/input`, so it never reaches `Cellos >`. The phase-02/03/04a/05 evidence only ever saw that outer layer.
**Confidence:** CONFIRMED (3 signals: reproduction at HEAD, a layer-by-layer repro with the outer layer patched, and commit archaeology).

## Failing tests

| Test | File:line |
|---|---|
| `x86_tier2_admission_is_refused_until_switch_is_qualified` | `tests/integration/tests/x86_64-boot.rs:279` |
| `x86_tier2_fault_isolation_fixture_is_refused` | `tests/integration/tests/x86_64-boot.rs:378` |

Both boot `build/vicell-x86-prod-refusal.iso`, which `scripts/build-x86_64-prod-refusal-ci.sh` builds. That script is not a CI lane: no `.github/workflows/*` step runs `--test x86_64-boot`. The CI job `qemu-x86_64-boot` runs `scripts/qemu-x86_64-test.sh` instead. Only local runs see this failure.

## Reproduction (private worktree at `28dee8b3d`)

1. `assemble-x86-image.py --profile headless` → kernel (`KF=policy-required`) → `make-iso-ci.sh` → `build-x86_64-prod-refusal-ci.sh` → `cargo test --test x86_64-boot -- --test-threads=1`.
2. Result: `7 passed; 2 failed`. The 7 boot/shell tests pass on the default ISO.
3. Repro-only step: the witness script was patched in the worktree to add `service-input` and `/bin/input` and to pass `--config-features input` to `mkfat32.py`. The witness was rebuilt and both tests were run again. Neither patch was committed.

## Findings

[CAUSE] `kernel/src/loader/domain_admission.rs:154-158` — `switch_ordering_qualified()` is `true` for every x86_64 build. A production kernel now prints ENABLED and admits domain-class cells, so the premise of both tests no longer holds.
[EVIDENCE] [EXPANDED] Witness boot (layer-2 run): `[INFO] Tier 2 admission: ENABLED (development profile)` → test panics at `x86_64-boot.rs:293` "the refusal witness must be a production-feature x86_64 image". The test expects the line `Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate)`, which comes from `kernel/src/main.rs:1290`.
`tier2-exploit` (unsigned, UNTRUSTED) is admitted, not refused: `[domain] admitted cell 'tier2-exploit' to Tier 2 Paged Domain (CR3 isolation)` → `[#PF user] va=0x0 … no VMA covers this address` → `[fault] Cell 3 … terminated` → `Cellos >`. The test waits for `path=/bin/tier2-exploit error=NotSupported` and panics at `:398` after 30 s.
Introduced by `b5b52e6f4` (2026-10-09), "feat(x86): close production Tier-2 switch-ordering gate" (`git log -S'|| cfg!(target_arch = "x86_64")'`). The tests come from `07b9f22a4` (2026-09-28) and were never updated.
[FIX] Retire the switch-ordering-refusal premise in `x86_64-boot.rs:256-445`. See "Proposed fix" below.

[CAUSE] `scripts/build-x86_64-prod-refusal-ci.sh:71-72,79-109,173` — the witness image does not build, package or start `/bin/input`. Cellbox's serial session is created through the INPUT service, so the shell exits before printing a prompt.
[EVIDENCE] Witness boot at HEAD: `USER: shell: authenticated serial session failed: NotFound` → `Syscall::Exit: task 6 exited with code 125`. Both tests panic at the first wait (`:285` / `:384`) with `shell prompt: timeout: pattern "Cellos >" not seen in 45s`. `inspect_fat.py` on the witness `kernel_fs.img` lists no `/bin/input`. The message comes from `cells/tools/cellbox/src/main.rs:216`, via `executor::start_serial_session` → `TerminalSession::create_serial` (`libs/terminal/src/session.rs:72`).
Introduced by `5043e2cf5`/`e053cdca6` (2026-10-29, Cellbox replaces the shell). The sibling scripts were fixed: `a88658288` (x86 domain-test) and `69a3c6fc3` (aarch64 prod-refusal, `--config-features input`). The x86 prod-refusal script was not touched after `e053cdca6`.
[FIX] Mirror `69a3c6fc3` in `build-x86_64-prod-refusal-ci.sh`: add `-p service-input`, `"$REL/service-input"`, `/bin/input` and `BASE_CELL_COUNT=11`, pass `--config-features input --config-output-dir "$TH_DIR/boot-config"`, and assert that `services.toml` starts `/bin/input`. With this patch, the repro reaches `Cellos >`.

[CAUSE] `tests/integration/tests/x86_64-boot.rs:306,339,418` — the test drives and markers are pre-Cellbox: `tier2-smoke &` and `[loader] SpawnFromPath refused: caller=`.
[EVIDENCE] `cells/tools/cellbox/src/executor.rs:4282-4284` refuses `cmd &` ("shell: SDK background launch requires explicit nonterminal streams") before the loader is asked. Shell foreground launches are refused by `kernel/src/task/cell_session.rs:836` as `[loader] PrepareChild refused: …`, not by `SpawnFromPath`. `69a3c6fc3` made exactly these two edits in `aarch64-boot.rs` (foreground `tier2-smoke`, `PrepareChild refused`). [INFERENCE] This layer was not exercised on x86: finding 1 fails first.
[FIX] Launch in the foreground (`tier2-smoke`). Assert `[loader] PrepareChild refused: caller=` wherever a refusal is still expected.

[CAUSE] `scripts/build-x86_64-prod-refusal-ci.sh:231` — the "production posture" guard greps the kernel binary for the DISABLED string. That string is always compiled in (`main.rs:1290` is a runtime branch), so the guard passes on a kernel that boots ENABLED.
[EVIDENCE] The witness build exits `0` (`==> Done`), but the same kernel's boot log prints `Tier 2 admission: ENABLED (development profile)`.
[FIX] Drop the guard or replace it with a boot-time check. This is the recurrence gap that let finding 1 hide behind a green build step.

### Separate HEAD regression, found on the way (not one of the 2 failures)

[CAUSE] `scripts/unsafe-allowlist.toml` — `24d75559a` (phase 05) added `driver-virtio-{blk,net,scsi}` to `HEADLESS_CELLS` and new `unsafe` files, with no allowlist entries.
[EVIDENCE] At clean HEAD, `assemble-x86-image.py --profile headless` fails F1: 8× `[token] … contains the \`unsafe\` keyword and is not in unsafe-allowlist.toml` (`virtio-blk/src/{hal_x86,io_x86,mmio}.rs`, `virtio-net/src/{mmio,pci_x86/hal,pci_x86/raw_net}.rs`, `virtio-scsi/src/{config,dma}.rs`), then `REFUSED: not signing`. The default ISO cannot be assembled from HEAD. This repro added the entries in the private worktree only. [INFERENCE] The CI `qemu-x86_64-boot` job fails at its assemble step on the next push. The main tree's uncommitted allowlist diff adds only `mlibc-posix-x86` entries.
[FIX] Add 8 `[[file]]` `driver-mmio` entries plus a `[[crate]] driver-virtio-scsi` entry. The file is currently dirty in the main tree (another session's R3 edits), so coordinate with that owner.

## Eliminated hypotheses

- **Clocksource/phase-02 regression:** eliminated. The witness timer line `clock=hpet clockevent=LapicPeriodic` matches the passing lanes, and the 7 default-ISO tests pass.
- **Wrong ISO booted / missing fixture:** eliminated. `/bin/tier2-smoke` and `/bin/tier2-exploit` are in the witness VIFS1, and the loader loads `tier2-exploit` (`ELF LOAD …`, `[wx] 'tier2-exploit'`).
- **Only Cellbox auth drift (the phase-02 evidence `[INFERENCE]`):** incomplete. Once auth is fixed, both tests still fail on the posture (finding 1).

## Proposed fix (minimal)

1. `tests/integration/tests/x86_64-boot.rs`: the switch-ordering-refusal premise is dead on x86_64. The two tests should witness the posture production actually has:
   - **Refusal row:** move it to the fleet profile. `main.rs:1308-1320` prints `Tier 2 admission: DISABLED (fleet profile)` under `policy-required`, which the shipping CI kernel carries. Boot a witness built with `--features policy-required`, then assert that `DISABLED (fleet profile)` appears and that a foreground `tier2-smoke` gives `[loader] PrepareChild refused: … path=/bin/tier2-smoke error=<PolicyDisabled mapping>`. [INFERENCE] The commit message of `07b9f22a4` gives that mapping as `PermissionDenied`; confirm it from one boot.
   - **Fault row:** rename it to `x86_tier2_fault_isolation_fixture_is_contained`. Assert `[domain] admitted cell 'tier2-exploit'`, `[fault] Cell … terminated … addr=0x0`, a returned `Cellos >`, and the echo. The layer-2 repro already prints all four.
2. `scripts/build-x86_64-prod-refusal-ci.sh`:
   - add the input fix (finding 2);
   - add `--features policy-required` to the kernel build, unless (1) chooses another refusal profile;
   - replace the vacuous guard at line 231 (finding 4);
   - update the header comment at lines 17-19.
3. `.agents/261103-2218-x86-platform-breadth/reports/phase-02-evidence.md:23-25`: correct the `[INFERENCE]` so it cites this report.

## Ownership

- **`tests/integration/tests/x86_64-boot.rs`:** in the S1 file set of plan `261103-2218` (`cp1-pcie-irq-power-abi.md:135`). S1 is closed, and the Intel WIP lane is held by RE (`261104-0802`). Its H-4 marks the x86 test harness S1-held, so the next 261103-2218 slice handoff (S2) must name this file.
- **`scripts/build-x86_64-prod-refusal-ci.sh`:** not in any S1 row. It came from `07b9f22a4` (the production-refusal witness work). It needs an explicit row in the same handoff.
- **`scripts/unsafe-allowlist.toml`:** the virtio entries are owed by plan `261103-2218` phase 05 (`24d75559a`). The file is concurrently dirty under the RE slice R3, so land the change through Main as integrating owner.
- **Kernel:** no change. `b5b52e6f4` is the intended, CHANGELOG-recorded posture.

## Unresolved questions

- Which refusal profile should the x86 witness pin: `policy-required` (fleet), or the browser-engine profile's DISABLED branch (`main.rs:1303`)? This is the owner's call. Fleet matches the shipping kernel.
- The exact error the fleet-profile refusal prints at `PrepareChild` was not observed this session.
- The evidence for phases 02-05 recorded "pre-existing 2/9" with the wrong cause. Should those rows be amended or only cross-referenced?
