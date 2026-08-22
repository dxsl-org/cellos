# Trusted-init hart-affinity differential

**Root Cause:** The exact shared-boot init NX-stack instruction fault is migration-dependent: it reproduced in the unpinned control and disappeared when only trusted init was constrained to logical hart 0.
**Confidence:** CONFIRMED (2 signals: reproducible two-image differential + fixed affinity/terminal marker correlation; migration-independent behavior for the exact signature eliminated)
**Next Step:** diagnose RV64 Context migration from the clean retirement baseline; do not ship affinity, destination-`tp` rebinding, or deferred requeue without an isolated causal proof.

## Executive Summary
- **Issue:** Fresh RV64 `-smp 2` shared boot faults trusted init at `sepc=stval=0x817cfd88`, an NX stack VA, after SMP retirement and atomic-publication gates pass.
- **Experiment:** Same signed cells, embedded init, kernel filesystem, RAM, QEMU topology, and marker gates. Variant difference was a temporary `test-hooks-init-hart0` feature that routed only trusted init tid 5 to logical hart 0, excluded only that tid from stealing, and asserted/logged every observed init selection on hart 0.
- **Verdict:** **migration-dependent for the exact `0x817cfd88` init fault.** Control reproduced it. Pinned init was selected on hart 0 and advanced through `Init: services spawned.` without that signature.
- **Important limit:** Pinning did **not** make the shared boot healthy. The pinned run instead faulted VFS task 8 (`scause=15`, then `scause=2`) and ended with `Init: WARN service registry mismatch.`; no registry-verified or VFS terminal-pass marker appeared. This differential eliminates migration-independent init-local corruption for the exact NX-stack signature, but does not identify or repair the broader cross-hart context corruption.
- **Status:** Diagnostic complete; temporary feature, affinity code, markers, and integration runner removed. No product fix or scheduler change retained.

## Exact commands

Fresh control image (rebuilds/signs the same eight cells and embeds the fresh init/kernel FS):

```sh
bash scripts/build-test-hooks-ci.sh
cp target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks \
  /tmp/cellos-kernel-test-hooks-control
```

Independent direct control reproduction:

```sh
timeout 90s qemu-system-riscv64 -machine virt -m 256M -smp 2 -nographic \
  -bios default -kernel /tmp/cellos-kernel-test-hooks-control \
  -monitor none -serial stdio
```

Pinned kernel build, reusing the exact already-built/signed embedded init and `kernel_fs.img` from the control build:

```sh
EMBEDDED_OVERRIDE=kernel/src/embedded-test-hooks \
RUSTFLAGS='-D warnings -C relocation-model=pic' \
cargo build --release --target riscv64gc-unknown-none-elf \
  -Z build-std=core,alloc --features test-hooks-init-hart0 -p cellos-kernel
cp target/riscv64gc-unknown-none-elf/release/cellos-kernel \
  /tmp/cellos-kernel-test-hooks-init-hart0
```

Temporary integration differential runner (booted both images with `QemuRunner::boot_rv64_smp(..., 2)`, asserted all AP-00..AP-15/shared gates, printed both complete serial captures, and emitted the verdict):

```sh
cargo test --manifest-path tests/integration/Cargo.toml \
  --target x86_64-unknown-linux-gnu \
  --test init-hart0-diagnostic -- \
  --exact compare_unpinned_and_init_hart0_shared_boots --nocapture
```

Result:

```text
cargo test: 1 passed (1 suite, 2.15s)
CAUSAL_VERDICT=migration-dependent
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Image identities used by the passing runner:

```text
74043e64c98c276ba57fc6d0f44f49dc9b8bc55746363f97f3158b2476510c52  /tmp/cellos-kernel-test-hooks-control
e0a9d9ff09d5914b11df3124bd909d7cc1b40c0ef8eb16b6c55a16bc29794b4e  /tmp/cellos-kernel-test-hooks-init-hart0
```

## Gate and marker comparison

Both complete captures contained:

```text
[ INFO] [smp] hart 1 online, parked
[ INFO] [selftest] SMP-RETIREMENT: stage=hart1-interrupts-enabled
[ INFO] [selftest] SMP-RETIREMENT: stage=worker-queued-hart1
[ INFO] [selftest] SMP-RETIREMENT: stage=worker-dispatch-ipi
[selftest] SMP-RETIREMENT: stage=selected-pre-executing-hold
[selftest] SMP-RETIREMENT: stage=selected-pre-executing-observed
[selftest] SMP-RETIREMENT: stage=forced-post-pick-ssip-deferred
[ INFO] [selftest] SMP-RETIREMENT: stage=root-retired-during-selection
[ INFO] [selftest] SMP-RETIREMENT: stage=remote-switch-requested hart=1 epoch=1
[ INFO] [selftest] SMP-RETIREMENT: stage=selected-switch-permitted
[ INFO] [selftest] SMP-RETIREMENT: stage=remote-switch-completed hart=1 epoch=1
[ INFO] [selftest] SMP-RETIREMENT: stage=worker-context-entered
[ INFO] [selftest] SMP-RETIREMENT: stage=worker-executing-observed
[ INFO] [selftest] SMP-RETIREMENT: stage=forced-post-pick-ssip-delivered-after-switch
[ INFO] [selftest] SMP-RETIREMENT: stage=remote-switch-requested hart=1 epoch=2
[ INFO] [selftest] SMP-RETIREMENT: stage=post-stack-pre-epoch-hold
[ INFO] [selftest] SMP-RETIREMENT: stage=completion-permitted
[ INFO] [selftest] SMP-RETIREMENT: stage=remote-switch-completed hart=1 epoch=2
[ INFO] [selftest] SMP-RETIREMENT: stage=completion-epoch-published
[ INFO] [selftest] SMP-RETIREMENT: stage=idle-attribution-cleared current=0 executing=0 selected=0 deferred=0 cell=0
[ INFO] [selftest] SMP-RETIREMENT: PASS (selected Context + zombie switch completion gate owner release + CellId reuse)
[ INFO] [selftest] VFS-LIFETIME: PASS (exact lease + quarantine + cell-owner death watch)
[ INFO] ATOMIC_PUBLICATION_AP-00: PASS
...
[ INFO] ATOMIC_PUBLICATION_AP-15: PASS
[ INFO] ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED
[ INFO] ATOMIC_PUBLICATION_ALL: PASS
```

The runner individually asserted every exact `ATOMIC_PUBLICATION_AP-00: PASS` through `AP-15: PASS`, not only the terminal marker. A few simultaneous retirement-stage lines were byte-interleaved by the two-hart UART, as shown in the complete runner capture, but the complete stage text remained observable and the terminal retirement PASS was exact in both runs.

### Unpinned control endpoint

```text
[ INFO] Successfully spawned init with complete root authority (tid=5)
USER: Init: Starting Cellos Orchestrator...
USER: VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)
USER: [config] Config Service v0.3 (typed IPC)
USER: Init: cell not found — skipping:
USER: /bin/input

[panic-in-cell 1] panicked at hal/arch/riscv/src/rv64/trap.rs:150:21:
Cellos: Kernel exception: scause=12 sepc=0x817cfd88 stval=0x817cfd88 sstatus=0x8000000200006100
[ERROR] [fault] Cell 1 (task 5 'init') terminated: cause=0x0 pc=0x0 addr=0x0
```

Absent from control:

```text
Init: services spawned.
Init: service registry verified.
[vfs-test] ALL TESTS PASSED
```

### Init-hart0 endpoint

Fixed diagnostic markers:

```text
[ INFO] [diagnostic] INIT-HART0: armed trusted init launch
[ INFO] [diagnostic] INIT-HART0: registered tid=5 target-hart=0
[ WARN] [diagnostic] INIT-HART0: first-selection tid=5 hart=0
```

Endpoint:

```text
USER: Init: Starting Cellos Orchestrator...
USER: VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)
USER: [vfs-file-handle] wrong-owner-read-close-preserves-entry PASS
USER: [vfs-file-handle] quota-32-per-owner PASS
USER: [vfs-file-handle] nonreuse-and-u64-exhaustion PASS
USER: [vfs-file-handle] exact-generation-purge PASS
[panic-in-cell 2] panicked at hal/arch/riscv/src/rv64/trap.rs:150:21:
Cellos: Kernel exception: scause=15 sepc=0x80208740 stval=0x58 sstatus=0x8000000200046100
[ERROR] [fault] Cell 2 (task 8 'vfs') terminated: cause=0x0 pc=
[panic-in-cell 2] panicked at hal/arch/riscv/src/rv64/trap.rs:150:21:
Cellos: Kernel exception: scause=2 sepc=0x806866e6 stval=0x0 sstatus=0x8000000200046100
[ERROR] [fault] Cell 2 (task 8 'vfs') terminated: cause=0x0 pc=0x0 addr=0x0
USER: Init: services spawned.
USER: Init: WARN service registry mismatch.
```

Absent from pinned run:

```text
Cellos: Kernel exception: scause=12 sepc=0x817cfd88 stval=0x817cfd88 sstatus=0x8000000200006100
Init: service registry verified.
[vfs-test] ALL TESTS PASSED
```

## Causal interpretation

1. Same shared-boot topology and every retirement/AP prerequisite pass in both images.
2. Control reaches init tid 5 and deterministically instruction-faults at the NX stack VA `0x817cfd88`.
3. The only scheduled-task differential pins trusted init tid 5 to logical hart 0; fixed registration and first-selection markers prove the mode was active.
4. Under that differential, init advances beyond the control endpoint to `Init: services spawned.` and never emits the exact `0x817cfd88` fault.
5. Therefore the exact init NX-stack signature depends on init being eligible for cross-hart migration/work stealing. A purely init-local, migration-independent corruption would have survived the pin and is eliminated for this signature.
6. Separate pinned-run VFS/context faults mean affinity changes the victim/timing rather than restoring correctness. Do not convert the diagnostic pin into a production workaround.

## Actionable next investigation

- Trace every RV64 task Context ownership transition with `(tid, source_hart, destination_hart, context_ptr, saved sp, saved sepc, selected/executing/completion epoch)` at publication, steal, selection, raw-switch entry, and incoming completion.
- Focus first on Normal-priority cross-hart steal/requeue after retirement selftest completion. The exact init signature is migration-dependent, while pinning moves corruption to VFS task 8.
- Require a healthy unpinned `-smp 2` run with both `Init: service registry verified.` and `[vfs-test] ALL TESTS PASSED` before accepting any production context fix.
- Do not weaken fault handling, VFS assertions, scheduler concurrency, or shared boot.

### Causal baseline disposition

The next diagnostic baseline retains only independently necessary retirement mechanisms: per-hart boot contexts; secondary-hart SIE delivery; selected and executing ownership pins; incoming-side completion epochs; task-to-idle attribution clearing; and RV64 pre-selection SIE masking with complete outgoing `sstatus` saved and restored after late `s11` restoration. The temporary affinity diagnostic is absent.

Destination-`tp` Context rebinding and delayed publication of an outgoing task through deferred requeue are excluded. The `tp` A/B recorded no mismatch before the exact fault, and neither mechanism is required by the retirement owner-lifetime contract. Either may return only as an isolated, falsifiable experiment with its own control/treatment evidence; neither is part of this causal baseline.

## Cleanup proof

Removed after interpretation:

- kernel feature `test-hooks-init-hart0`
- trusted-init arming/registration state and fixed markers
- init-specific ready-queue routing and steal exclusion
- selection-affinity assertion
- temporary `init-hart0-diagnostic` integration test and Cargo stanza

The report is the only retained diagnostic artifact under `.agents/debug/`; no diagnostic product-source behavior remains.
