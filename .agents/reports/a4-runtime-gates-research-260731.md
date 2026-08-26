# A4 research — runtime gates left by phases 09 and 11

**Scope:** `.agents/260727-2101-midori-lessons-cellos`, read against the implementation
reports, commit history, CI recipes, and the preserved `wx-verify` worktree.

## Verdict

- **Phase 09 is only partly runtime-verified.** Commit `3afd524c` records one RV64 boot with
  a valid 23-entry policy and zero strip events. Still open are the promised three-architecture
  breadth, the three shell-launched demos, the full regression suite, and especially a real
  runtime negative case where a loaded policy omits a P-TRUST path.
- **Phase 11's report is stale.** Its two missing runtime artifacts now have evidence: commit
  `f8eb7525` says `gen_disk.ps1` completed, signed 39 cells through F1/F5, and produced the disk
  used by the W^X harness; that preserved disk subsequently passed `wx-text-write` 2/2 and the
  RV64 `boot` suite 54/54. During this research, `OBJCOPY=riscv64-unknown-elf-objcopy bash
  scripts/test-cell-signing.sh` also completed `ALL PASS` on the preserved real RV64
  `app-shell` ELF. A4 should update the record rather than rerun phase 11 unless independent
  reproduction is required.

## Phase 09 — `NoEntry` fail-closed

Plan: `.agents/260727-2101-midori-lessons-cellos/phase-09-noentry-fail-closed.md`  
Implementation: `3afd524c` (`feat(kernel): strip device-trust caps when a loaded policy omits a path`)

Files in the implementation commit:

- `kernel/src/audit.rs`
- `kernel/src/policy.rs`
- `kernel/src/task/cap.rs`
- `scripts/sign-policy.py`

### Gate status

Already evidenced:

- Host bake-negative test: deleting `/bin/nvme` makes `sign-policy.py` exit 1, name the path,
  and write no blob.
- Real RV64 standard boot: commit `3afd524c` records `policy loaded + verified (23 entries)`,
  shell reached, and zero `PolicyNoEntryStripped` events. This proves the current complete blob
  is behaviour-neutral on that lane.

Still open:

1. Boot the current complete policy on RV64, AArch64 and x86_64 to a shell without panic/fault.
2. From the shell, run `periph-demo`, `robot-demo`, and `sensor-demo`; expected evidence includes
   normal startup/output (`[periph-demo] ...`, `[robot-demo] ... done (5 cycles)`,
   `[sensor-demo] ...`) and no permission-denied/path-policy regression.
3. Run the RV64 integration suite and retain the pass count; the current gate-of-record is the
   54-test `boot` suite, serially.
4. Exercise the actual new branch at runtime: boot an intentionally test-only policy that is
   validly signed but omits one P-TRUST row (for example `/bin/nvme`), spawn that path, and show:
   the cell loses only its P-TRUST bit, ordinary caps survive, and audit event 26
   `PolicyNoEntryStripped` records the tid/mask. The current production bake guard deliberately
   prevents constructing this image through the normal script, so this needs a test fixture or
   controlled temporary mutation; a normal boot can only prove **zero false positives**.

### Commands / QEMU roles

RV64 build and full suite, from the preserved worktree or a fresh checkout:

```bash
export PATH=".../scratchpad/shim:$PATH"
export CC_riscv64gc_unknown_none_elf=riscv-none-elf-gcc
export AR_riscv64gc_unknown_none_elf=riscv-none-elf-ar
export OBJCOPY=riscv-none-elf-objcopy
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
pwsh ./gen_disk.ps1
cargo test --manifest-path tests/integration/Cargo.toml \
  --target x86_64-unknown-linux-gnu --test boot -- --test-threads=1
```

QEMU roles:

- `qemu-system-riscv64`: authoritative policy path and full suite; the complete disk contains
  `/POLICY.BIN` and all three demos.
- `qemu-system-aarch64`: architecture smoke via `BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh`;
  use the ARM image only if it contains the requested demos (the CI image currently guarantees
  `periph-demo`, not `robot-demo` and `sensor-demo`).
- `qemu-system-x86_64`: architecture smoke via
  `BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso`; peripheral demos may
  legitimately use their non-ARM/synthetic paths.

Success evidence should preserve command, commit, artifact hash/path, pass count, the three demo
markers, `policy loaded + verified`, absence of unexpected event 26 on a normal boot, and one
positive event-26 trace from the deliberately incomplete-policy test.

## Phase 11 — `cellos-sign` F1/F5 admission

Plan: `.agents/260727-2101-midori-lessons-cellos/phase-11-cellos-sign-f1.md`  
Implementation: `13d5c5f6` (`feat(build): gate cell signatures behind an F1/F5 admission check`)

The commit touches the signing package and policy (`scripts/cellos-sign`,
`scripts/cellos_sign/*.py`, `scripts/unsafe-allowlist.toml`, `scripts/sign-cell.py`,
`scripts/lib-sign-cells.sh`, tests), both CI workflows, `gen_disk.ps1`, `libs/ostd` entry support,
the shell safety rewrite, and the cell crate roots migrated to `#![forbid(unsafe_code)]`.
Use `git show --name-status 13d5c5f6` as the exact long file inventory.

### Original open gates and present evidence

1. **Real cross-ELF sign -> verify -> tamper rejection.** Command:

   ```bash
   OBJCOPY=riscv64-unknown-elf-objcopy bash scripts/test-cell-signing.sh
   ```

   This ran during this research on `wx-verify` at `4f11e6ae`: valid sign and verify passed,
   the PT_LOAD tamper was rejected, and the script ended `test-cell-signing: ALL PASS`.

2. **Image lane builds/signs and a signed image boots.** `f8eb7525` records a clean
   `gen_disk.ps1` run signing 39 cells via `cellos-sign`; the resulting preserved image was then
   used for the phase-10 W^X run (`wx-text-write` 2/2 and `boot` 54/54). The preserved
   `app-shell` also contains `__ViCell_sig` by `riscv64-unknown-elf-readelf -S`.

That closes the phase-11 report's stated missing evidence. A stricter future release gate would
rebuild the kernel with `--features signing-required` and add a negative unsigned-cell boot test,
but phase 11 explicitly left enabling production posture to the release checklist; it is not an
unmet phase-11 acceptance criterion.

## Recommended A4 disposition

- Mark **phase 11 runtime verified** with the evidence above.
- For **phase 09**, do not spend time merely repeating the already-recorded RV64 normal boot.
  Run the missing negative runtime case first, then the AArch64/x86_64 smoke and demo breadth.
  If A4 is scoped only to existing automated tests, record that no existing integration test can
  exercise `PolicyNoEntryStripped`; the critical branch remains runtime-unverified even if every
  current suite is green.
