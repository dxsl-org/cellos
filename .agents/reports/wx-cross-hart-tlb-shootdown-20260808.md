# W^X Cross-Hart TLB Shootdown Evidence — 2026-08-08

## Implemented Contract

- RV64 probes SBI RFENCE before secondary startup, orders PTE writes with compiler fencing and `fence rw, rw`, then performs local `sfence.vma` and synchronous `remote_sfence_vma` for every online remote hart.
- Cell-segment unmap and `Drop` invalidate every owned VA before its frame or PIE slot can be reused. A post-mutation remote RFENCE error is fail-stop.
- A test-hooks-only probe primes a remote writable translation, checks that RFENCE makes the next write fault without changing the physical word, then proves the oracle with a compile-time-gated RFENCE bypass negative control.

## Compile Evidence

- PASS: RV64 test-hooks check with `RUSTFLAGS='-D warnings -C relocation-model=pic' cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc --features test-hooks`.
- PASS: normal RV64, AArch64, and x86_64 target checks; `cargo fmt --all -- --check`; `git diff --check`.

## Runtime Evidence

Command: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test wx-cross-hart-tlb -- --nocapture`.

Result: PASS, five QEMU boots in one test invocation. QEMU 8.2.2 selected both physical boot-hart variants across investigation runs; Cellos mapped the boot physical hart to logical 0, selected the other physical hart in HSM STOPPED state as logical 1, installed the shared kernel `satp`, then logged `trap-ready` before online publication. Every final iteration passed the positive RFENCE physical-byte oracle and its test-hooks-only negative control.

Image SHA-256: `b9833cd9a1902627ad8bde24430eaa42f10ea862c5e22388cb9582d7f4be4a1e`.

## Required Next Evidence

Real RV64 SMP hardware remains `HOST-GATED`; preserve the same identity/content oracle there. AArch64 and x86_64 remain separately runtime-gated and do not inherit the RV64 QEMU result.

## Reverification — 2026-08-09

- HEAD: `7ee86d5522c083341ebb2926d637274729744368`.
- Host command: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test wx-cross-hart-tlb -- --nocapture`.
- Guest command per iteration: `qemu-system-riscv64 -machine virt -m 256M -smp 2 -nographic -bios default -kernel target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks -monitor none -serial stdio`.
- Emulator/firmware: QEMU 8.2.2, OpenSBI v1.3, `Platform HART Count: 2`, and the firmware banner reported `Platform HSM Device: ---` while SBI HSM status reported the non-boot hart as STOPPED (`state = 1`).
- Direct-log image SHA-256: `b9833cd9a1902627ad8bde24430eaa42f10ea862c5e22388cb9582d7f4be4a1e`.

Five direct boots produced:

1. physical 0 -> logical 0 boot; physical 1 -> logical 1 STOPPED/trap-ready/online; oracle PASS.
2. physical 0 -> logical 0 boot; physical 1 -> logical 1 STOPPED/trap-ready/online; oracle PASS.
3. physical 1 -> logical 0 boot; physical 0 -> logical 1 STOPPED/trap-ready/online; oracle PASS.
4. physical 0 -> logical 0 boot; physical 1 -> logical 1 STOPPED/trap-ready/online; oracle PASS.
5. physical 0 -> logical 0 boot; physical 1 -> logical 1 STOPPED/trap-ready/online; oracle PASS.

Each PASS line explicitly included `RFENCE + physical oracle + negative control`; the mapped and translated frame were equal (`0x83bd0000`) in all five boots. The direct evidence command was bounded with a host timeout after the PASS marker because the kernel intentionally remains in its scheduler loop.

The delegated verifier then rebuilt the test-hooks image and ran the integration harness, whose test body performs five independent QEMU boots and asserts the same identity/oracle markers. It passed with test-hooks SHA-256 `fa2bd721dbbbb73dc3a85b0c3161815cb63e08933f600a347503fc0c8e685b09`. After `disk_v3.img` was regenerated to restore `/bin/wx-test`, the single-hart regression passed 2/2. The disk refresh changed the normal release image and is not the artifact used by the cross-hart proof.

Hardware availability audit found no configured SSH host aliases and no attached `/dev/ttyUSB*`, `/dev/ttyACM*`, or `/dev/ttyAMA*` device. Therefore real RV64 remains `HOST-GATED`; AArch64 remains `RUNTIME-GATED` because Cellos has no two-PE startup path for this oracle; x86_64 remains `RUNTIME-GATED` because Cellos has no SMP/LAPIC shootdown path. These lanes are not reported as PASS.

## Rollback

Revert physical/logical mapping, per-hart trap restore, secondary `satp`, RFENCE/IPI target translation, and CellSegments reuse ordering as one unit. Remove the test-hooks oracle separately. A failed remote RFENCE after a PTE mutation must remain fail-stop; reverting only one mapping/order component can silently target the wrong hart.
