# Diagnosis: KVM domain lane stops at the `X86-VMM-SMOKE` step

Date 2026-11-04. Base HEAD `28dee8b3d`, private worktree (since removed). Host: i7-14700, WSL2 kernel 6.18, `kvm_intel nested=Y`, QEMU 8.2.2.

## Verdict

**Cellos bug, not a host limitation.** Three defects in the Intel VMX vCPU (`hal/arch/x86/src/x86_64/vmx_vcpu.rs`). It is not a hang: the guest **triple-faults about 1 s after boot**, and `-no-reboot` turns that into a clean QEMU exit (code 0). The lane accepts exit 0, so the only visible symptom is a log that ends at `PKU-SELFTEST`.

The defects became reachable in `15830f4fd` (2026-10-29). That commit dropped the "VMX not attempted under a hypervisor" gate, so KVM `-cpu host` (which exposes VMX) now runs the VMX smoke. TCG `qemu64` has no VMX, so the TCG lane uses SVM and never reaches this code.

## Path

- `kernel/src/main.rs:718-741`: the BSP runs VMXON. KVM log: `x86 virt: root operation active (VMX)`.
- `kernel/src/main.rs:1497-1502` (`test-hooks`): `has_x86_virt()` is true, so `svm_registry::x86_smoke()` runs.
- `kernel/src/hypervisor/svm_registry.rs:585-647`: create the VM and vCPU, then up to 10 runs. The next line expected is `X86-VMM-SMOKE: PASS|FAIL`.
- `svm_registry.rs:399-436` `run_vcpu_hal` (cli, run, restore IF) → `VmxVcpu::run` (`vmx_vcpu.rs:329`) → `vmx_vmrun` (`world_switch.rs:146-250`).
- TCG lane: `X86-VMM-SMOKE: FAIL got_port=false got_hlt=false last_exit=Preempted` (SVM). The lane still passes because it does not assert this marker.

## Findings (severity order)

[CAUSE] vmx_vcpu.rs:151-159: `rdmsr IA32_GS_BASE` keeps only EAX (`out("eax") host_gs_base`, `out("edx") _`), so `HOST_GS_BASE` is truncated to 32 bits.
[EVIDENCE] [EXPANDED] Monitor state at the triple fault (`-action reboot=shutdown,shutdown=pause`): `GS base=0000000080b4b180`. The real `CPU_LOCALS` is at `ffffffff80b4b180` (`nm`), and `RAX=0000000080b4b1a0` is `CPU_LOCALS+0x20` through the bad base. `RIP=ffffffff800016a5` is `fxsave64` in `x86_64_idt_common`. `RSP=ffffffff80b4cf50` is inside `__boot_stack_guard` (`ffffffff80b4c000`). So every handler entry faulted again on a GS-relative access until the boot stack ran into its guard page.
[FIX] Combine EDX:EAX (or use `msr::read`) in `VmxVcpu::init_pvh`, :151-159. Experiment A (this fix only): `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)`, and boot continues to the `tier2-exploit` NULL store.

[CAUSE] vmx_vcpu.rs:100-148: `HOST_TR_BASE` (0x6C0A) is never written, so every VM exit loads TR.base=0 (limit 0x67).
[EVIDENCE] [EXPANDED] Same dump: `TR =0028 0000000000000000 00000067 … TSS64-busy` and `CR2=000000000000002c`. Offset `0x2C` is TSS.IST2, and `IST_DF = 2` (`ist.rs:35`). The final #DF could not load its IST stack, and that is the triple fault. Experiment A alone still died at the first ring-3 fault: the log ends at `[tier2-exploit] deliberately writing to NULL`, QEMU exits early, and the lane reports `saw 0` contained faults, because delivery from ring 3 reads TSS.RSP0 at base 0.
[FIX] Decode the TSS base from the GDT descriptor at `gdtr.base + (tr & !7)` (16-byte system descriptor) and `vmwrite(HOST_TR_BASE, …)` in `init_pvh`. Experiment B (A plus this): the smoke PASSes and `[fault] Cell 5 … cause=0x6 … addr=0x0` is contained. QEMU now runs the full window.

[CAUSE] vmx_vcpu.rs:199-203 + :355-357: `EXIT_ACKNOWLEDGE_INTERRUPT` is set, but an `EXTERNAL_INTERRUPT` exit returns `Preempted` without reading `VM_EXIT_INTR_INFO` or re-dispatching the vector. The acknowledged vector stays in the LAPIC ISR and is never EOI'd.
[EVIDENCE] Experiment B, sampled during the stall (3 samples, 2 s apart): `ISR 240`, `IRR 32`, `PPR 0xf0`, CPU in `HLT` in `kmain`, IF=1. The IPI vector 0xF0 (`apic.rs:22`) is stuck in service and blocks the timer (vec 32) and everything else below 0xF0. The log ends at `USER: shell: starting`, and the lane fails with `'Cellos >' prompt never appeared`.
[FIX] On `EXIT_REASON_EXTERNAL_INTERRUPT`, read `VM_EXIT_INTR_INFO` and run the host handler for that vector (KVM-style `handle_external_interrupt_irqoff`). Alternative: drop the ack and give the host an interrupt window before re-entry (next finding). Experiment C (A+B, ack removed): **lane PASS, every marker, `pcid_usable=true`**, but `X86-VMM-SMOKE: FAIL … last_exit=Preempted`.

[CAUSE] svm_registry.rs:615-634 + :409-436: the smoke runs in `kmain` with IF=0 [INFERENCE: experiment C and TCG both FAIL with `Preempted`]. A pending host interrupt makes every entry exit at once, and `run_vcpu_hal` never opens an interrupt window between runs. All 10 iterations are spent on `Preempted`.
[EVIDENCE] TCG/SVM at HEAD: `X86-VMM-SMOKE: FAIL got_port=false got_hlt=false last_exit=Preempted`. KVM/VMX in experiment C: the same line. The guest blob (8 bytes, `OUT` + `HLT`) never ran.
[FIX] After a `Preempted` exit, let the host service the IRQ before re-entry (`sti; nop; cli`, or the re-dispatch above), in `run_vcpu_hal` or the smoke loop. This applies to both backends; the SVM half is the TCG FAIL.

## Eliminated hypotheses

| Hypothesis | Evidence against |
|---|---|
| Nested VMX unavailable (host limitation) | `nested=Y`. VMXON succeeds (`FEATURE_CONTROL=0x100005 VMX_BASIC_revid=0x11e57ed0`, `root operation active (VMX)`). With A applied, VMLAUNCH/VMRESUME decode `PortOut` + `Hlt` correctly. |
| Genuine guest spin or hang | `-d cpu_reset` run: QEMU exit 0 after **1.04 s**, the log ends at `PKU-SELFTEST`. The pause-on-shutdown dump shows a triple fault, not a loop. |
| CET-IBT / shadow stack at the `vmx_vmrun` landing pad (no ENDBR) | KVM log: `CET-IBT: unavailable`. |
| KVM-only regression elsewhere in the boot (x2APIC, kvmclock, PCID) | Same image, KVM `-cpu host,-vmx`: smoke `SKIP`, **lane PASS** (`PASS: … every marker present`, shell ready). |

Noise, not related: two manual QEMU runs launched with `-display none` or `-gdb` stalled in SeaBIOS (VGA ROM, then SMM at `0xef860`) before the kernel loaded. Runs with the lane's own flags reproduced the original failure every time.

## Timeline

- `15830f4fd` (2026-10-29): `kmain` attempts VMX root operation even when a hypervisor is present (`main.rs` diff removes `Some(Vmx) if !under_hypervisor` and the "not attempted (cap closed)" arm). The same commit adds `vmx_vcpu.rs` with the three defects. Before it, KVM printed `X86-VMM-SMOKE: SKIP` (code reading, not run).
- `64ad34027` (2026-11-02) is a descendant. The phase-03 evidence saw the same stall there (PRIOR, consistent).
- The stale comment at `main.rs:704-708` ("VMX is never attempted under a hypervisor") no longer matches the code.

## Fix and owner

1. **Code fix (recommended):** the three `init_pvh`/`run` changes above in `hal/arch/x86/src/x86_64/vmx_vcpu.rs`, plus an interrupt window in `kernel/src/hypervisor/svm_registry.rs` (`run_vcpu_hal`, or the `x86_smoke` loop). No file lock in `261103-2218` covers these files; they belong to the Tier-3 x86 VMM (VT-x backend). `261104-0802` phase 05 also lists the VMX/SVM entry/exit paths (XCR0 swap), so sequence the work with that plan. The lane is held by RE, so queue it as its own small slice once the Intel WIP lane is released. Experiments A/B/C cover about 15 lines in total.
2. **Interim guardrail, no code:** run the KVM lane with `X86_CPU_MODEL_HOST=host,-vmx` (supported by `qemu-domain-test.sh:46-48`). Verified here: lane PASS with PCID live. This hides the VMX path, so use it only until (1) lands.
3. **Detection gaps** (owner: `scripts/x86/qemu-domain-test.sh`, phase 07b already modifies it):
   - QEMU exit 0 before the window ends means a guest reset under `-no-reboot`. Fail fast with "guest reset/triple fault" instead of reporting missing markers.
   - Require `X86-VMM-SMOKE: (PASS|SKIP)` and reject `FAIL`. TCG currently passes with a silent SVM `FAIL`.
   - No CI job runs this lane on KVM, so only local runs can catch it.

## Repro (in a private worktree)

```
bash scripts/build-x86_64-domain-test-ci.sh
X86_ACCEL=kvm bash scripts/x86/qemu-domain-test.sh target/x86-domain-test/x86_64-unknown-none/release/cellos-kernel-domain-test build/dt-kvm.iso
# triple-fault state:
qemu-system-x86_64 -machine q35 -accel kvm -cpu host -m 256M -nographic -cdrom build/dt-kvm.iso -boot d \
  -action reboot=shutdown,shutdown=pause -monitor unix:/tmp/mon.sock,server,nowait   # then: info registers / info lapic
```

## Unresolved

- HEAD `28dee8b3d` cannot build the domain-test image. The F1 check refuses to sign because the committed virtio-blk/net/scsi files (`24d75559a`) are missing from `scripts/unsafe-allowlist.toml` (`REFUSED: not signing`). The diagnosis worktree added throwaway entries. The main tree's uncommitted allowlist diff only adds mlibc entries [INFERENCE: other uncommitted changes may differ]. Needs an owner.
- Which path sends the self-IPI 0xF0 just before the smoke was not traced. Any external interrupt that arrives during a VMX run hits the same path.
- `HOST_FS_BASE=0` and `HOST_IA32_EFER` without the load-EFER exit control did not show up in these runs. Review them when applying fix 1.
