# Scout report — Chrome guest kiosk and resources (2026-11-25)

Tree: `main` at `894048d219` plus this plan's phase-01 edits. Machine: i7-14700, WSL2 (Hyper-V),
nested KVM, QEMU 11.1.3. Sources: reading the tree (three read-only Explore agents plus the
main session) and the measurements named below. Line numbers are as read on 2026-11-25.

## 1. Measurements

| What | Result | How |
|---|---|---|
| Chrome lane at `894048d219` | 13/13, Chromium UI 29 s after the click | `scripts/qemu-tier3-chrome-x86.sh` |
| Same guest image, plain QEMU/KVM microvm, 1 / 2 / 4 vCPUs | cage -> Chromium UI 6.9 / 6.0 / 5.9 s | virtio-mmio devices, `-smp N`, screen polled every 0.25 s |
| CellOS hypervisor, outer machine with 1 / 2 / 4 CPUs | click -> Chromium UI 28.9 / 29.9 / 28.9 s; cage starts at guest uptime 5.1-5.8 s | lane ISO, `-smp N`, screen polled every 0.5 s |
| Window content from click to Chromium, before phase 01 | black for 26 s, with cage's pointer on it from about 14 s | one screen per second |
| Pointers over the window, before phase 01 | two: the compositor's arrow and the guest's | screen crops |
| Ctrl+Alt+F2, before phase 01 | the window goes black (a Linux VT) | screen |
| Chromium's own restore button, before phase 01 | Chromium shrinks, cage's black background shows around it | screen |
| Chromium quit from inside | guest prints `reboot: Power off not available: System halted instead`; the hypervisor never ends | serial log |
| VM exits, click -> Chromium UI, one vCPU | 60,000 `RunVcpu` calls in 25.0 s: 17.2 s in the VMX run scope, 12.6 s of that in the guest. Port I/O 47.5k (UART 32.5k, 8259 14.8k), EPT violations (virtio) 7.0k, HLT 3.9k, host interrupts 1.6k, CPUID 1.3k | temporary counters, removed afterwards |
| Same, idle Chromium for the next 20 s | 60,000 more calls: HLT 45.6k, 8259 10.3k; 10.7 s counted in the guest | same |
| Same, with `loglevel=4` and `seatd -l error` (two runs) | click -> Chromium UI 20.8 and 22.3 s; UART exits 2.6k; guest console 15 lines instead of 242 | same |

Reading: under KVM one nesting level up the same vCPU needs about 9.9 s from boot to
Chromium's UI; here the guest alone took 12.6 s and the VMM added as much again: 76 µs of VMX
bookkeeping per `RunVcpu` call (4.6 s) and 7.8 s outside the run scope. More than half of the
calls were console bytes. CPUs added to the outer machine change nothing, and an idle guest
spins a host CPU on HLT.

## 2. x86 guest virtualization (kernel, HAL)

- Syscalls 220-227 (`libs/abi/abi/syscalls.toml:1065-1158`): `CreateVm`, `CreateVcpu`,
  `MapGuestMemory`, `RunVcpu(vm, vcpu, budget_ns, out)`, `VcpuRegs`, `InjectIrq`,
  `Write/ReadGuestMemory`. Store for SVM and VMX: `kernel/src/arch/x86_64/hypervisor/svm_registry.rs`.
- One static `VM_REGISTRY` IRQ-off spinlock is held across guest execution
  (`svm_registry.rs:530-586`); `InjectIrq`, `VcpuRegs` and guest-memory copies take it too.
- Every `RunVcpu` on VMX does: `INVEPT` single-context (`svm_registry.rs:547`), `VMPTRLD`,
  17 host-state `VMWRITE`s (`vmx_vcpu/host_state.rs:46-91`), `VMLAUNCH` (never `VMRESUME` across
  calls), then `VMCLEAR` (`vmx_vcpu/vmcs_scope.rs:24-35`). MSR, CPUID and CR exits are handled in
  the kernel loop; HLT, external interrupt (`Preempted`), port I/O and virtio-window EPT
  violations return to the Cell (`vmx_vcpu.rs:404-471`).
- `budget_ns` is ignored on x86 (`kernel/src/arch/x86_64/vm.rs:86`). No preemption timer, no
  interrupt-window exiting, no MSR bitmap (every MSR access exits), no VPID, no APICv.
- Injection is one slot and lossy: a vector is dropped when RFLAGS.IF is clear, in an STI
  shadow, or below the software TPR (`vmcs_scope.rs:44-56`).
- The guest LAPIC is one untrapped RAM page per VM at `0xFEE0_0000`, APIC ID 0
  (`svm_registry.rs:268-290`); on VMX nothing reads it, so ICR writes, EOI and the LAPIC timer
  do nothing. The only tick is the Cell's PIT IRQ0, delivered at exit boundaries.
- CPUID is the host's, filtered (`hal/arch/x86/src/x86_64/guest_cpuid.rs:44-65`): x2APIC,
  TSC-deadline, XSAVE, OSXSAVE and AVX hidden; hypervisor leaves zero; topology leaves raw.
  Guest FPU state is FXSAVE only.
- `create_vcpu` accepts several vCPUs but makes each a PVH boot CPU (`svm_registry.rs:348-365`).
- VMXON runs on every CPU (`smp_bringup.rs:413-426`); the VMCS is migration-safe because it is
  cleared after every use.
- Guest RAM: one contiguous host run mapped at GPA 0 with 4 KiB leaves only
  (`kernel/src/arch/x86_64/memory/ept.rs:150`, `frame.rs:1869`); below the virtio hole at
  `0xd000_0000` (3328 MiB). Budget: `guest_memory: 1544 * MIB` (`kernel/src/catalog.rs:398`).
- Threads: `ostd::task::spawn` (syscall `Spawn`), up to 32 per Cell; the hypervisor Cell is
  single-threaded and has no `Spawn` in its allowlist (`cells/services/hypervisor/src/main.rs:36-75`).

## 3. Hypervisor Cell (x86)

- PVH boot (`boot_x86.rs`), ACPI RSDP/XSDT/FADT/DSDT/MADT with one LAPIC entry and no IOAPIC
  (`acpi.rs`), command line `... nox2apic pci=off lpj=4000000 nohz=off highres=off ...`
  (`boot_x86_profile.rs:27`), `tsc_early_khz` appended.
- Devices: 16550, 8259 (master only, no IRR/ISR), 8253, CMOS RTC on ports; virtio-mmio v2 at
  `0xd000_0000` stride `0x200`: blk slot 0 IRQ 5, net slot 1 IRQ 6, gpu slot 3 and input slot 4
  sharing IRQ 3; slots 2 and 5-31 free; `MAX_QUEUES = 2` per device (`virtio_mmio.rs:9`).
- Run loop (`run_loop_x86.rs`): devices are polled only at `Hlt` and `Preempted` exits; the
  `Hlt` arm re-enters at once (an idle guest spins); the launcher mailbox is drained there too.
- virtio-gpu is 2D only; the cursor queue is handled; scanout pixels are copied row by row with
  `ReadGuestMemory` into a Grant shared with the launcher (`virtio_gpu/resource/render.rs`).
- `virtio_console.rs` exists (single port) but is not instantiated on x86.

## 4. Desktop and host services

- Compositor opcodes `0x01-0x13`, `0x20-0x28` (`libs/wire/src/display.rs:1130-1266`); events
  `0xA0-0xA3`. One fixed 16x16 arrow; no API for a surface to set or hide the pointer
  (`cells/services/compositor/src/cursor_state.rs`, `render.rs:115`).
- No clipboard service or API; Files, the editor and Terminal keep private state. ADR-0017 §2.2
  names "clipboard bridge" and "file sharing" as required additions.
- No audio driver, service or API; syscall 218 `AudioPlay` is retired.
- VFS: path and handle requests (`libs/wire/src/ipc.rs:39-290`), rules by path
  (`cells/services/vfs/src/access/rules.rs:84-149`): `/data`, `/tmp`, `/mnt/sd`, `/mnt/disk`,
  `/srv` writable; `/mnt/sd` is 256 MiB FAT32 and holds the 246 MiB guest root.
- `wire::vmm_session` (`libs/wire/src/vmm_session.rs`): ops 1-5, events `0x81-0x83`, 64-byte
  receive buffer in the hypervisor, 32 messages per poll.

## 5. Change control

- Law 1 (`docs/code-standards.md:40-47`): `libs/abi`, `libs/types` need the maintainer's
  design approval before the edit and approval of the delta after. `libs/wire` is formally
  outside it, but every wire opcode since 2026-11-11 has a record with a CP column in
  `docs/abi-boundary-records.md` (§16-§20); §17-§20 still read "CP2 pending".
- Service ids are `libs/abi/src/syscall.rs` `pub mod service` (Law 1).
- Lock inventory (`.agents/cell-contract-0024-locks.md:104, 213`): Intel VMX/EPT is under P09
  with "explicit per-slice handoff"; the first Chrome plan proceeded on the user's decision.
- ADR-0025 §3.2: no KVM-compatible subsystem, VMM stays scoped to its approved workload.
  ADR-0017 §5d: guest GPU acceleration is "a separate later plan".
- Gates: `scripts/ci-local.py run lint|x86_64-lint|aarch64-lint|unit-tests|f1-admission`;
  x86 hypervisor lanes in CI run SVM under TCG (QEMU 10.2.0); the VMX backend and the Chrome
  path run only in the manual KVM lanes `qemu-tier3-chrome-x86.sh` and `qemu-tier3-display-x86.sh`.
- Pins an edit moves: `X86_CANDIDATE_RECIPE_SHA256` (assembler), guest package versions and the
  builder digest in `scripts/build-chrome-guest-image.sh`, the mutable-globals inventory for new
  kernel/HAL statics, `kernel/residents.toml` for new kernel files.
- ADR-0030: device models come from public specifications; GPL sources are not consulted.

## 6. Unresolved

- Whether KVM offers CellOS the VMX preemption timer, VMCS shadowing and APICv here is not
  probed; phase 02 reads the capability MSRs.
- Where shared files live: `/mnt/sd` has about 10 MiB free; `/data` needs offline provisioning.
- No physical Intel machine has run any of this.
