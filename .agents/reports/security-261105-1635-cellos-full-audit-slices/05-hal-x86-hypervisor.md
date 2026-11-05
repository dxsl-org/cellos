## Slice: HAL x86 + hypervisor (VMX/SVM/EPT) + firmware tables + userspace VMM — files read: 36 (me) + 45 (VMM scout) + 15 (firmware scout) — unsafe blocks reviewed: ~60 (me) + 13 + 53

### Findings

#### [S-1] `MapGuestMemory` with `size ≥ 2^64−4095` wraps the span check and maps host physical memory beyond the guest carve into the guest — Severity: Critical — STRIDE: E — OWASP: A04
- Where: kernel/src/task/syscall.rs:10109-10111; kernel/src/hypervisor/svm_registry.rs:380-391; kernel/src/memory/ept.rs:258-290 (`let span = (n_pages as u64) * PAGE_SIZE as u64` at :265, SAS check :278, loop :284); same bug in kernel/src/memory/stage2.rs:246-287 (`page_bytes` at :253) and stage2.rs:318.
- What: The syscall only checks `ipa.checked_add(size)`; `size.div_ceil(PAGE_SIZE)` then yields `n_pages = 2^52` for `size = usize::MAX`, and `n_pages * 4096` wraps to 0 in release (workspace `[profile.release]` sets no `overflow-checks`, Cargo.toml:258-261). With `span == 0`, `gpa_end == gpa`, `hpa_end == hpa`, so the MMIO-hole and SAS checks (`hpa_end > guest_end`) pass, and the `for _ in 0..n_pages` loop maps consecutive HPAs starting at `guest_pa` straight past the carved guest RAM until the frame allocator runs dry — with no rollback of the partial mapping (the `Err(OutOfMemory)` is returned after the leaves are written).
- Exploit: Any cell holding `HypervisorCap` calls `MapGuestMemory{ipa:0, size:usize::MAX, writable:true}`; the guest then reads/writes every host frame above its carve (kernel heap, other cells, page tables, the EPT itself). Dev profile instead panics the kernel.
- Fix: In `map()`: `let span = (n_pages as u64).checked_mul(PAGE_SIZE as u64).ok_or(Overflow)?`; additionally cap `n_pages ≤ self.guest_ram_pages` and reject at the syscall when `size > guest_pages * PAGE_SIZE`; unmap on partial failure.

#### [S-2] VMX: no TPR shadow and no CR8 load/store exiting — guest `mov cr8` reprograms the host LAPIC TPR and masks all host interrupts on that CPU — Severity: High — STRIDE: D — OWASP: A04
- Where: hal/arch/x86/src/x86_64/vmx_vcpu.rs:188-199 (pin = external-interrupt exiting only; proc = HLT|UNCOND_IO|SECONDARY — no `CR8_LOAD/STORE_EXITING`, no `USE_TPR_SHADOW`); kernel/src/hypervisor/svm_registry.rs:408-415 (guest runs with `cli` under `VM_REGISTRY`).
- What: With "use TPR shadow"=0 and CR8-load exiting=0, MOV to CR8 in VMX non-root writes the physical LAPIC TPR; VM exit does not restore it. A 64-bit guest (`CR0.PG` transition at :431-461 enables IA-32e) executes `mov cr8, 15`; from then on vectors 0x20 (timer) and 0xF0 (IPI) are never delivered, so external-interrupt exiting never fires and a spinning guest owns the CPU forever while holding `VM_REGISTRY` with IF=0. Even after a HLT exit the host TPR stays 0xF0 (nothing in the host resets it).
- Exploit: Guest kernel code: `mov cr8, 15; jmp $`.
- Fix: Set `CPU_BASED_CR8_LOAD_EXITING|CR8_STORE_EXITING` (or TPR shadow with a virtual-APIC page), and restore host TPR after exit. SVM is safe here (`VINTR_MASKING` set, vmcb.rs:119).

#### [S-3] SVM: CLGI/STGI (plus XSETBV, SKINIT, VMLOAD/VMSAVE, INVLPGA, MONITOR/MWAIT) are not intercepted; guest `clgi; jmp $` holds the CPU forever — Severity: High — STRIDE: D — OWASP: A04
- Where: hal/arch/x86/src/x86_64/vmcb.rs:109-114 (`INTERCEPT1 = INTR|CPUID|PAUSE|HLT|IOIO|MSR`, `INTERCEPT2 = VMRUN` only); hal/arch/x86/src/x86_64/svm_vcpu.rs:355 and vmcb.rs:126 (guest EFER.SVME forced on, so SVM instructions are legal in the guest); CPUID passthrough svm_vcpu.rs:335-345 advertises SVM.
- What: With GIF cleared by an un-intercepted `CLGI`, physical INTR/NMI are not recognised in the guest, so `VMEXIT_INTR` never occurs; `MAX_INTERNAL_EXITS` (svm_vcpu.rs:30) cannot help because no exit happens. The host CPU is lost with IF=0 and `VM_REGISTRY` held. Un-intercepted `XSETBV` (guest sets CR4.OSXSAVE freely — CR intercepts are 0 at vmcb.rs:109) changes the physical XCR0, which `#VMEXIT` does not restore; `SKINIT` executes natively.
- Exploit: Guest ring-0: `clgi; jmp $` (or `xsetbv` to enable AVX, then rely on S-6).
- Fix: Set INTERCEPT2 bits for VMMCALL/VMLOAD/VMSAVE/STGI/CLGI/SKINIT/XSETBV/MONITOR/MWAIT/INVLPGA (KVM's baseline) and route them to `Unknown`; save/restore XCR0 around `svm_vmrun`.

#### [S-4] Guest PKRU is not restored on VM exit (both backends); on a PKU-armed host the kernel faults on its first user-page access after the exit — Severity: High (PKU+IBT hardware profile) — STRIDE: D — OWASP: A04
- Where: hal/arch/x86/src/x86_64/world_switch.rs:69-143 and :147-250 (only GPRs saved; no PKRU/XCR0 handling); hal/arch/x86/src/x86_64/vmx_vcpu.rs:410-429 and svm_vcpu.rs:335-345 (CPUID leaf 7 passed through → guest sees PKU; CR4 writes never exit); hal/arch/x86/src/x86_64/pku.rs:98-124 (production sets CR4.PKE when PKU+IBT exist; comment at :126-129 wrongly states PKRU "only restricts Ring-3 accesses"); kernel/src/hypervisor/registry.rs:559-561 (`core::ptr::write(exit_out…)` to a U=1 page after the exit).
- What: PKRU is not in the VMCS/VMCB and is not saved by VMRUN/VMLAUNCH exits. The guest sets CR4.PKE and `wrpkru` with EAX=0xFFFFFFFF; after the exit the host kernel runs with that PKRU. Protection keys also gate supervisor accesses to U/S=1 pages, so the kernel's write of the exit record / `vcpu_regs` buffer takes a kernel-origin #PF → `Route::PageFault` → `vi_handle_page_fault` with kernel CS (fatal path).
- Exploit: Guest: set CR4.PKE, `wrpkru` all-deny, then any exiting instruction (`hlt`).
- Fix: Read/write PKRU around the world switch (KVM pattern: `if host_pkru != guest_pkru { wrpkru(host) }`), or mask the PKU CPUID bit and intercept CR4.PKE.

#### [S-5] VMX host state (HOST_GS_BASE, HOST_TR_BASE, HOST_CR3) is captured once at `create_vcpu` on the creating CPU; the VMCS is never VMCLEARed before use on another CPU — Severity: High [INFERENCE: requires the VMM task to be rescheduled on a different hart, which `push_ready` does when the waker runs elsewhere] — STRIDE: T/E — OWASP: A04
- Where: hal/arch/x86/src/x86_64/vmx_vcpu.rs:101, :147, :163; kernel/src/hypervisor/svm_registry.rs:281-296; kernel/src/task/scheduler.rs:827-842 (task lands on the *calling* hart's ready queue); hal/arch/x86/src/x86_64/syscall.rs:58-77 (per-CPU `CpuLocal` reached through GS).
- What: After a VM exit on CPU B, the CPU loads GS base = CPU A's `CpuLocal` and TR base = CPU A's TSS. Every subsequent `gs:`-relative access on CPU B (`current_cpu_id`, `set_kernel_stack` → CPU A's TSS.rsp0, `gs:24` CR3 scratch, `gs:16` PKRU) corrupts CPU A's per-CPU state; an NMI/#DF on B then runs on A's IST stack concurrently with A. `vmptrld` of a VMCS still active on CPU A is architecturally undefined. SVM is immune (`vmsave` host state each entry, world_switch.rs:73-74).
- Exploit: VMM cell blocks on IPC (e.g. net/compositor); the waker on another hart queues it there; next `RunVcpu` exits on the wrong CPU.
- Fix: Re-write HOST_GS_BASE/HOST_TR_BASE/HOST_CR3/HOST_RSP on every `run()` (or pin the vCPU to its creating CPU and VMCLEAR+VMPTRLD on migration).

#### [S-6] No INVEPT ever executed: EPT remap/unmap is not invalidated and teardown frees EPT roots/guest RAM without INVEPT or VMCLEAR — Severity: High [INFERENCE on TLB retention; capacity makes it probabilistic] — STRIDE: I/T — OWASP: A04
- Where: kernel/src/memory/ept.rs:298-317 (`unmap`), :344-369 (`map_single` overwrites leaves), :395-414 (`Drop` frees root + sub-tables + guest RAM); kernel/src/hypervisor/svm_registry.rs:559-570 (`reap_vms_for_task` just drops); only `vmclear` call is at vmx_vcpu.rs:53 (creation); `grep invept` → none.
- What: Guest-physical EPT translations are tagged by EP4TA and survive VM entries/exits (no VPID is enabled either). A later VM whose EPT root frame is reallocated at the same PA (or whose guest RAM reuses freed frames) can hit stale cached GPA→HPA entries pointing at frames now owned by the kernel or another cell. Unmapping a GPA from a live guest is also ineffective until the TLB entry ages out. The VMCS frame is freed while still the current VMCS; the CPU may later write cached VMCS data into the re-used frame. SVM is safe: ASIDs never reuse (svm_registry.rs:112-121) and `TLB_FLUSH_ASID` on every VMRUN (vmcb.rs:118).
- Exploit: Cell creates VM1, has the guest touch many GPAs, destroys it, creates VM2 (likely same root PA from the free list), guest reads GPAs mapped in VM1.
- Fix: `INVEPT single-context` after every `map_single`/`unmap_single` batch and before freeing in `Drop`; `VMCLEAR` each VMCS in `SvmVm::drop`.

#### [S-7] VMX `inject_irq`/diagnostics operate on whichever VMCS happens to be current, not the target vCPU's — Severity: Medium — STRIDE: T — OWASP: A01
- Where: kernel/src/hypervisor/svm_registry.rs:66-83 (`vmread(GUEST_RFLAGS)`/`vmwrite(VM_ENTRY_INTR_INFO_FIELD)` with no `vmptrld(self.vmcs_pa)`), :88-108.
- What: `VmxVcpu::run` does `vmptrld` (vmx_vcpu.rs:364) but `inject_ext_irq_gated` does not, so with two VMs (possibly different owner cells) an `InjectIrq` for VM A lands in VM B's VMCS if B ran last; the IF/shadow gate is also read from the wrong guest, so an interrupt can be forced into an IRQ-off section.
- Exploit: Cell A calls `InjectIrq` right after cell B's `RunVcpu` returned.
- Fix: `vmptrld(self.vmcs_pa)` at the top of the `Vmx` arms (and in `shutdown_diagnostics`/`npf_diagnostics`).

#### [S-8] Guest AVX/YMM upper state and FPU state are neither saved nor restored by the world switch — Severity: Medium — STRIDE: I — OWASP: A04
- Where: hal/arch/x86/src/x86_64/world_switch.rs:69-143, :147-250 (GPRs only); hal/arch/x86/src/x86_64/syscall.rs:347-357 (`fxsave64`/`fninit` on entry, `fxrstor64` on exit covers only x87/SSE).
- What: On SVM a guest can `xsetbv` (S-3) to enable AVX; YMM/ZMM upper halves are not covered by FXSAVE, so after exit they survive the syscall return into the VMM cell and across context switches into other cells/VMs (a cross-tenant channel). Separately, the guest's own x87/SSE state is reset to `fninit` on every `RunVcpu` entry — a correctness bug for any FP-using guest. On VMX `XSETBV` exits unconditionally → `Unknown`, so only the FP-reset half applies.
- Exploit: Guest A loads secrets into YMM, HLTs; cell/guest B reads YMM.
- Fix: XSAVE/XRSTOR guest and host extended state around VMRUN/VMLAUNCH (or intercept XSETBV and reject AVX enablement); keep per-vCPU FP state.

#### [S-9] Firmware MMIO bases (HPET/MADT LAPIC/IOAPIC) identity-mapped without alignment/range/RAM-overlap checks; unaligned low bits leak into PTE flags — Severity: Medium — STRIDE: T/E — OWASP: A08 (from firmware scout; PTE write verified by me)
- Where: kernel/src/memory/paging.rs:527-553 (`write_volatile(pte_ptr, va as u64 | pte_flags_mmio())`, `base + PAGE_SIZE`); kernel/src/acpi/hpet.rs:42-44; kernel/src/acpi/madt.rs:34-36, 56-66, 92-95; kernel/src/acpi/dmar.rs:133; kernel/src/main.rs:497-519, 1911-1913.
- What: Parsers accept any non-zero address. An address with bit 2 set (`0xFED0_0004`) ORs U/S=1 into the LAPIC/HPET/IOAPIC PTE → user-accessible MMIO; an address inside Usable RAM rewrites that RAM's identity PTE as UC MMIO and IOAPIC register writes land in kernel RAM; `base + PAGE_SIZE` near `usize::MAX` wraps (release) or panics (dev).
- Exploit: Hostile/corrupt ACPI (supply chain / firmware).
- Fix: Reject `addr & 0xFFF != 0`, `addr >= 1<<52`, and any overlap with Usable/kernel ranges in the parsers; `checked_add` in paging.rs.

#### [S-10] u16 port arithmetic on FADT GPE/PM1 blocks overflows — Severity: Medium — STRIDE: D — OWASP: A08 (firmware scout)
- Where: kernel/src/acpi/sci.rs:91-99, 172-176; kernel/src/acpi/power.rs:455-469.
- What: `block.port + gpe_half + index` on `u16` with `port=0xFFF0, len=0x40` panics under overflow-checks or wraps to ports 0x00.. (8237 DMA / PIC) and writes 0xFF there.
- Exploit: FADT `GPE0_BLK=0xFFF0, GPE0_BLK_LEN=0x40`.
- Fix: `checked_add` in `gpe_blocks`/`sci::init`; refuse the block on `None`.

#### [S-11] virtio-input config-space reads return one byte per aligned word — Severity: Medium — STRIDE: - — OWASP: - (VMM scout)
- Where: cells/services/hypervisor/src/virtio_input.rs:277-321; virtio_mmio.rs:114-115; x86-mmio-dispatch.rs:97-99,135-137.
- What: Transport reads `config_read(off & !3) >> ((off&3)*8)`, but `InputDev::config_read` returns a single byte at bits 0-7 for a byte index, so every non-aligned byte reads 0; EV_KEY bitmap bytes 1-3,5-7,… and BTN_LEFT/RIGHT (byte 34) are unreachable.
- Exploit: Not attacker-driven; guest input driver sees a crippled device.
- Fix: Return the full LE 32-bit word for the aligned offset.

#### [S-12] VMX EPT-violation handler has no instruction decode: assumes 4-byte access via RAX and advances RIP by `VM_EXIT_INSTRUCTION_LEN` — Severity: Low — STRIDE: - — OWASP: -
- Where: hal/arch/x86/src/x86_64/vmx_vcpu.rs:386-409.
- What: Any EPT violation (including ones outside the virtio window, guest page-walk faults, or non-RAX/non-dword MOVs) becomes an `MmioRead{reg:0}`/`MmioWrite{val:RAX}` with RIP advanced; SVM has a proper decoder (vmexit_decode.rs:117-171) and GPA gate (`is_mmio_data_npf`). Likewise CR0 writes never exit (CR0 guest/host mask is 0) so the IA-32e transition at :431-461 is dead, and all RDMSR/WRMSR exit as `Unknown` (no MSR bitmap). Guest-only impact (VMX backend is functionally incomplete).
- Fix: Port the SVM decoder + GPA gate; add MSR bitmap and CR0/CR4 guest-host masks.

#### [S-13] SYSRET path restores R11 (RFLAGS) from the frame unmasked, unlike `__trap_exit` — Severity: Low — STRIDE: E — OWASP: A04
- Where: hal/arch/x86/src/x86_64/syscall.rs:396, :409-410 vs hal/arch/x86/src/x86_64/boot.rs:130-133 (masks to bits 11:0, forces 0x202).
- What: SYSRET loads IOPL/NT/AC from R11. Today `frame.sstatus` is only ever the caller's own RFLAGS (no kernel path writes it from user data — verified: only task.rs:201/210, scheduler.rs:1019 set constants), so this is not exploitable now, but any future sigreturn/context-restore syscall would hand IOPL=3 to ring 3.
- Fix: `and $0x0FFF, %r11; or $0x202, %r11` before `sysretq`.

#### [S-14] Speculative-execution and entry-hardening posture — Severity: Low — STRIDE: I — OWASP: A05
- Where: targets/x86_64-unknown-cellos.json (no retpoline, `cpu: x86-64`); hal/arch/x86/src/x86_64/syscall.rs:254-258 and idt/entry.rs:117-118 (no `lfence` after `swapgs`); hal/arch/x86/src/x86_64.rs:144-158, :218-241 (SMEP/SMAP/UMIP opt-in features, off by default); context.rs:153-209 (no IBPB on switch); no KPTI (shared kernel CR3 by design, domain.rs:14).
- What: No retpoline/IBRS/IBPB/STIBP, no MDS `verw`, no swapgs-fence; SMEP/SMAP/UMIP default off; docs/security-model.md:82-92 already states Spectre is unmitigated. Posture only.
- Fix: Enable `x86-smep`/`x86-umip` in production profiles; add `lfence` after `swapgs` on the user→kernel paths; consider IBPB on cell switch.

#### [S-15] Host NMI/#MC/INIT are delivered into the guest (no NMI intercept / NMI exiting) — Severity: Low — STRIDE: D — OWASP: -
- Where: hal/arch/x86/src/x86_64/vmcb.rs:110-113 (INTERCEPT1 lacks NMI/SMI/INIT bits); hal/arch/x86/src/x86_64/vmx_vcpu.rs:188 (pin controls lack `NMI_EXITING`).
- What: A host watchdog NMI or machine check arriving while a guest runs is handled by the guest IDT; the host never sees it.
- Fix: Set NMI/SMI/INIT intercepts (SVM) and `PIN_NMI_EXITING` (VMX).

#### [S-16] Guest `swapgs` (VMX) leaves a guest value in IA32_KERNEL_GS_BASE → VMM cell's user GS base is guest-controlled — Severity: Info — STRIDE: T — OWASP: -
- Where: hal/arch/x86/src/x86_64/world_switch.rs:147-250 (no KERNEL_GS_BASE save/restore; VMX exit loads only GS_BASE from HOST_GS_BASE, vmx_vcpu.rs:163); syscall.rs:409 (`swapgs` before `sysretq`).
- What: Kernel GS stays correct (the swap pair is symmetric), but the VMM cell resumes with a guest-chosen GS base. Rust userspace does not use GS, so impact is nil today. SVM restores via `vmload` (world_switch.rs:133-134).
- Fix: Add KERNEL_GS_BASE to a VM-exit MSR-load list or restore it after `vmx_vmrun`.

#### [S-17] MSI vectors are not remapped: an MSI-capable device owner can inject any vector (timer 0x20, IPI 0xF0, or 0-31 → `FatalException` halt) — Severity: Low — STRIDE: D — OWASP: A01
- Where: hal/arch/x86/src/x86_64/msi.rs:12-24 (range policy only applies to kernel-encoded messages); idt/policy.rs:57-69 (kernel-origin vector 0-31 → `FatalException`); kernel/src/task/smp.rs:899-905 (interrupt remapping acknowledged as absent).
- What: A Tier-1 driver cell with MMIO access to a PCI function's MSI capability writes an arbitrary vector; delivery of vector 8 looks like a kernel #DF and halts. Trusted-driver model mitigates.
- Fix: VT-d interrupt remapping, or have the kernel own MSI capability programming.

#### [S-18] Remaining scout findings (Low/Info) — see Where
- Firmware: duplicate XSDT entries parsed twice (sdt.rs:146-158, madt.rs:46-87, dmar.rs:139,159) — Low; v1 RSDP needs 36 admitted bytes (sdt.rs:224, mod.rs:208) — Low/correctness; two parallel SDT validators (sdt.rs:76-105 vs mod.rs:168-288) — Info; checksum `read_volatile` over any firmware "Reserved" range (sdt.rs:62-69, main.rs:402-414) — Low.
- VMM: virtio-input eventq skips `pending_count` and never completes rejected chains (virtio_input.rs:161-178, 250-253) — Low; unthrottled host-console `println` on guest-reachable error paths (x86-port-dispatch.rs:48-52, run_loop.rs:145-174, virtqueue.rs:126, virtio_blk.rs:268-273, virtio_net.rs:119-123, virtio_mmio.rs:143-183) — Low D; 8259 `vector_base + line` u8 overflow (pic_8259.rs:36-40,105-111) — Low; guest TX frames forwarded with any source MAC, net cell learns from them (virtio_net.rs:262-289; cells/services/net/src/handlers.rs:211-220) — Low S; short `write_guest_memory` treated as success + unchecked u64 adds in loaders (boot_x86.rs:24-30,98-104; boot_arm.rs:109-116,162; loader_image.rs:77-94) — Low; RX injection ignores DRIVER_OK (virtio_net.rs…
- Doc drift: `switch_with_root` doc (hal/arch/x86/src/x86_64/context.rs:142-151) and domain_admission.rs:142-147 say CR3 is programmed inside the switch; the asm only updates `gs:24` (context.rs:173-185) and the trap-exit paths write CR3 — Info.

### Verified-safe areas
- SYSCALL entry: `swapgs` first, user RSP stashed in `gs:8`, kernel stack from `gs:0`, full frame + FXSAVE before Rust, IA32_FMASK clears IF/TF/DF/AC/NT/IOPL (syscall.rs:44-46, 254-357); exit reloads user RSP from the frame not `gs:8` (:401-406); non-canonical RCX trapped before SYSRET (:362-366); PKRU 0 on entry / task value on exit; CR3 switch after the frame is committed (:333-344). `KERNEL_GS_BASE=0`, CR4.FSGSBASE never enabled (no user wrgsbase).
- IDT common: `swapgs` only when saved CS RPL==3 on both entry and exit (idt/entry.rs:117-118, 199-203); CLAC gated on `ViCell_smap_active`; `cld`; CR3/PKRU restore skipped for kernel-destination; IRETQ rebuilds nothing from user data. Only vector 0x80 has DPL 3 and it is a no-op route (idt.rs:102, policy.rs:65). NMI/#DF/#MC on per-CPU IST stacks with guard pages (ist.rs, gdt.rs:142-147); their handler reads no GS (fatal.rs). TSS `iomap_base=104` denies ring-3 port I/O (gdt.rs:139); `__trap_exit` masks RFLAGS to bits 11:0|0x202 (boot.rs:130-133).
- Context switch saves/restores `gs:24` (user CR3) per task (context.rs:173-185) — no cross-task CR3 confusion. PCID/INVPCID gating and untagged fallback (domain.rs:111-161).
- Nested paging: `map()` SAS carve check and MMIO-hole check (ept.rs:270-281) hold for all sane sizes; `map_device_frame` only used for the kernel-owned APIC frame (svm_registry.rs:237-240); `read/write_guest_memory` bounded by `guest_pages*PAGE_SIZE` with per-page `translate` (svm_registry.rs:445-519); `vcpu_regs` is 16 words behind `validate_user_buf(…,256)` (syscall.rs:10159); SVM ASIDs monotonic, never reused (svm_registry.rs:112-121).
- SVM: IOPM/MSRPM all-ones; passthrough list is exactly the VMSAVE/VMLOAD-managed MSRs; EFER write stays intercepted and SVME re-asserted (svm.rs:122-144, svm_vcpu.rs:349-357); guest page-walk for instruction fetch is bounded to guest RAM (svm_vcpu.rs:220-271); MMIO decoder bounds `reg<16`, rejects RSP/high-byte regs, cursor checked (vmexit_decode.rs:117-171, 173-192); `is_mmio_data_npf` restricts to the virtio window; `MAX_INTERNAL_EXITS` caps CPUID/MSR/PAUSE spins (svm_vcpu.rs:30, 309-314); host `vmsave`/`vmload` fence with `clgi`…`stgi` and `cli` before any `gs:` access (world_switch.rs:69-136); `VINTR_MASKING` protects host TPR.
- VMX: VMXON revision/fixed-bits/FEATURE_CONTROL readback (vmx.rs:83-146); HOST_RSP/HOST_RIP rewritten each run (world_switch.rs:156-162); callee-saved host regs restored on both exit and VMfail paths; `VMCS_LINK_POINTER=~0`; guest DR7 reserved bit set.
- Firmware/SMBIOS: all table bytes admitted through the mapper before deref; lengths bounded 36..=1 MiB; `TableBytes` checked reads; MADT/DMAR/SMBIOS record walks terminate on `len < min`; SMBIOS string walk double-NUL bounded (see scout list: mod.rs:172-183, sdt.rs:80-98, bytes.rs:27-33, madt.rs:39-42, dmar.rs:116-124, smbios.rs:123-357, mcfg.rs:50-62, power.rs:154-237, 300-305, 360-362, 461-467, 600-607).
- VMM: descriptor chains index/flag/overflow checked and length-capped (virtqueue.rs:24-70, virtqueue_guard.rs:19-85); `queue_sel` bounded; blk `sector*512` saturating + capacity check (virtio_blk.rs:340-356, 485-502); net TX ≤ IPC buffer with `checked_add`; gpu `w*h*4` `checked_mul`, attach ≤128 entries, cursor ≤64×64, resource table ≤64; UART/PIT/RTC port offsets bounded; GIC arrays via `get`; ELF/notes reads via `.get()` (loader_image_x86.rs:195-243).

### Not covered
- hal/arch/x86/src/x86_32/** (only built for `target_arch = "x86"`, no such target in `targets/`), common.rs, critical_section.rs.
- hal x86_64: clock/*, hpet.rs, rtc.rs, uart_16550.rs, ioapic.rs, cet.rs, trap.rs, apic.rs (only `register_cpu`/`cpu_apic_id` bounds read), vmcs.rs field tables (:144-448), svm.rs:1-99 (`enable`), pku.rs beyond `init`, paging.rs beyond flag policy/`walk_create`/`map_bios_area` header, idt/{cpl3_*,probe*}.rs (all `x86-idt-cpl3-test` test-only).
- kernel: hypervisor/smoke_guest.rs and pending_irqs.rs (aarch64-only), svm_registry.rs:644-780 (`test-hooks` smoke), registry.rs aarch64 branches, stage2.rs beyond `map`/`map_mmio_passthrough`, cpu_features.rs:84-217 beyond root-entry gating, acpi/tests*.rs (test-only), `validate_user_buf`/`caller_has_hypervisor` bodies (KernelSyscall slice), `iommu::init`, `ioapic::add_unit`.
- VMM: virtio-mmio-tests.rs (test-only); net cell bridge semantics beyond handlers.rs:211-220.

[Some lines truncated to 768 chars. Use agent://HalX86Hypervisor-2:raw:103-103 to read line 103 whole]