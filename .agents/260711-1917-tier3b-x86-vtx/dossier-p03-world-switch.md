# Design Dossier — P03: VMCB/VMCS + vCPU World-Switch + Exit Decode

> **Scope:** de-risk the make-or-break phase before a line of production code is written.
> **Primary target:** AMD SVM (VMCB / VMRUN / #VMEXIT) — the only hardware-virt path that runs
> under QEMU TCG CI (`-cpu qemu64,+svm -accel tcg`). **VT-x delta (§8)** kept separate for P09.
> **Deliverable is analysis + pseudo-code only** — no production Rust. Implementation later is
> mechanical transcription of §2/§4/§5 pseudo-code + §3 checklist ticks.
>
> **Reference spec editions cited:** AMD APM Vol 2 "System Programming" (Pub. 24593), §15 "Secure
> Virtual Machine" + Appendix B "Layout of VMCB" + Appendix C "SVM Intercept Codes". Intel SDM Vol 3C
> §23-28 for the VT-x delta. Where a value must be exact, the section/table is cited inline. **Every
> numeric offset/bit below is `[VERIFY against your local APM revision]` — TCG follows the APM but
> some fields (decode-assist, nRIP) are revision/CPUID-gated.**

## Anchor: what already exists (verified in-repo, re-grepped)

| Asset | Location | Role in P03 |
|-------|----------|-------------|
| ARM64 world-switch (working reference) | `hal/arch/arm/src/aarch64/vcpu.rs:160` `run_vcpu_impl` | The structural template §2 maps onto |
| ARM64 exit decode | `hal/arch/arm/src/aarch64/vcpu.rs:121` `decode_exit` → `trap_el2::decode_vmexit` | Template for §4 decoder |
| ARM64 smoke + 1000× isolation test | `kernel/src/hypervisor/smoke_guest.rs:155` `run_register_isolation` | Template for §7 test |
| VM registry (arch-generic shell) | `kernel/src/hypervisor/registry.rs:69/119/182` `create_vm`/`create_vcpu`/`run_vcpu` | x86 branch added in P03 |
| HAL exit enum (not-yet-ABI-frozen) | `hal/traits/hypervisor/src/lib.rs:10` `ViVmExit` | P03 emits this; P04 freezes `#[repr(C)]` |
| API exit enum (frozen at P04) | `libs/api/src/abi/hypervisor.rs:19` `ViVmExit` (`#[repr(C,u8)]`, VERSION=1) | P04 adds PortIn/PortOut/Hlt/Msr |
| x86 hypervisor stub (to replace) | `hal/arch/x86/src/hypervisor.rs:11` `X86_64Hypervisor` all-NotSupported | P03 wires SVM backend |
| x86 CPU-local via GS | `hal/arch/x86/src/x86_64/context.rs`, trap.rs (`gs:16` kernel_rsp) | **The GS.base leak hazard — see §3/§7** |
| CET-IBT landing-pad convention | `hal/arch/x86/src/x86_64/boot.rs:80` (`.byte 0xF3,0x0F,0x1E,0xFA` = ENDBR64) | World-switch asm entry needs this |
| x86 `ViTrapFrame` (288B fixed) | `hal/arch/x86/src/x86_64/trap.rs:50` | Host IDT path for VMEXIT_INTR dispatch |

**Structural invariant carried from ARM (verified `vcpu.rs:5-17` doc-comment):** the world-switch is a
*coroutine* — `run_vcpu_impl` calls an asm stub that "returns" only when the guest traps. On x86 the
`#VMEXIT` returns control to the instruction *after* `VMRUN` (no separate trap vector like ARM's
`vt_vcpu_trap`), which is **simpler** than ARM — no VBAR/IDT trampoline for the exit itself. The host
IDT is only involved for the physical-interrupt (`VMEXIT_INTR`) re-dispatch path (§6).

---

## 1. VMCB layout map (AMD)

The VMCB is one 4 KiB, 4 KiB-aligned page. Two halves: **Control Area** (offset 0x000–0x3FF) and
**State-Save Area** (offset 0x400–0xFFF). `VMRUN RAX` = VMCB physical address (low 12 bits must be 0).

### 1a. Control Area — fields P03 touches (APM Appendix B, Table B-1)

| Off | Field | Width | P03 value | Set when | Notes |
|-----|-------|-------|-----------|----------|-------|
| 0x000 | CR read/write intercepts | dword | **bit 16 = CR0-write** | create | low16=reads, high16=writes; CR0-write = bit16 (§5 IA-32e dance). CR3 read/write intercepts NOT set (NPT active) |
| 0x008 | Exception intercepts | dword | 0 (MVP) | create | one bit per vector 0-31; leave off for M1 |
| 0x00C | Instruction intercept set 1 | dword | **INTR(0) · CPUID(18) · HLT(24) · IOIO_PROT(27) · MSR_PROT(28)** | create | INTR(0) for budget timer (§6); IOIO(27) drives PortIn/Out; MSR(28) drives Msr + EFER dance |
| 0x010 | Instruction intercept set 2 | dword | **VMRUN(0) = MANDATORY** | create | VMRUN intercept bit **must** be 1 or VMRUN → `VMEXIT_INVALID` (§3). VMMCALL(1) set → hypercall (`Vmmcall`→ maps to Hvc-equiv) |
| 0x040 | IOPM_BASE_PA | qword | 0 (unconditional IOIO) OR IOPM page PA | create | with IOIO_PROT set + IOPM=0 → all ports intercept; for MVP intercept-all is fine |
| 0x048 | MSRPM_BASE_PA | qword | MSRPM page PA | create | MSR permission bitmap; must be a real 8 KiB-aligned page even if all-intercept |
| 0x050 | TSC_OFFSET | qword | 0 (MVP) | create | leave 0; Linux TSC calibrates via PIT fallback (no-LAPIC MVP) |
| 0x058 | GUEST_ASID | dword | **≥ 1** (e.g. 1) | create | **ASID=0 → `VMEXIT_INVALID`** (§3) |
| 0x05C | TLB_CONTROL | byte | 1 (flush this ASID on VMRUN) for MVP | each switch | safe default; optimize later |
| 0x060 | V_INTR control (V_TPR/V_IRQ/V_INTR_MASKING…) | qword | **V_INTR_MASKING(bit24)=1** | create | so physical INTR governed by host IF, not guest IF (§6 budget) |
| 0x068 | Interrupt shadow | qword | read on exit | — | bit0 = guest in interrupt shadow (affects re-inject/PC advance) |
| 0x070 | **EXITCODE** | qword | **read every exit** | read | Appendix C code → decoder (§4) |
| 0x078 | **EXITINFO1** | qword | read every exit | read | IOIO qualifier / NPF error code (§4) |
| 0x080 | **EXITINFO2** | qword | read every exit | read | IOIO next-RIP / NPF faulting GPA (§4) |
| 0x088 | **EXITINTINFO** | qword | read on exit | read | event in-flight at exit — must re-inject if valid (§4 note) |
| 0x090 | NP_ENABLE | qword | **bit0 = 1** (nested paging) | create | required — guest isolation depends on NPT (P02) |
| 0x0A8 | **EVENTINJ** | qword | write to inject | as needed | V(31)/TYPE(10:8)/VEC(7:0)/EV(11)/ERRCODE(63:32). P05 IRQ injection |
| 0x0B0 | **N_CR3** | qword | NPT root PA from P02 `ncr3()` | create | `registry.rs`/P02 supplies this |
| 0x0C0 | VMCB Clean Bits | dword | 0 (MVP: everything dirty) | each switch | perf optimization only; 0 = always reload = correct-but-slow |
| 0x0C8 | nRIP | qword | read on exit (if supported) | read | next-sequential RIP for instruction intercepts → PC advance without instr decode (CPUID `8000_000A` EDX NRIPS bit) |
| 0x0D0 | #NPF decode-assist bytes | 16B | read on exit (if supported) | read | guest instruction bytes + count; needed for MMIO reg/size decode when NRIPS/DecodeAssists present |

**Field write-frequency partition** (the money question for the reviewer):

- **Set ONCE at `create_vcpu`:** all intercept dwords (0x000/0x008/0x00C/0x010), IOPM/MSRPM base,
  ASID, NP_ENABLE, N_CR3, V_INTR_MASKING, and the *initial* PVH guest state (below). These never
  change across a run loop for one guest.
- **Written EVERY world-switch (host side):** only TLB_CONTROL (if flushing) and EVENTINJ (if an IRQ
  is pending this entry). **RAX/RSP/RIP/RFLAGS/segments/CR/EFER are NOT written by software each
  switch — VMRUN and #VMEXIT save/restore them in the state-save area automatically (§2).**
- **Read EVERY world-switch (host side):** EXITCODE, EXITINFO1, EXITINFO2, EXITINTINFO, nRIP.

### 1b. State-Save Area — PVH entry state (APM Appendix B, Table B-2; PVH contract from plan research #4)

Segment entries are 16 bytes: `{selector:u16, attrib:u16, limit:u32, base:u64}`. **SVM attrib is the
*packed* AMD form** (12-bit: type[3:0], S[4], DPL[6:5], P[7], AVL[8], L[9], DB[10], G[11]) — NOT the
raw x86 descriptor access byte. `[VERIFY: APM §15.5 "Segment State in the VMCB" — packed vs unpacked]`.

| Off | Field | PVH init value | Written by VMRUN/#VMEXIT? |
|-----|-------|----------------|---------------------------|
| 0x400 | ES | flat data: sel=0x10, attrib=0xC93, limit=0xFFFFF, base=0 | yes (auto) |
| 0x410 | CS | flat code: sel=0x08, **attrib=0xC9B** (P,S,code,R, DB=1,G=1, **L=0**), limit=0xFFFFF, base=0 | yes |
| 0x420 | SS | sel=0x10, attrib=0xC93 | yes |
| 0x430 | DS | sel=0x10, attrib=0xC93 | yes |
| 0x440 | FS | sel=0x10, attrib=0xC93 | **VMLOAD/VMSAVE only** (§2 hazard) |
| 0x450 | GS | sel=0x10, attrib=0xC93 | **VMLOAD/VMSAVE only** (§2 hazard) |
| 0x460 | GDTR | base = flat-GDT GPA, limit = 0x17 (3 entries) | yes |
| 0x480 | IDTR | base=0, limit=0 (PVH: no IDT until guest builds one) | yes |
| 0x490 | TR / 0x470 LDTR | null-ish valid entries | VMLOAD/VMSAVE (TR,LDTR) |
| 0x4CB | CPL | 0 | yes |
| 0x4D0 | **EFER** | **0x1000 (SVME **only** — see §3/§5)** | yes | ⚠ **guest EFER.SVME MUST be 1** |
| 0x548 | CR4 | 0 | yes |
| 0x550 | CR3 | 0 (paging off) | yes |
| 0x558 | CR0 | **0x11 (PE + ET), PG=0** | yes |
| 0x570 | RFLAGS | 0x2 (reserved bit 1) | yes |
| 0x578 | RIP | PHYS32_ENTRY (P05) / smoke-blob GPA (M1) | yes |
| 0x5D8 | RSP | 0 (guest sets its own) | yes |
| 0x5F8 | **RAX** | 0 | **yes — VMRUN loads guest RAX from here, #VMEXIT saves it back** |
| 0x648 | CR2 | 0 | yes |
| 0x680 | **G_PAT** | 0x0007040600070406 (power-on default) | yes | **required when NP_ENABLE=1** — a zero G_PAT is illegal |

**M1 note:** for the smoke blob the entry state is even simpler — CS/DS flat, CR0=0x11, RIP=blob GPA.
`RBX` = `hvm_start_info` GPA is a **P05** concern, not M1.

---

## 2. The world-switch sequence (annotated pseudo-code)

### 2a. What VMRUN / #VMEXIT do automatically vs what the asm stub must hand-manage

This is the crux of the whole phase. Get the partition wrong → silent host corruption.

| State | VMRUN saves host / loads guest? | #VMEXIT restores host / saves guest? | ⇒ Stub responsibility |
|-------|---------------------------------|--------------------------------------|------------------------|
| RAX | host→HSAVE; guest←VMCB[0x5F8] | host←HSAVE; guest→VMCB[0x5F8] | **none** (read guest RAX from VMCB after exit) |
| RSP, RFLAGS, RIP | host→HSAVE; guest←VMCB | host←HSAVE; guest→VMCB | **none** |
| CS/SS/DS/ES, GDTR/IDTR | host→HSAVE; guest←VMCB | host←HSAVE; guest→VMCB | **none** |
| CR0/CR2/CR3/CR4, EFER | host→HSAVE; guest←VMCB | host←HSAVE; guest→VMCB | **none** |
| RBX,RCX,RDX,RBP,RSI,RDI,R8–R15 | **NOT touched** | **NOT touched** (still hold guest values on exit) | **save host before, load guest before VMRUN; save guest after, restore host after** |
| **FS.base, GS.base, KernelGSBase** | **NOT touched by VMRUN** | **NOT touched** | **VMSAVE host + VMLOAD guest before; VMSAVE guest + VMLOAD host after** — ⚠ the GS.base leak |
| **LSTAR/STAR/CSTAR/SFMASK/SYSENTER_*** | NOT touched | NOT touched | **same VMSAVE/VMLOAD pair** (host syscall entry breaks otherwise) |
| TR, LDTR | NOT touched | NOT touched | VMSAVE/VMLOAD pair |
| x87/SSE/MXCSR, DR0-3, XCR0 | NOT touched | NOT touched | eager save/restore in MVP (smoke blob uses none; still snapshot in §7) |

> **The single most dangerous line in the whole plan:** VMRUN does **not** save/restore GS.base or
> KernelGSBase. Cellos reads CPU-local via `gs:16` (kernel_rsp, `context.rs`/trap.rs). If a Linux guest
> sets GS.base for its own per-CPU data and the stub skips the VMSAVE/VMLOAD pair, the **first host
> instruction after #VMEXIT that touches `gs:` reads guest garbage** → wrong kernel stack → triple
> fault, or worse, silent aliasing. The ARM reference had no analog (TPIDR_EL2 is EL2-private and the
> guest can't touch it — `vcpu.rs:351`). **On x86 this is a real cross-world leak and MUST be a
> VMSAVE/VMLOAD pair (or explicit RDMSR/WRMSR of 0xC0000100/0101/0102) around VMRUN.**

**VMSAVE/VMLOAD register set (APM §15.5.2):** FS, GS, TR, LDTR (sel+base+limit+attr), KernelGSBase,
STAR, LSTAR, CSTAR, SFMASK, SYSENTER_CS/ESP/EIP. Use a dedicated **host-state VMCB page** (call it
`host_vmcb`, distinct from `VM_HSAVE_PA` and from the guest VMCB) as the VMSAVE target.

### 2b. Host→guest→host, annotated (SVM). Maps onto `run_vcpu_impl` (`vcpu.rs:160`).

```
// registry.rs x86 branch of run_vcpu()  →  hal::x86_64::svm::run_vcpu_impl(vcpu, budget_ns)
// ─────────────────────────────────────────────────────────────────────────────
fn run_vcpu_impl(vcpu, budget_ns) -> HalVmExit {

  // ── 1. (ARM step 2a analog) arm the budget timer BEFORE entry ───────────────
  //     ARM used the native VMX-preemption-timer equivalent (CNTV); SVM has none.
  arm_host_oneshot_timer(budget_ns)      // §6 — LAPIC/HPET one-shot; INTR intercept already set
  //     ↑ if budget_ns == 0 or "run to natural exit", skip.

  // ── 2. update per-entry VMCB control fields (the only writes each switch) ────
  if vcpu.pending_inject.valid { write VMCB.EVENTINJ = vcpu.pending_inject; clear it }
  write VMCB.TLB_CONTROL = FLUSH_ASID            // MVP; optimize with clean-bits later

  // ── 3. THE ASM STUB (world_switch.rs). Everything below is the naked fn. ─────
  //     Pass: RDI = &guest_vmcb_pa (or the VMCB PA), RSI = vcpu.host_save_ptr,
  //           RDX = host_vmcb_pa (VMSAVE/VMLOAD target).
  svm_vmrun(guest_vmcb_pa, vcpu, host_vmcb_pa)   // returns after #VMEXIT

  // ── 4. (ARM step 4 analog) host EL1-bank restore — on x86 done by VMLOAD in the
  //     stub, so nothing here. CPU-local (gs:) is valid again.

  // ── 5. read exit fields (ARM read ESR/ELR/FAR/HPFAR; here read VMCB) ─────────
  code  = VMCB.EXITCODE ; i1 = VMCB.EXITINFO1 ; i2 = VMCB.EXITINFO2
  nrip  = VMCB.nRIP     ; intinfo = VMCB.EXITINTINFO

  // ── 6. budget check (ARM had Preempted via preemption timer) ─────────────────
  if code == VMEXIT_INTR && budget_deadline_passed() {
      ack_and_eoi_host_timer_irq()               // §6 — consume the physical IRQ
      return HalVmExit::Preempted
  }
  if code == VMEXIT_INTR {                        // some OTHER host IRQ, not budget
      dispatch_through_host_idt(acked_vector)     // §6 — re-enter, not surfaced to cell
      // (loop / re-VMRUN handled by caller run loop)
  }

  // ── 7. re-inject in-flight event (no ARM analog — x86-specific) ──────────────
  if intinfo.valid { vcpu.pending_inject = intinfo }  // event was interrupted by the exit

  // ── 8. decode → HalVmExit (§4). Carry nrip for PC-advance (P05). ─────────────
  return decode_svm_exit(code, i1, i2, nrip, &vcpu.gpr)
}
```

```
// world_switch.rs — the naked asm stub. SVM. (Law 4: unsafe + // SAFETY:)
// This is the x86 analog of vcpu_enter_guest (vcpu.rs:339) — but note there is NO
// separate trap trampoline: #VMEXIT returns to the instruction after VMRUN.
svm_vmrun(RDI=guest_vmcb_pa, RSI=vcpu*, RDX=host_vmcb_pa):
    ENDBR64                                  // CET-IBT landing pad (boot.rs:80 convention)

    push  callee-saved (RBX,RBP,R12-R15)     // ABI: preserve for the Rust caller
    push  RSI ; push RDX                     // keep vcpu* and host_vmcb_pa across VMRUN

    // 2b.1  save host FS/GS/KernelGSBase/syscall-MSRs/TR/LDTR
    mov   RAX, host_vmcb_pa
    VMSAVE                                    // host → host_vmcb  (RAX = host_vmcb_pa)

    // 2b.2  load guest FS/GS/... from guest VMCB
    mov   RAX, guest_vmcb_pa
    VMLOAD                                    // guest FS/GS/... ← guest_vmcb

    // 2b.3  load guest GPRs the CPU does NOT auto-load (all except RAX)
    mov   RBX,[RSI+gpr_rbx] ; RCX,RDX,RBP,RSI(careful!),RDI, R8..R15 ← vcpu.gpr
    //     ↑ load RSI LAST-but-one and stash vcpu* somewhere reachable, OR keep
    //       vcpu* on the stack (we pushed it) and reload after. Classic KVM
    //       pattern: vcpu* lives at a fixed stack slot; guest RBX..R15 loaded,
    //       then RAX = guest_vmcb_pa for VMRUN.

    mov   RAX, guest_vmcb_pa                  // VMRUN operand = guest VMCB PA
    VMRUN                                     // ── GUEST RUNS ── returns here on #VMEXIT
    //     On return: RAX = host RAX (from HSAVE), RBX..R15 = GUEST values,
    //     GS.base/etc = guest (VMLOADed). guest RAX already saved to VMCB[0x5F8].

    // 2b.4  save guest GPRs (they hold guest values now) — BEFORE clobbering
    //       (vcpu* must be recovered from the stack slot without using gs:!)
    mov   [stack_vcpu*].gpr_rbx = RBX ; ... R8..R15   // guest GPR snapshot

    // 2b.5  restore host FS/GS/KernelGSBase/syscall-MSRs  ← host_vmcb
    mov   RAX, host_vmcb_pa
    VMLOAD                                    // host FS/GS/... restored → gs: valid again

    pop   RDX ; pop RSI
    pop   callee-saved
    ret                                       // returns into run_vcpu_impl step 5
```

**Ordering hazard (the second silent-failure source):** between VMRUN-return and the host VMLOAD,
`gs:` is still the *guest's* GS.base. **No instruction in that window may use `gs:`** — including any
implicit CPU-local access, stack-protector check, or an interrupt (interrupts are masked here because
`VMEXIT_INTR` returns with GIF handling; keep IF=0 until VMLOAD completes). The guest-GPR save in
2b.4 must therefore reach `vcpu*` via the **pushed stack slot**, never via a CPU-local lookup.

---

## 3. VM-entry consistency-check checklist (VMRUN → VMEXIT_INVALID)

VMRUN performs canonicalization + consistency checks (APM §15.5.1). A failure yields
`#VMEXIT` with `EXITCODE = VMEXIT_INVALID (-1 / 0xFFFFFFFF_FFFFFFFF)` and **no useful sub-reason** —
this is exactly where silent bring-up death lives. Tick every box **before the first VMRUN**, and on
any `VMEXIT_INVALID`, bisect this list. `[VERIFY each against your APM revision — the set grows with
extensions.]`

**Host precondition (else #UD/#GP, not VMEXIT_INVALID):**
- [ ] `EFER.SVME = 1` on the host (set in P01 `svm::enable()`) — else VMRUN `#UD`.
- [ ] `VM_HSAVE_PA` MSR (0xC0010117) points to a valid 4 KiB host save area (P01) — else `#GP`.
- [ ] `VMRUN RAX` (guest VMCB PA) is 4 KiB-aligned — else `#GP`.

**Guest-state consistency (→ VMEXIT_INVALID):**
- [ ] **VMRUN intercept bit set** (VMCB 0x010 bit 0). Not set → `VMEXIT_INVALID`. *(most-forgotten)*
- [ ] **ASID ≠ 0** (VMCB 0x058).
- [ ] **Guest EFER.SVME = 1** (VMCB 0x4D0 bit 12). The guest doesn't know SVM exists → §5 MSR handler
      must force this bit; a fresh PVH EFER of 0 → INVALID.
- [ ] EFER reserved bits (above bit 15 region per APM) all zero.
- [ ] `CR0.CD = 0 && CR0.NW = 1` is **illegal** → INVALID. (PVH CR0=0x11 is fine.)
- [ ] `CR0[63:32]` = 0 (reserved).
- [ ] CR3 has no reserved bits set for the current paging mode.
- [ ] `CR4` reserved bits = 0.
- [ ] `DR6[63:32] = 0`, `DR7[63:32] = 0`.
- [ ] **Long-mode consistency triad:**
      - `EFER.LME=1 && CR0.PG=1` ⇒ `CR4.PAE` must be 1, else INVALID.
      - `EFER.LME=1 && CR0.PG=1 && CR4.PAE=1 && CS.attrib.L=1` ⇒ `CS.attrib.D` must be 0.
      - Effective `EFER.LMA` must be consistent with `LME & PG` (don't hand-set LMA wrong).
      - **PVH entry:** LME=0, PG=0 ⇒ triad trivially satisfied. The trap comes later (§5).
- [ ] **NP_ENABLE=1** ⇒ `N_CR3` (0x0B0) is a legal, MAXPHYADDR-bounded, aligned nested-PT root.
- [ ] **NP_ENABLE=1** ⇒ `G_PAT` (0x680) is a legal PAT value (not zero).
- [ ] If IOIO_PROT set with an IOPM base ≠ 0, `IOPM_BASE_PA` within MAXPHYADDR (12 KiB region).
- [ ] If MSR_PROT set, `MSRPM_BASE_PA` within MAXPHYADDR (8 KiB region), page actually allocated.
- [ ] EVENTINJ (if V=1) has a legal TYPE and (for exceptions) legal vector/error-code-valid combo.
- [ ] Segment attrib packing is the **AMD packed form**, not the raw access byte (silent wrong-mode).

**Diagnostic protocol on VMEXIT_INVALID:** log the full VMCB control + relevant state-save dwords;
start from a **known-good minimal VMCB** (flat CS/DS, CR0=0x11, EFER=SVME-only, ASID=1, NP off first
to isolate, then NP on) and add fields until the INVALID reproduces. TCG's `svm_helper.c` does emit a
log line on the internal consistency failure path — capture QEMU stderr with `-d int,cpu_reset`.

---

## 4. Exit-decode table (SVM EXITCODE → `ViVmExit`)

Codes from APM Appendix C (Table C-1). Cross-referenced to the P04 ABI variants
(`libs/api/src/abi/hypervisor.rs` — PortIn/PortOut/Hlt/Msr are the **new** P04 additions;
MmioRead/MmioWrite/Preempted/Shutdown already exist). P03 emits the **HAL** enum
(`hal/traits/hypervisor/src/lib.rs`); P04 freezes the `#[repr(C,u8)]` ABI mirror.

| EXITCODE | Value | `ViVmExit` variant | EXITINFO1 / EXITINFO2 decode |
|----------|-------|--------------------|------------------------------|
| VMEXIT_IOIO | 0x7B | **PortIn / PortOut** (P04) | EXITINFO1: TYPE bit0 (0=OUT→PortOut, 1=IN→PortIn); STR bit2; REP bit3; SZ8/16/32 bits 4/5/6 → size; A16/32/64 bits 7/8/9; **PORT = bits[31:16]**. EXITINFO2 = RIP of next instr (PC advance). ⚠ handle STR/REP for INS/OUTS (P05 8250 UART is single-byte OUT — plain path) |
| VMEXIT_MSR | 0x7C | **Msr** (P04) | EXITINFO1 bit0: 0=RDMSR, 1=WRMSR. MSR index in guest **ECX**; value in **EDX:EAX** (`vcpu.gpr`). §5 EFER special-case here |
| VMEXIT_NPF | 0x400 | **MmioRead / MmioWrite** | EXITINFO1: P(0),RW(1)=write,US(2),RSV(3),ID(4 instr-fetch); bit32=fault during guest-PT walk, bit33=final. **EXITINFO2 = faulting GPA.** size/reg/direction from decode-assist bytes (VMCB 0x0D0) if NRIPS/DecodeAssists present, else fetch+decode the instr from guest RAM. RW bit → MmioWrite vs MmioRead |
| VMEXIT_HLT | 0x78 | **Hlt** (P04) | none. PC already past HLT (nRIP). Guest idle → run loop may inject IRQ or yield |
| VMEXIT_CR0_WRITE | 0x10 | (internal → §5, not surfaced) | with DecodeAssists: EXITINFO1[3:0]=source GPR#; else decode MOV CR0. Used only for the IA-32e flip; re-enter, don't surface to cell |
| VMEXIT_INTR | 0x60 | **Preempted** (if budget) OR internal re-dispatch | no VMCB payload; the acked vector comes from the host LAPIC/PIC. Budget deadline → Preempted; else host IRQ → dispatch via IDT + re-enter (§6) |
| VMEXIT_VMMCALL | 0x81 | **Hvc-equivalent** (hypercall) | guest RAX/RBX/... carry hypercall args (`vcpu.gpr`). P05 uses for PVH/paravirt; M1 unused. Advance PC by nRIP |
| VMEXIT_CPUID | 0x72 | (internal — emulate) | emulate CPUID in VMM (mask vendor/features), write guest EAX-EDX, advance nRIP, re-enter. Not surfaced to cell |
| VMEXIT_SHUTDOWN | 0x7F | **Shutdown** | triple-fault / shutdown condition → tear down VM |
| (any other) | — | **Unknown{reason,qual}** | catch-all: log EXITCODE + EXITINFO1/2; run loop treats as fatal (mirror ARM S1PTW `Unknown` policy, `phase-03` m1 guard) |

**PC-advance policy (mirror `vcpu.rs:278` ARM advance table):** for HLT/IOIO/MSR/CPUID/VMMCALL the
trapping instruction is *consumed* — advance guest RIP to **nRIP** (VMCB 0x0C8) if NRIPS is supported,
else advance by the decoded instruction length. For NPF (MMIO) the VMM emulates the access and sets
RIP explicitly (the cell decides, same as ARM MMIO). **Do not blanket-advance** — an NPF advanced past
its instruction silently drops the memory access.

**NPF-vs-real-fault guard (x86 analog of ARM's S1PTW guard, `phase-03` m1):** if EXITINFO1 bit32=1
(fault occurred while walking a *guest* page table, not the final access), the GPA in EXITINFO2 is a
guest-PT address, **not** an MMIO target — do NOT dispatch as MMIO; treat as `Unknown`/fatal.

---

## 5. The IA-32e CR0.PG trap dance (SVM specifics)

**The critical SVM/VT-x divergence, stated up front:** on **SVM there is no "IA-32e mode guest" entry
control**. The processor derives long mode from `CR0.PG & EFER.LME` *during* guest execution and sets
EFER.LMA itself. Therefore **SVM does NOT strictly need to trap `CR0.PG` 0→1** — the guest can do its
entire 32-bit-protected → long-mode transition inside a single VMRUN, and the *next* VMRUN's
consistency check passes because by then LME=1, PG=1, PAE=1 are mutually consistent. The plan's
cross-cutting invariant ("trap CR0.PG, flip entry control") is a **VT-x requirement (§8)**; on SVM the
CR0-write intercept is needed only for the **EFER.SVME preservation problem**, not the mode flip.

**What actually must be handled on SVM — the EFER.SVME dance:**

```
PVH entry state:      CR0=0x11 (PG=0), EFER=0x1000 (SVME only, LME=0)
guest kernel does:    (a) build page tables in guest RAM
                      (b) WRMSR EFER, EDX:EAX = (LME|NXE|...)   ← guest omits SVME (bit12)
                      (c) MOV CR4, ...|PAE
                      (d) MOV CR0, ...|PG                        ← PG 0→1

If EFER WRMSR is intercepted (MSR_PROT + MSRPM bit for 0xC0000080 set):
   → VMEXIT_MSR (0x7C), EXITINFO1 bit0 = 1 (write), ECX = 0xC0000080, value in EDX:EAX
   → HANDLER (§4 Msr path, but EFER-special):
        new_efer = (EDX:EAX)  |  EFER_SVME(bit12)     // ← FORCE SVME back in
        VMCB.state_save.EFER[0x4D0] = new_efer
        advance RIP to nRIP ; re-enter
   Rationale: without the OR-in, VMCB guest EFER loses SVME → next VMRUN → VMEXIT_INVALID (§3).

If EFER WRMSR is NOT intercepted:
   → processor writes guest EFER shadow directly; AMD keeps SVME transparent to the guest
     (guest RDMSR EFER cannot observe/clear the host-managed SVME). ← [VERIFY under TCG:
     svm_helper.c EFER handling — some emulators do NOT auto-preserve SVME. If TCG lets the
     guest clear SVME, you MUST intercept + force it. Test this in the P03 spike.]

CR0.PG transition (SVM):
   Option A (recommended MVP): DO intercept CR0 writes (VMCB 0x000 bit16). On VMEXIT_CR0_WRITE:
        - copy the new CR0 value into VMCB state-save CR0 (0x558)
        - (EFER.LMA is auto-derived by the CPU on next entry; no manual flip needed)
        - advance nRIP ; re-enter.
        Benefit: a single choke-point to validate CR0 bits (reject illegal CD/NW combos → §3)
        and to co-locate the EFER.SVME re-assertion. Cost: one extra exit per boot.
   Option B: DON'T intercept CR0; let the guest transition freely. Simpler, fewer exits.
        Risk: no choke-point to catch an inconsistent CR0/CR4/EFER that would VMEXIT_INVALID
        on the *following* entry — harder to diagnose. Prefer A for bring-up, switch to B once green.
```

**VMCB state transitions across the dance (Option A, SVM):**

| Step | CR0 (0x558) | CR4 (0x548) | EFER (0x4D0) | Exit produced |
|------|-------------|-------------|--------------|---------------|
| entry | 0x11 (PG=0) | 0 | 0x1000 (SVME) | — |
| after EFER WRMSR | 0x11 | 0 | 0x1500 (SVME+LME+NXE) ← SVME forced | VMEXIT_MSR |
| after CR4 write (if intercepted; else silent) | 0x11 | PAE | 0x1500 | (VMEXIT_CR4 or none) |
| after CR0.PG write | 0x80000011 (PG) | PAE | CPU sets LMA→0x1D00 | VMEXIT_CR0_WRITE |
| next VMRUN | consistency triad holds (LME&PG&PAE, LMA consistent) → **enters long mode** | | | |

---

## 6. SVM budget / preemption (no preemption timer)

**Problem:** ARM used the native virtual-timer / VMX uses the native preemption timer (exit reason
52). **SVM has neither.** `sys_run_vcpu(budget_ns)` (the shipped ABI, `registry.rs:182`) must still
yield a synchronous `Preempted` exit so the run loop can service cell IPC (VFS/Net) and respect the RT
watchdog.

**Design (host one-shot timer + INTR intercept):**

```
create_vcpu (once): VMCB.intercept INTR (0x00C bit0) = 1
                    VMCB.V_INTR_MASKING (0x060 bit24) = 1
                    // ⇒ physical interrupts are governed by HOST EFLAGS.IF during guest run,
                    //    NOT the guest's virtual IF. Guest cannot mask our budget timer.

run_vcpu_impl(budget_ns):
   if budget_ns > 0:
       deadline = now() + budget_ns
       program_host_oneshot(deadline)      // LAPIC timer one-shot (TSC-deadline or count),
                                           //   or HPET one-shot on no-LAPIC hosts. Kernel-side.
   VMRUN
   on #VMEXIT:
       if EXITCODE == VMEXIT_INTR:
           vector = host_ack_interrupt()   // "acknowledge interrupt on exit" analog; on SVM read
                                           //   the pending vector from LAPIC/PIC ISR
           if vector == BUDGET_TIMER_VEC && now() >= deadline:
               EOI(vector)
               return Preempted            // ← the synchronous yield the ABI promises
           else:
               dispatch_through_host_idt(vector)   // real host device IRQ; re-VMRUN in run loop
       ... else normal decode (§4)
   cancel_host_oneshot()                    // disarm if we exited for another reason first
```

**TCG viability analysis (the open question, plan lines 148-150):**
- TCG runs the SVM guest via `target/i386/tcg/system/svm_helper.c` in a software translation loop.
  Between translation blocks it checks `cpu->interrupt_request`. An emulated LAPIC/PIT timer IRQ sets
  `CPU_INTERRUPT_HARD`; with the INTR intercept active and V_INTR_MASKING governing via host IF, the
  helper's intercept check (`cpu_svm_check_intercept_param` / the INTR path) **should** raise a
  `VMEXIT_INTR` before the next block executes. **Expected to work**, but TCG's interrupt-delivery
  latency is coarse (block granularity, not instruction), so `budget_ns` fidelity under TCG is
  *approximate* — fine for a yield point, not for hard-RT accounting (which is a real-HW concern
  anyway, mirroring the existing bench "QEMU TCG caveat").
- **Failure mode if it doesn't fire:** a compute-bound guest (`jmp .`) never HLTs and never exits →
  the vCPU thread spins forever → RT watchdog kills the owning cell. That is *detectable* (watchdog
  log), not silent — good.

**The P03 spike (must prove before P05 run loop depends on it):**
```
Spike test (test-hooks, SVM/TCG):
   guest blob = `1: jmp 1b`   (infinite loop, NO hlt, NO I/O — nothing else can cause an exit)
   arm host one-shot for ~1 ms
   t0 = wall_clock()
   exit = run_vcpu_impl(budget_ns = 1_000_000)
   assert exit == VMEXIT_INTR-derived-Preempted
   assert wall_clock() - t0 is bounded (e.g. < 100 ms TCG slack)
Pass  ⇒ budget path is real; build the P05 run loop on it.
Fail  ⇒ fallback ladder:
   (1) HLT-yield only: rely on guest HLT (VMEXIT_HLT) as the yield point. Works for idle guests
       (Linux HLTs in its idle loop) — sufficient for M2 boot-to-shell; a busy guest overruns.
   (2) Trap-flag single-step: set guest RFLAGS.TF for a #DB after N instructions — slow, last resort.
   (3) Real-HW LAPIC one-shot on the KVM lane (P09) — the budget timer is validated there regardless.
```

---

## 7. Host-state-corruption failure modes + the 1000× snapshot-equality test

**What a leak looks like (each is silent under a liveness-only test):**

| Leak | Symptom | Why liveness misses it |
|------|---------|------------------------|
| GS.base / KernelGSBase not restored | `gs:` CPU-local reads guest value → wrong kernel_rsp | kernel may limp for a while if guest GS.base happened to alias a mapped page; corrupts later |
| LSTAR/STAR/SFMASK not restored | next `syscall` from any cell jumps to guest's LSTAR | no syscall happens *during* the run loop → looks fine until a cell syscalls |
| A guest GPR (e.g. R12) leaked into host | host callee-saved register holds guest value | Rust caller may not read R12 immediately → corruption surfaces arbitrarily later |
| DR7/DR6 leaked | host debug breakpoints altered | invisible unless debugging |
| MXCSR / x87 CW leaked | host FP rounding/exceptions change | invisible until host does FP |
| CR2 leaked | a later host #PF reads wrong fault address | only matters if host faults |
| RFLAGS.DF leaked | host `rep movs` runs backwards | VMRUN restores RFLAGS from HSAVE, so *should* be safe — assert anyway |

**Test design (mirror `smoke_guest.rs:155` `run_register_isolation`, with x86 teeth):**

```
run_register_isolation_x86():
   // 1. HOSTILE guest — actively clobbers everything a leak could carry:
   guest blob:
       mov all GPRs to 0xDEAD... sentinels
       wrgsbase / WRMSR GS_BASE       = 0xBAD0_GS
       WRMSR KernelGSBase             = 0xBAD0_KGS
       WRMSR LSTAR                    = 0xBAD0_STAR
       wrfsbase / WRMSR FS_BASE       = 0xBAD0_FS
       set DR7 = garbage ; set MXCSR = garbage
       hlt                             // → VMEXIT_HLT, one clean round-trip
   // A benign all-zero guest would NOT exercise the leak — the sentinels are the point.

   // 2. snapshot HOST state that VMRUN does NOT auto-manage:
   snap = { RDMSR GS_BASE, KernelGSBase, FS_BASE, LSTAR, STAR, CSTAR, SFMASK,
            DR7, MXCSR, CR2, and the callee-saved GPRs the stub hand-manages }

   // 3. 1000× round-trip:
   for i in 0..1000:
       exit = run_vcpu_impl(&vcpu, budget=0)
       assert exit == Hlt
       vcpu.rip = blob_start           // reset PC (mirror ARM vcpu.g_elr_el2 reset, smoke:184)

   // 4. assert bit-equality:
   cur = { same reads as snap }
   assert cur == snap, field by field   // ANY mismatch = a world-switch leak → fail the merge
```

**Why equality, not liveness:** the ARM comment (`smoke_guest.rs:150-153`) says it exactly — "host
shell liveness alone proves scheduling continues but NOT that the world-switch is balanced; a single
leaked sysreg write goes undetected without this explicit snapshot comparison." On x86 the highest-value
snapshot targets are **GS.base + KernelGSBase + LSTAR** — those are the ones VMRUN silently leaves to
software and the ones whose corruption is both catastrophic (CPU-local, syscall entry) and invisible
until much later.

---

## 8. VT-x delta (for P09 — kept separate)

Same 8 concerns, VMCS/VMLAUNCH mechanics. **Compiles in P03, real bring-up on the KVM/HW lane (P09).**

**8.1 Layout — VMCS (vs VMCB):** VMCS is **opaque** — no documented byte offsets; every field is
accessed by `VMWRITE`/`VMREAD` with an encoded field ID (SDM Vol 3D Appendix B). First dword = VMCS
revision ID (`IA32_VMX_BASIC[30:0]`, MSR 0x480). Lifecycle: `VMCLEAR`(vmcs_pa) → `VMPTRLD`(vmcs_pa) →
populate → `VMLAUNCH`. Four field classes: guest-state, host-state, control, read-only (exit info).
Unlike VMCB there is no create-once/write-each split by *memory region* — you VMWRITE control +
host-state once, guest-state per-vCPU-create, and only re-VMWRITE the few per-entry fields.

**8.2 World-switch — VMLAUNCH/VMRESUME (vs VMRUN):**
- First entry uses `VMLAUNCH` (requires VMCS launch-state = clear); subsequent entries use `VMRESUME`
  (launch-state = launched). The stub must track this per-vCPU (a bool) — using VMLAUNCH on a
  launched VMCS or VMRESUME on a clear one fails.
- **VMX auto-saves/restores MORE host state than SVM:** the VMCS *host-state area* includes host
  RIP, RSP, CR0/3/4, CS/SS/DS/ES/FS/GS/TR selectors, **FS.base, GS.base**, GDTR/IDTR base, SYSENTER
  MSRs. ⇒ **the GS.base leak (§2) does NOT exist on VT-x** — no VMSAVE/VMLOAD dance needed. But
  LSTAR/STAR/SFMASK/KernelGSBase are NOT in the host-state area — use the **VM-exit MSR-load list**
  (or accept that Cellos host doesn't change them). Guest GPRs (incl. **RAX** — VMX does *not*
  auto-save guest RAX, unlike SVM) are ALL hand-managed in the asm stub.
- Host RIP field must point at the post-VMLAUNCH resume label; on exit the CPU jumps there directly.

**8.3 Consistency checks:** far larger (SDM Vol 3C §26.3). Failure signalled via `RFLAGS.ZF/CF` +
the **VM-instruction-error** field (`VMREAD 0x4400`) — a *numbered* error (unlike SVM's opaque
VMEXIT_INVALID), which is actually easier to diagnose. **Controls MUST be computed from the "true"
capability MSRs** (`IA32_VMX_TRUE_PINBASED_CTLS 0x48D`, `TRUE_PROCBASED 0x48E`, `TRUE_EXIT 0x48F`,
`TRUE_ENTRY 0x490`, secondary from `0x48B`, EPT/VPID cap `0x48C`) applying allowed-0/allowed-1 masks —
hard-coded control bits are the #1 VT-x bring-up failure.

**8.4 Exit decode:** `VMREAD` **exit reason** (0x4402), **exit qualification** (0x6400), **guest-
physical address** (0x2400, for EPT violations), **guest-linear** (0x640A), VM-exit instruction length
(0x440C, for PC advance). Reason map: I/O = 30, EPT violation = 48, HLT = 12, RDMSR = 31, WRMSR = 32,
external-interrupt = 1, preemption-timer = 52, triple-fault = 2, CR-access = 28, CPUID = 10, VMCALL =
18. I/O qualification: size[2:0], direction[3], string[4], REP[5], port[31:16].

**8.5 IA-32e CR0.PG dance — this is where VT-x genuinely differs (and why the plan's invariant is
VT-x-shaped):** VMX has an explicit **VM-entry control "IA-32e mode guest"** (bit 9 of VM-entry
controls) that **must** match the guest's actual mode or entry fails the consistency check. So you
**must** trap the CR0.PG 0→1 transition: set the **CR0 guest/host mask** bit for PG + a **CR0 read
shadow**, take the **CR-access exit (reason 28)**, then VMWRITE the entry control "IA-32e mode
guest"=1 and update guest EFER.LMA/LME **before VMRESUME**. Letting it free-run → VM-entry failure
(numbered error). (On SVM, recall §5: no such control, the flip is automatic.)

**8.6 Budget/preemption — native, no hack:** arm the **VMX-preemption timer** (pin-based control
bit 6; timer value in VMCS `0x482E`, decremented in units scaled by `IA32_VMX_MISC[4:0]`); expiry →
**exit reason 52** → `Preempted`. No host one-shot timer, no INTR-intercept trick, no TCG spike (and
TCG doesn't run VMX anyway). This is strictly simpler than SVM — the SVM §6 mechanism exists *only*
because SVM lacks this timer.

**8.7 Host-corruption test:** same 1000× snapshot-equality test, but the snapshot set shrinks — VMX
auto-restores FS.base/GS.base via host-state area, so the highest-risk items become **LSTAR/STAR/
SFMASK** (if not in the MSR-store list) and the **hand-managed guest GPRs incl. RAX**. Still run it —
a bug in the asm GPR save/restore is equally silent.

**8.8 Shared vs vendor-split (Law 7 boundary):** the **exit decoder** (§4 reason→ViVmExit) and the
**control-computation-from-capability-MSRs** discipline are the two places to keep a single shared
code path across SVM/VMX where possible; VMCS-vs-VMCB field access, VMLAUNCH-vs-VMRUN, and the
CR0.PG-flip-vs-automatic mode transition are the genuine vendor forks.

---

## Cross-cutting risk register (P03-specific, beyond phase-03.md table)

| Risk | L×I | Mitigation | Owner file |
|------|-----|------------|------------|
| GS.base/KernelGSBase leak (silent CPU-local corruption) | **High×Crit** | VMSAVE/VMLOAD host pair around VMRUN; §7 snapshot test asserts equality; no `gs:` use between VMRUN-return and host VMLOAD | `world_switch.rs` |
| `gs:` touched in the VMRUN-return→VMLOAD window (incl. implicit/IRQ) | Med×Crit | recover `vcpu*` via pushed stack slot only; keep IF=0 until VMLOAD done | `world_switch.rs` |
| VMEXIT_INVALID silent bring-up death | High×High | §3 checklist ticked before first VMRUN; force guest EFER.SVME; VMRUN-intercept bit; ASID≥1; G_PAT≠0; known-good-minimal-VMCB bisect | `vmcb.rs` |
| SVM budget path dead under TCG | Med×High | §6 spike **before** run loop; fallback ladder (HLT-yield → real-HW P09) | `world_switch.rs` |
| NPF-during-guest-PT-walk misdispatched as MMIO | Med×High | EXITINFO1 bit32 guard → Unknown (x86 analog of ARM S1PTW) | `vmexit_decode.rs` |
| MMIO size/reg wrong when DecodeAssists absent under TCG | Med×Med | prefer VMCB decode-assist bytes; fall back to a tiny instr decoder; Unknown-log on unhandled encodings | `vmexit_decode.rs` |

## Open questions carried to implementation

1. **Does TCG's `svm_helper.c` auto-preserve guest EFER.SVME when the guest WRMSRs EFER?** If yes, §5
   Option-A EFER intercept is belt-and-suspenders; if no, it is **mandatory**. Resolve in the P03 spike
   (add an EFER-clear probe to the budget spike blob).
2. **Is NRIPS / DecodeAssists advertised by `-cpu qemu64,+svm`?** (CPUID `8000_000A` EDX bits.) Governs
   whether §4 PC-advance and MMIO decode can use VMCB fields or must decode instructions. Probe in P01
   test-hook and record.
3. **VMSAVE/VMLOAD vs manual RDMSR/WRMSR for the host GS/syscall MSRs under TCG** — VMSAVE/VMLOAD is
   cleaner but confirm TCG implements them for the fields we need; if partial, fall back to explicit
   MSR save/restore of 0xC0000100/0101/0102 + LSTAR/STAR/SFMASK.
