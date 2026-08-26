# Adversarial ABI-Freeze Review — `ViVmExit` VERSION=2 CC-Neutrality

**Scope:** analysis only, no code. Gate: freezing `ViVmExit` VERSION=2 (P04) under Law 1 must not
preclude Confidential-Computing (CC) guests — TDX / SEV-SNP (x86) and ARM CCA/RME (ARMv9.3) — per
roadmap §"Confidential computing for Tier 3" (`docs/project-roadmap.md:336`).

**Sources audited:** `libs/api/src/abi/hypervisor.rs:17-42` (frozen enum, VERSION=1),
`hal/traits/hypervisor/src/lib.rs:10-20` (HAL mirror), `libs/api/src/abi/syscall.rs:280-298`
(`RunVcpu` = 223, `VcpuRegs` = 224, out-param `*mut ViVmExit`),
`.agents/260711-1917-tier3b-x86-vtx/phase-04-vmexit-abi-registry.md` (P04 append of disc 8-11).

**The CC threat model (the invariant every finding is measured against):** under TDX/SEV-SNP/CCA,
guest RAM is encrypted and the host (kernel + VMM-cell) **cannot read arbitrary guest GPA, nor read
the guest register file**. The hardware/firmware mediator (SEV **GHCB** via `#VC`, TDX
**TDG.VP.VMCALL**, CCA **RMM** `RmiRecExit`) delivers to the host ONLY the values the guest
*explicitly published*. Any exit-ABI field that can only be filled by the host dereferencing guest
memory or peeking a guest GPR is CC-unsafe.

---

## 1. Variant-by-variant CC audit (disc 0-11)

Classification: **VALUE** = field is a value the guest can explicitly deliver (CC-safe shape);
**INDEX** = carries a register/target index only, no guest-RAM deref, but implies a host-side
register touch to *complete* (CC-tolerable if completion is abstracted — see §4); **NONE** =
payload-less (trivially CC-safe); **SYNDROME** = architectural exit code from hardware/RMM, not
guest-RAM derived (CC-safe).

| Disc | Variant | Fields | Class | CC verdict |
|------|---------|--------|-------|-----------|
| 0 | `MmioRead` | `ipa,size,reg` | VALUE(ipa,size)+INDEX(reg) | Safe shape — see §2 |
| 1 | `MmioWrite` | `ipa,size,val` | VALUE | Safe shape — see §2 |
| 2 | `Hvc` | `imm,regs:[u64;8]` | VALUE **if provenance held** | Safe *only under a doc invariant* — see below |
| 3 | `Wfi` | — | NONE | Safe |
| 4 | `SysReg` | `op0..op2,rt,is_write` | INDEX(rt), **no value field** | Not reusable for CC; harmless dead variant — see below |
| 5 | `Preempted` | — | NONE | Safe (budget, no guest state) |
| 6 | `Shutdown` | — | NONE | Safe |
| 7 | `Unknown` | `ec,iss` | SYNDROME | Safe (fatal-path syndrome, no deref) |
| 8 | `PortIn` | `port,size,reg` | VALUE(port,size)+INDEX(reg) | Safe shape (GHCB IOIO-in / TDVMCALL\<IO\>) |
| 9 | `PortOut` | `port,size,val` | VALUE | Safe (val delivered via GHCB/TDVMCALL) |
| 10 | `Hlt` | — | NONE | Safe (TDVMCALL\<HLT\> / GHCB idle) |
| 11 | `Msr` | `index,is_write,val` | VALUE | Safe (GHCB MSR proto / TDVMCALL\<MSR\> both deliver `val`) |

**`Hvc { imm, regs: [u64;8] }` (`hypervisor.rs:25`) — flagged.** The 8-GPR envelope is *wide enough*
and CC-compatible in principle: ARM CCA's `RSI_HOST_CALL` and SEV/TDX hypercall paths all copy a
**bounded, guest-published** set of registers into the shared exit structure. The danger is not the
layout — it is the doc at `hypervisor.rs:14` ("the kernel writes via `*mut ViVmExit` … SAS shared")
combined with the ARM non-CC implementation habit of reading x0-x7 out of the saved vCPU context.
That habit is "host snapshots arbitrary guest GPRs," which is exactly what CCA/TDX forbid. The fix is
a **provenance invariant in the frozen header**, not a layout change: `regs[]` carries *only the
registers the guest declared as hypercall arguments*, never an opportunistic host-side GPR peek.
Zero code cost; converts an implicit CC-hostile assumption into an explicit CC-safe contract.

**`SysReg { …, rt, is_write }` (`hypervisor.rs:29`) — flagged.** This is the *only* variant that
both (a) carries a target-register INDEX (`rt`) and (b) carries **no value field**, so completing it
*requires* the host to read (write=1) or write (read) guest GPR `rt`. Under CCA, host-side
system-register emulation essentially does not exist — the RMM owns timer/`CNTP_*` and most trapped
sysregs — so `SysReg` will simply **never be emitted to a Realm guest**. It is therefore a harmless
dead variant under CC (same status the plan already assigns it on x86, `phase-04:33`). The
freeze-relevant conclusion: **do not attempt to reuse `SysReg` for any future CC sysreg path** — its
shape can't carry the value in-struct. A CC sysreg exit, if ever needed, is a NEW append-only
variant. No action beyond a one-line "ARM-non-CC only; not CC-reusable" note.

**`MmioRead.reg` / `PortIn.reg` (INDEX).** Both name a guest destination register for the read
result. Under CC the host cannot deposit into a guest GPR; the value round-trips via the shared page
(`#VC`/GHCB scratch, TDVMCALL). This is a *completion-path* concern (§4), not an exit-struct shape
break — `reg` becomes an ignored hint under CC. Append-only-safe.

---

## 2. The MMIO problem (highest risk) — verdict: shape SURVIVES, assumption must be pinned

The worry is that `MmioRead`/`MmioWrite` (disc 0/1), which P04 *reuses* for x86 EPT-violation / NPF
(`phase-04:31`), bake in "the host decodes the faulting guest instruction to recover
size/register/value" — precisely the ISV=0 / VMX-EXIT-qualification path that TDX cannot support
(the host may not read guest RIP bytes from encrypted RAM).

**Finding: the ABI *shape* is CC-neutral; the *fill mechanism* is what differs, and it differs
below this ABI.** Decisive detail: the variants carry only **already-decoded fields**
(`ipa/gpa`, `size`, `reg`, `val`) — they do **not** carry a raw instruction pointer, instruction
bytes, or a "go decode this" handle. Contrast the CC-hostile design that would be a hard blocker:
an exit that carried `rip` or `insn: [u8; 15]` and expected the cell to fetch/decode from guest RAM.
`ViVmExit` does not. So:

- **ARM (disc 0/1):** the header already promises **ISV=1** (`hypervisor.rs:20,22`) — the stage-2
  syndrome hardware-decodes size/SRT/direction with **no** instruction fetch. Non-decoded faults are
  routed to `Unknown`(7) and treated fatal (`hypervisor.rs:34-36`). This is *already* CC-favorable:
  Cellos never host-decodes an ARM guest instruction.
- **x86 (disc 0/1 reused for EPT violation):** here the **non-CC** VMM *does* decode the guest
  instruction to fill `size`/`reg`/`val` — but that decode is an implementation step **inside the
  kernel/registry x86 arm** (`phase-04:47-49`), not a field of the ABI. Under TDX/SEV-SNP, MMIO
  never arrives as an EPT violation at all: the guest's `#VC` handler (SEV-ES+) or
  `TDG.VP.VMCALL<MMIO>` (TDX) does the decode *inside the guest* and hands the host
  `{gpa, size, direction, value}` through the shared page. The kernel then fills the **same**
  `MmioRead`/`MmioWrite` fields from GHCB/TDVMCALL instead of from an instruction decoder. Same
  struct, different source. **No ABI break.**

**Required to lock this in (zero code):** a frozen-header invariant stating that
`MmioRead`/`MmioWrite`/`PortIn`/`PortOut` fields are *always explicitly-delivered decoded values*
(hardware syndrome on ARM non-CC; in-guest decode via GHCB/TDVMCALL under CC) and that **no exit
variant will ever carry a guest instruction pointer or raw instruction bytes for host-side decode.**
This makes the CC-neutrality a written contract rather than an accident of the current field list.

**Secondary (pre-existing, not a freeze blocker):** `MmioWrite.val: u64` and `PortOut.val: u32` cap
transfer width at 8/4 bytes — a 16-byte (XMM) or REP-string MMIO can't be represented. This is a
limitation on non-CC too and irrelevant to Cellos's target device set (UART/PIC/PIT ≤ 4 bytes). Note
it; do not fix at freeze.

---

## 3. Append-only discipline + the size envelope

**Discriminants:** `#[repr(C, u8)]` with explicit `= N` values (`hypervisor.rs:21-36`) guarantees a
future `GhcbRequest`/`TdVmCall`/`AttestedLaunch` at disc 12+ needs **no renumbering** of 0-11.
Confirmed safe. The `_`-less matches (e.g. `run_loop.rs:46`, `phase-04:34`) will fail to compile on a
new variant — a *loud, gated* signal, not a silent break. Good.

**Size envelope — this is the real freeze boundary, not the discriminants.** With `#[repr(C, u8)]`
the layout is `{ u8 tag; <pad to 8>; union of variant structs }`. The largest variant is
`Hvc { imm: u16, regs: [u64;8] }` → payload struct 72 B (align 8) → total **≈80 B**. All new
x86 variants (≤ 16 B) fit; `size_of::<ViVmExit>()` is unchanged and the plan's `const_assert`
(`phase-04:58,98`) holds. **BUT** `RunVcpu` validates the out-buffer at exactly
`size_of::<ViVmExit>()` (`syscall.rs:280-282`, plan's `validate_user_buf`, `phase-04:37`). Therefore:

- **Any future CC variant whose payload exceeds the ~80 B Hvc envelope grows `size_of`, which
  changes the `validate_user_buf` size, which overflows the 80-B buffer every already-shipped
  hypervisor cell compiled against VERSION=2.** That is a hard compat break for all G2 hypervisor
  cells.
- The concrete trap: TDX `TDG.VP.VMCALL` can carry up to ~13 guest GPRs (R10-R15, RBX, RSI, RDI,
  …) and SEV GHCB delivers a valid-bitmap plus a register set. If a future designer **inlines** that
  register file into a CC exit variant, the payload is ~104 B > 80 B → size bump → break.

The mitigation is a design invariant, not headroom padding (see §5): CC exits must carry a
**shared-region reference** (`ghcb_gpa: u64` + small metadata), never an inline register file — the
host reads the actual registers from the GHCB/TDVMCALL shared page (which it *is* permitted to read,
because it is explicitly shared/decrypted). Such a reference-shaped variant is < 24 B and fits the
80-B envelope forever. This must be stated at freeze, because the `const_assert` only makes the break
*loud* — it does not prevent a well-meaning CC author from choosing the inline shape and then
"fixing" the assert.

---

## 4. Syscall boundary — by-value is correct for CC; no indirection seam needed

`sys_run_vcpu` returns `ViVmExit` **by value** into a cell-owned buffer (`syscall.rs:281`,
`out_ptr: *mut ViVmExit`). Question: does CC's GHCB/TDVMCALL round-trip force a "shared bounce
region" indirection into the exit ABI now?

**No — and adding one would be over-engineering.** The `ViVmExit` struct is a **host-internal
kernel→VMM-cell artifact**; it is never guest-encrypted memory. Under CC the kernel reads the GHCB /
TDVMCALL shared page (permitted — it is decrypted/shared), extracts the published values, and fills
`ViVmExit` by value for the cell. The cell never touches encrypted guest RAM. The GHCB round-trip
lives entirely *below* this ABI, as a kernel/registry implementation detail. By-value therefore
survives CC unchanged.

**Completion path (the subtle part):** to return an MMIO/PortIn read result, the cell today writes
guest GPR `reg` via `VcpuRegs` (224) then re-enters `RunVcpu`. Under CC the kernel cannot write the
encrypted GPR — but `VcpuRegs`'s ABI is the *logical* "set guest register rt = val"; under CC the
kernel transparently routes that deposit into the GHCB scratch / TDVMCALL return slot instead of the
physical GPR. **`VcpuRegs` is already the abstract completion channel; its ABI is CC-neutral and
needs no new syscall or field at freeze.** Document this as a future-CC implementation note, not an
ABI change. (For ≤ 8-byte MMIO — Cellos's whole device set — the register-shaped deposit maps
cleanly to a GHCB scratch write.)

Conclusion: **append-only variants suffice; do NOT add a shared-bounce-region field or indirection
to the exit ABI.** The only forward-looking reservation worth making is the *size/shape* invariant of
§3, which costs nothing.

---

## 5. Verdict

**GO-WITH-CHANGES** — freeze VERSION=2 as P04 specifies (append disc 8-11, bump VERSION 1→2,
`const_assert` on size). The append is CC-neutral in shape: every new variant carries values
in-struct (`PortOut.val`, `Msr.val`) or is payload-less (`Hlt`), and no variant carries a
host-dereferenceable guest pointer or a decode-this-instruction handle. The three required changes
are **doc-only, zero-code**, and cost nothing now while permanently protecting the CC path:

1. **Field-provenance invariant** (frozen header, `hypervisor.rs:16`): every `ViVmExit` field is a
   value the guest **explicitly delivered** (ARM ISV=1 syndrome / SEV GHCB / TDX TDVMCALL / CCA
   RMM), never a host snapshot of arbitrary guest GPRs or guest RAM. Explicitly: `Hvc.regs[]` =
   hypercall arguments the guest published; `MmioRead.reg`/`PortIn.reg` = guest destination hint only
   (value returned via `VcpuRegs`, which under CC routes through the shared page); **no variant will
   ever carry a guest RIP or raw instruction bytes.**
2. **CC-exits-use-indirection invariant** (frozen header): a future CC exit variant MUST carry a
   *shared-region reference* (`ghcb_gpa` + small metadata), never an inline guest register file, so
   it stays inside the ~80 B `Hvc` envelope and never grows `size_of::<ViVmExit>()`. This is the one
   line that keeps the `const_assert` from becoming a landmine.
3. **Discriminant-name reservation** (comment): reserve disc 12-15 for the CC class
   (e.g. 12=`GhcbRequest`, 13=`TdVmCall`, 14=`AttestedLaunch`) so no non-CC exit later claims them.

**Explicitly NOT recommended** (would be over-engineering per the "preserve the seam only"
directive): a shared-bounce-region field, a reserved padding variant to pre-grow the envelope, or a
new completion syscall. `VcpuRegs` already abstracts completion; by-value `ViVmExit` is correct;
append-only + the two invariants cover CC.

### The single most-likely break-forcer if left as-is

**Not a discriminant — the pinned size envelope.** The `const_assert` on `size_of::<ViVmExit>()`
(the ~80-byte `Hvc { regs: [u64;8] }` envelope, `hypervisor.rs:25`) is the freeze boundary that will
break first: the first CC variant that **inlines** a guest register file (TDX `TDG.VP.VMCALL` ≈ 13
GPRs ≈ 104 B, or a SEV GHCB register set) exceeds 80 B, grows `size_of`, and overflows the
`validate_user_buf(out_ptr, size_of::<ViVmExit>())` buffer of every hypervisor cell already shipped
against VERSION=2. Change #2 (require CC exits to reference the shared page, not inline registers)
neutralizes this at zero cost. If only one line survives review, it is that one.
