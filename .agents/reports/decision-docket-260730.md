# Cellos Decision Docket — 2026-07-30

Consolidated from [spec-unresolved-inventory-260730.md](spec-unresolved-inventory-260730.md)
(450 lines, specs 00→20 vs code) and [plan-inflight-inventory-260730.md](plan-inflight-inventory-260730.md)
(344 lines, 76 plan dirs). Every item is a **pick-one or yes/no question**, not a
recommendation to implement. Answer these and the architecture description becomes
internally consistent; most follow-ups are then mechanical edits.

**Standing rule for this docket**: where a spec and the code disagree, the question is
*which one is wrong* — not "make the code match". Several specs describe a better system
than the one built; several describe a worse one.

---

## Part 0 — Open follow-ups (actions, not decisions)

Findings that need work rather than a ruling. Kept here because they were surfacing as prose
inside analysis sections, where one of them had already gone stale within hours — the exact
failure Spec 21 §2 puts status in a generated file to avoid.

| # | Action | Why it matters | Evidence |
|---|---|---|---|
| **A1 — DONE 2026-07-31** | Parse the DTB memory node on RISC-V instead of `FALLBACK_MEMORY_MAP` | RV64 now builds a bounded, reservation-safe map from the effective DTB and falls back on rejection. The 2 GiB gate reports more than **2.10 GB managed**; host fixtures and focused boot gates pass. A fresh full serial suite timed out, recorded without inflating the verdict. | `a1-dtb-runtime-260731.md` |
| **A2 — DONE 2026-08-01** | Give cell-spawn `OutOfMemory` its own syscall error and log the failed allocation | The four cell-spawn calls now encode OOM as additive `-2`; generic errors remain `-1`. Runtime exhaustion proves typed decoding, bounded source and caller/path logs, no panic, and shell recovery. | `a2-a3-test-260801.md` |
| **A3 — DONE 2026-08-01** | Add a MemInfo syscall; make `memory_footprint` measure something | Opt-in `MemInfo=243` (allowlist bit 56) returns the fixed 32-byte `ViMemInfoV1` from exact transition-aware frame accounting. The benchmark measures **135,782,400 bytes (129.49 MiB)** allocator-committed and honestly fails `<10 MiB`. | `a2-a3-test-260801.md` |
| **A4 — DONE 2026-07-31, GAPS RECORDED** | Re-run the runtime gates phases **09 and 11** left open | Phase 11 is runtime verified. Phase 09's incomplete-policy strip path, complete-policy zero-event path, and three architecture shell lanes pass. ARM packages `periph-demo` but not `sensor-demo`/`robot-demo`; a fresh full serial RV64 verdict also remains unavailable after timeout. | `a4-runtime-gates-260731.md` |

The A2/A3 execution plan `.agents/260731-1930-capacity-observability/plan.md` is complete.
The ABI package received both required confirmations before implementation. Reducing the measured
129.49 MiB allocator commitment below the unchanged `<10 MiB` objective is a separate follow-up.

---

## Part 1 — Five blocking decisions

Nothing else can be made consistent until these land.

### D1. ~~Is `fast_ipc` the architecture, or a named exception?~~ — **RULED 2026-07-31**

> **Ruling (user, 2026-07-31): Spec 17 is the model of record. `fast_ipc` is to be
> rewritten for Tier 1 — not restored.**
>
> Full analysis and measurements: `d1-fast-ipc-analysis-260731.md`.

**Why the question was malformed.** Spec 17 §11.4 "Direct (non-`ecall`) service calls" was
ratified 2026-07-30 — Spec 17 does not compete with fast_ipc, it *governs* it, and already
binds a fast handler to "authorize exactly as its `ecall` counterpart does". So "model of
record" was settled before the docket asked.

**Why the old code cannot simply be re-enabled** (all verified statically):
- Two disjoint copies of the handler state — VFS registers into `ostd`'s static, clients
  read their own; the shell's pointer is always null and every call falls back.
- The bridge the module doc describes does not exist: `resolve_export` has one occurrence
  repo-wide (its own definition), `R_RISCV_JUMP_SLOT` is a constant in a dead-code module,
  and there is no `loader::dynsym`.
- The kernel's `call_vfs` executes `csrrci sstatus` and is documented S-mode-only; cells run
  U-mode (`task.rs:749`, SPP=0) and kernel `.text` carries no USER flag. A U-mode cell
  cannot reach it.

**What measurement changed.** Measured on QEMU TCG / RV64 (p50 per op): request encode
259 ns, reply decode 359 ns, bare `ecall` 1 861 ns, **full typed round trip 46 674 ns** —
independently corroborated by the repo's own `ipc_send_recv` at 48 500 ns. Marshalling is
**1.3 %** of a round trip; the cost is the rendezvous (two traps, two context switches, a
scheduler round trip). With a 10 µs handler the saving from running the handler on the
caller's thread is ≈ **82 %**; at 1 µs ≈ 98 %. An order of magnitude, not a
micro-optimisation — and the prior analytical guess ("the saving is serialization") was
wrong.

**Binding constraints on any rewrite** (not measurable by benchmark, all still open):
1. Privilege — a U-mode caller cannot execute kernel code containing privileged CSR access;
   a cell→cell call needs the dispatch table in a shared USER page, which removes the
   kernel's identity oracle.
2. Tier 1 only — unrepresentable for Tier 2 (Spec 18): the handler is another cell's text,
   unmapped in a domain page table, and `DataPtr` cannot be dereferenced across domains.
3. Real time — the current design holds interrupts off for the whole handler; max
   interrupt-off duration becomes a release-gating number, not a footnote.

**Consequent edits applied 2026-08-01:** the 2–3-cycle figure in `00-context.md:185` and
`16-rustc-tcb.md:230` must cite the measured round trip or be marked aspirational — it
currently compares an unrunnable path against seL4's measured numbers. `resolve_export` and
`R_RISCV_JUMP_SLOT` are scaffolding for a bridge that does not exist and should not be left
as-is. Per Spec 21, whatever survives takes an `impl` or `absent` anchor.

### D1b. IPC p99 misses its PDR target — **RULED 2026-08-01**

`ipc_send_recv` p99 = 86.6 µs against the 50 µs PDR target; the suite itself reports FAIL
(p50 = 48.5 µs passes). `context_switch` p50 = 36.4 µs. Independent of D1's ruling.
**Ruling:** 50 µs p99 is a qualified-hardware target. Scheduled QEMU records the same metric
and gates sustained relative regression; a QEMU miss is `HW-TARGET-MISS`, not a release-gate
`FAIL`. The workflow had in fact gated it through a broad `grep FAIL`; that contradiction is
removed without inventing a new absolute TCG ceiling. Evidence:
`d1b-ipc-target-semantics-analysis-260801.md`.

### D2. ~~Tier 2 — shipped, accepted-unbuilt, or the sole future answer?~~ — **RULED 2026-07-31**

> **Ruling (user, 2026-07-31): option (b) — accepted-but-unbuilt. Spec 18 does not contradict
> `security-model.md`; it adds a containment tier. Amend both documents and the item closes.**

**Correction to this docket's own framing.** D2 below called `security-model.md:74` a direct
contradiction of Spec 18. That was overstated. `:74` is an *operational warning about today*,
not a doctrinal claim that only Tier 3 can ever contain untrusted code; Spec 18 adds a tier
where an unsigned cell keeps the Tier-1 cell shape and SDK and is contained by the MMU instead
of by trust. The two are layered, not opposed.

**What the amendment had to fix beyond adding text.** Tier 2 does not exist in code — verified
again on this branch: one `KERNEL_ROOT` (`kernel/src/memory/paging.rs:38`) and no
`satp`/`TTBR0`/`CR3` write in any context switch. Five documents had begun stating Tier 2 in
the present tense, so adding the tier alone would have told a reader that untrusted native
code is safe to run today — the opposite of the intended advice.

**Applied:**
- `docs/security-model.md:74` — warning re-scoped to name *both* future containment tiers,
  with the code evidence that neither exists, and an explicit note that Spec 18 narrows this
  warning later rather than reversing it.
- `docs/security-model.md:~90` — "hardware isolation available **today** is Tier 3"; Tier 2
  marked accepted-but-unimplemented.
- `docs/specs/18-cell-trust-tiers.md` §2 — tier table gains a **Status** column
  (shipped / accepted-NOT-implemented / shipped-aarch64), plus a paragraph stating the tier
  *adds* an option and Tier 3 remains operative until the mechanism ships.

**Also reconciled:** the 2026-06-05 decision that per-Cell SATP is "explicitly NOT pursued"
applies to **Tier 1**, where a per-cell page-table switch would destroy zero-copy IPC. Tier 2
pays that cost deliberately, and only for unverified code. Both documents now say so, so the
next reader does not read it as an argument against Tier 2.

### D2 (original framing, superseded)

`security-model.md:74` says **"Do NOT run untrusted third-party code until Tier 3 VM is
implemented"**. Spec 18 (accepted today) says Tier 2 = unsigned native cell in a private
page-table domain, and five docs now repeat the Tier-2 sentence in present tense. Code:
one shared root table (`memory/paging.rs:34`), no `satp`/`TTBR0`/`CR3` write in any
context switch — Tier 2 does not exist.

**Pick one:** Tier 2 is (a) shipped, (b) accepted-but-unbuilt with Tier 3 as today's only
answer for untrusted code, (c) the sole future answer, superseding the "Tier 3 only"
statements. (b) and (c) differ in whether `security-model.md:74` stays as a current
warning or is rewritten.

### D3. One kernel-LOC definition and one owning document — **RULED 2026-08-01**

Six numbers in normative docs (5,600 / 7,200 / 11.5K / <10K / <6000 / 22,600). Spec 15 is
Ratified and its "Current ~5,600" row is false under every definition below.

**Measured on `4df193a6` (branch `feat/wx-post-reloc-and-f1-signing`), `kernel/src`,
excluding `third_party`:**

| Definition | Lines |
|---|---|
| All `.rs`, raw lines (103 files) | **31 189** |
| …excluding `*test*.rs` files (2 694) | 28 495 |
| **nLOC** (no comments, no blank lines) | **20 394** |
| nLOC excluding tests | **18 494** |
| nLOC excluding tests, drivers and hypervisor — *"core"* | **14 679** |

By subtree (raw): `task` 14 145 (of which `task/drivers` 4 300) · `memory` 3 616 ·
`loader` 2 280 · `hypervisor` 1 491 · `cell` 1 152 · `fs` 490 · `boot` 390.

Notes for whoever writes the number down: the earlier inventory's 27 856 was measured on
`main` before this branch's reactor/CQ and W^X work, so the figure moves per commit — which
is the argument for generating it (Spec 21 Layer 3) rather than writing it in prose. Raw
lines overstate by ~35 % versus nLOC because this codebase comments heavily by policy.

**Ruling:** generated nLOC excluding `*test*.rs` is canonical; a second core lens also excludes
`task/drivers/**` and `hypervisor/**`. `docs/code-metrics.generated.md` is the sole moving-number
owner and CI checks it. The fixed ≤5,000 target is withdrawn in favour of Spec 15 responsibility
gates plus generated trend evidence. Evidence: `d3-kernel-loc-ownership-analysis-260801.md`.

### D4. Instant-On snapshot — **VERIFIED 2026-07-31: shipped; the contradiction is spurious**

The answer is (a), and the reason the docs looked contradictory is that the two lanes never
meet.

**Snapshot is real and wired.** `kernel/src/snapshot.rs` (411 lines): serialize via syscall
`Snapshot = 420` (`libs/api/src/abi/syscall.rs:137`, allowlist bit 32 → SpawnCap);
restore called from the boot path at `kernel/src/main.rs:524`.

**The KASLR conflict does not exist in any built configuration.** Restore is gated
`#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]` (`main.rs:523`) — x86_64 is
explicitly excluded. And `KASLR=yes` appears only in `limine.conf`, the **x86_64** config;
`limine-vf2.conf` and `limine-pioneer.conf` (the RISC-V boards) both set `KASLR=no`. So the
one lane with physical ASLR is the one lane where snapshot is not compiled in. The
"no physical ASLR" prerequisite is satisfied wherever the feature exists.

**The Metadata Registry prerequisite is also stale**: snapshot ships without it, so
`03-runtime.md:96-102` describes a gate the feature already passed by another route.
`03 §4.5` is stale text (option (a)) — but see the hazard below before deleting it wholesale.

**New finding — the safety rests on a coincidence, not a check.** Restore writes frames to
`pa_base + idx * 4096` taking `pa_base` **verbatim from the snapshot header**
(`snapshot.rs:229`, `:270-276`), and `allocator.memory_start()` appears only in the
*serialize* path (`:100`) — restore never compares the saved base against the current boot's
RAM base. `kernel_hash` (`:222`) cannot cover this: under ASLR the binary is *identical*, only
its load address differs, so the hash matches and the restore proceeds. The code's own comment
states it "overwrites ALL of physical RAM including kernel `.bss`/`.data`".

Consequence: enabling snapshot on x86_64, or flipping `KASLR=yes` in a board config, turns a
warm boot into whole-RAM corruption with no diagnostic.

**GUARD APPLIED 2026-07-31.** `try_restore` now compares the recorded `pa_base`/`pa_end`
against the live frame allocator's `memory_start()`/`memory_end()` before writing a single
frame, and falls back to a logged cold boot on mismatch (or if the allocator is not ready).
The comment states why `kernel_hash` cannot cover this case: under a randomised load address
the binary is byte-identical, so the hash matches while the RAM base has moved. Typechecks on
all three arches; the W^X integration test still passes on the rebuilt image, so the boot path
is unaffected.

The invisible coupling between `#[cfg(any(riscv64, aarch64))]` and the per-board `KASLR=`
setting is now an enforced precondition rather than a coincidence — the kind of invariant
Spec 21 would anchor.

### D5. Cell-count target — **RULED 2026-08-01**

Original framing asked whether to withdraw the PDR's "1000+ Cells" in favour of Spec 19's
"≥10 000 actor-futures across ≤64 cells". User rejects that trade and is right to: the
two-level model answers "one app, many tasks" and fails "one server, many requests", and a
server is a stated target (README: Edge-to-Cloud).

**The hole in my Spec 19 §4 argument.** N futures in one cell share one heap, one quota, one
capability set — so a faulty or hostile future reads and corrupts the other N−1. That is
exactly the isolation a multi-request server needs, and the actor-future model does not
provide it. "Chase BEAM's process count" was rejected on the assumption that a cell must cost
512 KiB; the rejection does not survive once that assumption is removed.

**What actually blocks 1000+ cells today (measured on `4df193a6`):** not architecture — three
constants and one allocation policy, all changeable.

| Factor | Today | Hard limit? |
|---|---|---|
| `MAX_CELLS` | 64 (`cell_quota.rs:15`) | No — a constant; the VA allocator already sizes `MAX_SLOTS = 512` (`va_alloc.rs:48`) |
| Stack | 64 pages × 2 = **512 KiB/cell**, pre-allocated, no demand growth | No — phase 08 shrinks it; demand-paged stacks remove the pre-allocation entirely |
| Default quota | **16 MiB/cell** (`cell_quota.rs:22`) | No — a default, set per cell |
| ELF image | **full copy per spawn** (`elf.rs:273`); no text sharing between instances | **This is the real cost** for "1000 requests of one handler" |

1000 instances of a 1 MiB handler ≈ 1.5 GiB, most of it identical, immutable `.text`/
`.rodata` copies. That is waste, not a ceiling.

**Direction to pin (needs a spec, likely amending Spec 19 §3 + Spec 02):**
1. **Share `.text`/`.rodata` across instances of one image** — the highest-leverage lever,
   and SAS makes it natural now that W^X (phase 10) has made those segments read-only: N
   requests of one handler map the *same* frames and allocate only stack + heap + TCB.
   Needs image-hash frame refcounting in the loader (`va_alloc` already gives distinct
   slots; `elf.rs` does not yet share frames).
2. **Demand-paged stacks** instead of a 512 KiB pre-allocation — a light request touches a
   few KiB. Goes beyond phase 08's static per-path sizing, which still pre-allocates.
3. **Two named profiles, not one target:** *large-app* (a few big cells, MiB quota — today)
   and *per-request server* (thousands of tiny cells, shared image, demand stack, KiB
   quota — each request an isolated cell).

**Why this can beat BEAM on a dimension, not just match it:** BEAM processes share one VM and
a single NIF fault takes neighbours down; a per-request cell is separated by W^X + capabilities
+ (with Tier 2) a domain page table. "As many as BEAM" is a legitimate engineering target;
"more isolated than BEAM" is the part only Cellos can claim.

**Decide:** (a) commit the per-request server profile as a goal and open a plan for image
sharing + demand stacks (recommend); (b) keep it App-only and formally drop the 1000+ NFR.
The PDR's bare "1000+ Cells" NFR is withdrawn either way — it names a number without the
memory model that makes the number mean anything.

**MEASURED 2026-07-31** — full method and numbers in `d5-cell-scale-measurement-260731.md`.
Spawning parked cells until refusal, with `MAX_CELLS` raised to 512 and 2 GiB of guest RAM:
**refusal at n = 8** after the suite, **n = 9** on unfragmented memory (so not fragmentation).

**The binding ceiling is none of the three above — it is 190 MiB of hardcoded RAM.**
`kernel/src/boot.rs:232-250` `FALLBACK_MEMORY_MAP` declares the usable region as
`0x0BE0_0000` = 190 MiB for "RISC-V QEMU virt (256 MB)", and there is no DTB memory-node
parse: the guest's 2 GiB was never seen. That 190 MiB holds the kernel heap plus the ~14
cells init spawns (512 KiB stack + full ELF copy each), leaving room for nine more.

**Revised order of work** (Spec 19 §3 has 2 and 3 but omits 1, and had the priority wrong):
1. parse the DTB memory node instead of the fallback — cheapest, largest lever, and today
   every deployment silently discards RAM above 190 MiB;
2. share `.text`/`.rodata` across instances of one image (safe now Layer A made them
   read-only);
3. demand-paged stacks;
4. raise `MAX_CELLS`/`MAX_SLOTS` last, once the denominator has changed.

**Two defects found incidentally, both closed 2026-08-01:** cell-spawn OOM now has its additive
`-2` result and bounded diagnostics, while opt-in MemInfo now reads exact frame accounting. The
first real benchmark value is 135,782,400 bytes (129.49 MiB), an honest failure of the unchanged
`<10 MiB` objective. See `a2-a3-test-260801.md`.

**Ruling:** accept the per-request server profile and keep 1000 simultaneous isolated cells as
its qualification goal, not a claim about current capacity. The large-app profile and 64-cell
default remain. Measure N=64/128/256/512 first; qualification requires shared immutable image
frames after W^X, demand-paged stacks, profile quotas, dynamic tables, and isolation/reap proof.
The work is queued behind Midori. Evidence: `d5-cell-scale-profile-ruling-analysis-260801.md`.

---

## Part 2 — Contradictions needing a ruling (one side must be corrected)

| # | Two sources disagree | Question |
|---|---|---|
| D6 | **RULED 2026-07-31 — F1 reads "absolute outside the reviewed allowlist"; the three stale doc claims are corrected.** Verified by experiment, not by reading docs: injecting an `unsafe` block into a clean cell (`hello-cell`) makes `cellos-sign --check --strict` **fail with exit code 1** and name the file and the layer that caught it (`[token]`), so CI does go red; the file was reverted. Measured state: **58/75** cell crates carry the attribute; the check itself reports 77 crates / 348 files scanned with `unsafe` confined to **46 allowlisted files**; allowlist holds 72 entries with `class`/`reason`/`approver`/`date`; F5 confirms rustc `f53b654a8882` matches the pinned `nightly-2026-05-01`. The allowlist is stronger than the phase-11 plan required — `review_by` + `max_age_days = 90` report overdue entries, and entries whose file no longer contains `unsafe` are reported so the list tightens. Ruling rationale: declaring the 46 reviewed files "non-compliant" would create a meaningless state — documented, approved exemptions labelled violations, and Tier-1 admission refusing the very drivers needed to boot. **Applied:** `security-model.md:14` and `:57` (both still named the twice-retired `cargo-geiger`), `pdr:157` and `:533` ("✅ Zero unsafe code in Cells" was simply false), `00-context.md:219`, and Spec 16 F1 — each now points at `scripts/unsafe-allowlist.toml` as the source of truth and states that an entry **is** a hole in the LBI wall. Spec 16 F1 also directs readers to obtain the tally from `cellos-sign --check` rather than restating it (the three numbers above move every commit — Spec 21 Layer 3). | ~~original framing below~~ |
| D6-orig | **F1 enforcement — partly answered by implementation on 2026-07-30.** Spec 16 F1 + `00-context` say `forbid(unsafe_code)` is absolute; `security-model.md:14,57` and `pdr:533` still claim a cargo-geiger gate and "zero unsafe in Cells". Reality now: geiger is gone, the unsafe ratchet that replaced it is **also gone** (deleted), and the gate of record in both workflows is `python3 scripts/cellos-sign --check --strict` with the rule + allowlist in `scripts/unsafe-allowlist.toml`. Compliance moved from 16 to 51 crates; shell went 36 unsafe → 2 (`cmd_fs.rs`). | The mechanism question is settled — the remaining ruling is textual: does F1 now read "absolute outside the reviewed allowlist" (and `security-model.md:14,57` + `pdr:533` get corrected), or are allowlisted cells formally out of compliance and Tier-1 admission must say so? |
| D7 | **RULED 2026-07-31 — the code caught up to the spec; §5 stays as current state and gains its limits.** The docket assumed §5 was a false claim to demote into a target. Phase 10 inverted that: `loader/elf.rs:145-162` now derives `final_flags` from `p_flags` and adds WRITE only for the load window, and `task.rs:732` calls `wx::enforce` after relocation — so Text=RX / Data=RW / rodata=R is accurate. **Runtime-verified for the first time 2026-07-31** (phase 10 had shipped unbooted): `tests/integration/tests/wx-text-write.rs` 2/2 PASS — a cell storing to its own `.text` faults, is terminated, and the kernel keeps scheduling; the `boot` suite is 54/54 PASS on the same image, so nothing regressed. Verified from a detached worktree at `4f11e6ae` using the unblocked QEMU lane. **Applied to `02 §5`:** it now states the load-window-then-lower ordering, points at Spec 19 §2 Layer A for the mechanism (per Spec 21 — policy here, mechanism there, no duplication), and adds the three limits that are limits *of the guarantee*: (1) code integrity only — stack/heap/grant/MMIO stay USER+RW across cells, so cross-cell **data** needs Layer B; (2) no cross-hart TLB shootdown — `protect_page` invalidates the calling hart only, a real window on SMP; (3) bare-physical arches (riscv32 Nano, x86_32, arm32) have no page tables and `wx::enforce` logs the gap. | — |
| D8 | **RULED 2026-08-01 — §10 is Draft/reserved-but-unbuilt; `0x11`/`0x12` remain reserved.** Keeping an absent mechanism Ratified violates Spec 21 and conflates design approval with runtime availability. Releasing the values would discard reviewed design history and invite byte-0 collisions. No enum or implementation change is authorized; the 2026-07-23 Law-1 confirmation #1 remains historical and confirmation #2 is required immediately before any future ABI edit. Applied in Spec 17, the G4 roadmap status, and ADR 0001. | — |
| D9 | **VERIFIED 2026-08-01 — RK3588 is Armv8.2-A and has no MTE.** Rockchip specifies quad Cortex-A76 + quad Cortex-A55. Arm's TRMs state that both cores fully implement Armv8.2-A and define `ID_AA64PFR1_EL1[63:8]` as `RES0`, which includes the MTE field `[11:8]`. Arm's MTE white paper identifies FEAT_MTE with Armv8.5-A. Therefore Spec 19's page-table premise is correct; the stale RK3588 MTE claims in system architecture, roadmap, security model, research, and changelog are corrected. Generic MTE code remains valid only on QEMU or future Armv8.5+ hardware. Evidence: `research-260801-0715-rk3588-architecture-and-mte.md`. | — |
| D10 | **RULED 2026-08-01 — phased activation accepted.** Keep the tested RedoxFS mount as G1/QEMU proof-of-function through `VicellDisk -> blk_router -> BLOCK_DRIVER`; do not represent it as G2-qualified. G2 production status requires an automated RedoxFS-on-NVMe write/read/persistence test, a defined and measured `<100 us` filesystem-read benchmark, approved purchasable hardware, and an explicit P5 authorization decision. ADR 09b is amended; ADR 0002 records the rationale and rejected alternatives. Evidence: `d10-srv-backend-analysis-260801.md`. | — |
| D11 | **RULED 2026-08-01 — rewrite the graphics contract; no VFS edit required.** `06-graphics §5` now distinguishes compositor API ownership checks from Tier-1 memory isolation: Grant-backed surfaces remain `USER+RW`, so arbitrary direct data access does not fault and relies on LBI plus the trusted signed-cell boundary. Actual PTE faults cover W^X code/rodata, guard pages, and unmapped addresses; the handler terminates into zombie/reap rather than setting `CellState::Poisoned`. Tier-2 per-domain page tables remain the hardware wall for untrusted native cells. The cited `09-vfs` sentence was already absent. Evidence: `d11-page-fault-protection-claims-analysis-260801.md`. | — |
| D12 | **RULED 2026-08-01 — Spec 19 owns the Layer A/B/C hardware-isolation taxonomy.** Spec 05's stale MTE/MPK/PMP table is replaced by a Spec 19 pointer: Layer A W^X is implemented, Layer B per-domain page tables are the future/load-bearing native-code wall, and Layer C is opportunistic only. Spec 16 no longer calls MTE/MPK Spectre mitigations; Spec 12 retains the correct S-mode/PMP constraint. The ruling also closes the hidden PKU status contradiction: CR4.PKE, task PKRU values, and WRPKRU return paths exist, but PTE bits `[62:59]` are never stamped, every user page remains key 0, and the self-test checks constants + kernel `RDPKRU` rather than a denied access. Specs 10/15 and the roadmap/changelog/architecture/security-model status claims are corrected. No runtime code or ABI changed. Evidence: `d12-hardware-supplement-set-analysis-260801.md`. | — |
| D13 | **RULED 2026-08-01 — recommendation A approved and applied.** Signing exists, but production signed-only admission does not. All spawn sources converge on the Ed25519 gate, while `/bin/` remains path-scoped authorization rather than provenance. Default features leave `signing-required` off, the public dev seed and unchecked-dev route are test fixtures, no production key-provisioning path exists, and signature status does not select a memory tier. Specs 12/18, the security model, roadmap, architecture, and changelog now distinguish default G1/dev admission from future fleet-secure admission. A real key/profile/artifact-provenance/negative-test/secure-boot gate is required before claiming "Tier 1 = signed only." No runtime, ABI, key, or feature change was authorized. Evidence: `d13-tier1-signature-admission-analysis-260801.md`. | Closed. |
| D14 | **RULED 2026-08-01 — recommendation A approved and applied.** The scheduler row is now fixed-priority with three tiers, FIFO within tier, RT-hart routing on RV64, and architecture-scoped immediate preemption; the TLSF row now says the 256 KiB pool is initialised but unused, stacks still use the frame allocator, and Cellos has no end-to-end TLSF WCET qualification. Phase 25 remains historical. | Closed — docs-only ruling, no runtime/ABI change. |
| D15 | **RULED 2026-08-01 — recommendation A approved and applied.** `06-graphics.md` now routes input through the focus-gated, kernel-mediated queue path and links to Spec 17 §6; the direct/no-queue wording was withdrawn. | Closed. |
| D16 | **RULED 2026-08-01 — recommendation A approved and applied.** `system-architecture.md`, `project-roadmap.md`, `project-changelog.md`, and Spec 20 now use transitional four-state wording; no doc claims a two-node runtime or a fully shipped forwarder. | Closed. |
| D17 | **RULED 2026-08-01 — recommendation A approved and applied.** `00-fork.md`, `11-shell.md`, `00-context.md`, `code-standards.md`, and the README now treat `viFS1`/`viFS2` as retired names and keep `VIFS1` for BootFS/initramfs. | Closed. |

---

## Part 3 — Underspecified mechanisms (direction agreed, nothing pinned)

| # | Item | Question |
|---|---|---|
| D18 | **Metadata Registry** — withdrawn in favor of focused registries. `02-memory.md` now owns the invariants through the pin registry, grant tables, and resource registry; `03`/`07`/`08` must cite those specific owners. | **RULED 2026-08-01 — recommendation A approved and applied.** The monolithic registry is withdrawn. |
| D19 | **`catch_unwind`** — required by `01-core.md:43-47`, `10-testing.md:21`, `00-fork.md:98`. Code: absent and *impossible* in no_std abort-on-panic. `12-reliability.md:87-91` already flags it. | **RULED 2026-08-01 — recommendation A approved and applied.** Replace unwind recovery with terminate-and-supervise. |
| D20 | **`sys_grant` stub.** `libs/ostd/src/syscall.rs:855` returns `Err(Unknown)`. Spec 12 §4.4 concludes "leak-free by construction" **partly because** runtime grant creation is impossible. But `00-context.md:188` and `system-architecture.md:893` list the Grant API as implemented. | **RULED 2026-08-01 — recommendation A approved and applied.** Grants are reachable; delete the dead wrapper and re-derive the safety text from the active grant lifecycle. |
| D21 | **Layer B ADR ownership.** Spec 19:41-42 requires an ADR on grant mapping + Spec 17 wire contract before Tier 2; Spec 18:98-103 says `DataPtr`/`GetFile` raw pointers are unrepresentable across the tier boundary. | **RULED 2026-08-01 — recommendation A approved and applied.** Make raw-pointer removal a Tier-2 prerequisite; defer the full ADR to the Layer-B implementation window. |
| D22 | **Kernel boundary violations in specs.** `09-vfs.md:74` puts page-cache eviction in the kernel (vs Spec 15 §3.4 no-policy); `04-hardware.md:38-41` specifies a kernel Resource-Graph deadlock detector (absent; shipped detectors are watchdog + heartbeat); `04:35-36` SMP work-stealing is scaffolding only. | **RULED 2026-08-01 — recommendation A approved and applied.** Eviction belongs in VFS-cell policy, deadlock detection is watchdog/heartbeat, and two-hart work stealing is implemented with RT exclusion. |
| D23 | **Certification lane.** Spec 16 F7: no safety claims for RISC-V until Ferrocene qualifies it (12–24 months); Ferrocene adoption point is "before G2 production on ARM64 (RK3588)". PDR:103 says riscv64 is the **primary** build target. | **RULED 2026-08-01 — recommendation A approved and applied.** Split development/reference and certification lanes; ARM64 is the first qualification candidate. |
| D24 | **Spec 20 ratification order.** Spec 20 is non-normative but *amends* the Ratified Spec 17, and depends on four absent things (attested sender, broker-scoped watch primitive, `CellAddr` types, yielding broker handshake). | **RULED 2026-08-01 — recommendation A approved and applied.** Keep Spec 20 Draft and approve zero Law-1 ABI additions now. |
| D25 | **`machine_id` binding.** Spec 14 (canonical constants doc) says "lower machine_id wins Primary" with no binding requirement; `enrollment.rs:68-76` decodes it from the wire unbound → **spoofable to win Primary after every partition heal**. Spec 20 requires NodeId-derived machine_id. | **RULED 2026-08-01 — recommendation A approved and applied.** Bind `machine_id` locally to the authenticated NodeId now. |

---

## Part 4 — Stale claims (code contradicts the doc)

Mechanical corrections once acknowledged — except **D26**, which is a real hole.

- **D26 — RULED 2026-08-01 (A). ViUI v2 had no current spec.** `14-viui.md:3` said "awaiting G2 implementation" and still
  described egui/iced compatibility facades. The shipped design (Reactive Signal Tree +
  `ViNode` + `vi_design!`/`.vi`) now owns Spec 14 in place.
- **D27 — RULED 2026-08-01 (A).** Spec 15 §3 hard-coded kernel-residue LOC for hotswap, snapshot, and `pcie_ecam`.
  That belongs in generated status, not the normative spec; the boundary is partial, not
  fully migrated.
- **D28 — RULED 2026-08-01 (A).** `12-reliability.md:77` said heartbeat was "only net adopts it so far"; six
  binaries / 13 source files call `sys_heartbeat` today. The score table should reflect
  adoption without freezing a hard count.
- **D29 — RULED 2026-08-01 (A).** `05-application.md:349` called x86 hypervisor an "ENOSYS stub"; current code
  splits AMD SVM MVP, Intel VMX root-operation plumbing, and unsupported RISC-V H-ext.
  The spec should split backend evidence instead of using one binary label.
- **D30 — RULED 2026-08-01 (A).** Entropy: `system-architecture.md:869` described a predictable fallback on the
  default dev profile, while Spec 17 says production use must fail closed without
  `dev-weak-rng`. Keep both profiles explicit.
- **D31 — RULED 2026-08-01 (A).** `09-vfs.md:49` put littlefs at "G1 tail"; littlefs is shipped for `/data`, and
  the remaining gate is real-board repeated power-cut qualification.
- **D32 — RULED 2026-08-01 (A).** The PDR self-contradicts on ARM/x86 status, VFS state, coverage, reproducible
  builds, and codebase counts. It should move to evidence-based wording.
- **D33 — RULED 2026-08-01 (A).** `10-testing.md:16-17` specified **SASan** (Single Address Space Sanitizer) as a
  live layer. No such tool exists; replace it with actual existing gates.

---

## Part 5 — Plan portfolio

The original inventory counted 76 plan directories, but its COMPLETE/OPEN totals mixed
stale checkboxes with source reality. D34-D39 replace those frozen counts with the
evidence-based scheduling index in `.agents/plan-portfolio.md`.

### False-completion claims (verified)

- **`260624-cell-to-cell-anywhere`** marks P00–P03 "✅ COMPLETE"; `net-broker/src/main.rs:150-155`
  `dispatch()` is three TODO comments and `routing.rs:154-157` returns `self_tid` for every
  remote lookup — **remote calls terminate locally and are dropped.** Spec 20 §1 already
  records this correctly.
- **`260528-2016-vicell-full-implementation`** self-admits the same shape at `plan.md:22`:
  "~75% by functional tests, **100% by file existence**".
- **Correction to an earlier claim of mine**: `260712-1903-thread-cellid-quota-fix` is **not**
  stale — `plan.md:4` reads `status: done (kernel-side)` with a closure note dated 2026-07-27,
  and midori's plan already strikes the dependency. I had briefed this as stale; it was fixed
  three days after the midori plan was written.
- **`260605-1406-phase28-wasm-cells-epmp` is dead, not late** — no WASM crate in the tree,
  commit `8607a16e` removed WASM from the docs, ePMP is M-mode-blocked.

### Cross-plan conflicts

1. **`/bin` writability** — midori `plan.md:70-73` locks `/bin` per-cell and forbids
   `allow_write_all`; `260712-1000/phase-01-writable-cell-store.md:11` wants gated write into
   that same overlay. Precedence was decided for midori but **written in only one place** —
   the pkg-dist phase file has no reciprocal note.
2. **grant-reap double rewrite** — midori phase 07 and cap-revocation phase 02 both touch
   reclaim. Pin/quarantine foundation has landed; D36 now makes Midori mechanism/ordering
   authoritative and revocation the trigger/policy owner.
3. **Supervisory dependency correction** — P-TRUST landed in `721e1f6f`; supervisory
   Phase 00 is technically unblocked but queued by the D39 WIP limit.

### Consolidation questions

- **D34 — RULED 2026-08-01.** Close/supersede the four overlapping ViUI plans; preserve
  provenance instead of concatenating phase files. `260616-0755` is the closed record.
- **D35 — RULED 2026-08-01.** Keep manifest, revocation, and DICE as separate child
  plans under one Trust & Identity portfolio group; their "all unstarted" premise was stale.
- **D36 — RULED 2026-08-01.** Write reciprocal `/bin` and pin-aware grant-reclaim
  precedence notes into both sides.
- **D37 — RULED 2026-08-01.** Reject blanket defer by checkbox age; use evidence-based
  active/queued/deferred/completed/retired triage in `.agents/plan-portfolio.md`.
- **D38 — RULED 2026-08-01, corrected after source review.** WASM remains tracked, so
  its plan is partial/suspect (disposition unresolved) while ePMP is M-mode-blocked;
  Cell-to-Cell Anywhere is partial (foundation complete, integration blocked).
- **D39 — RULED 2026-08-01.** Midori is the sole active feature program until runtime
  closure of 02 and completion of 04/07/08, with narrow P0 security/CI/verification exceptions.

---

## Part 6 — Closure sweep for the remaining open blockers

- **D1 consequents — APPLIED 2026-08-01.** Normative/living docs no longer advertise the
  unreachable 2–3-cycle path. Unreferenced `resolve_export`/`R_RISCV_JUMP_SLOT` scaffold is
  removed; retained tables are explicitly inactive pending the ruled Tier-1 rewrite.
- **D1b — RULED 2026-08-01.** 50 µs p99 is hardware qualification; QEMU owns trend/regression
  evidence and no longer fails the hardware ceiling directly.
- **D3 — RULED 2026-08-01.** Generated nLOC excluding test files is canonical, with a separate
  core lens. Frozen totals and the ≤5,000 target are withdrawn.
- **D5 — RULED 2026-08-01.** The 1000-cell per-request profile is accepted with explicit
  prerequisites and staged gates; current defaults are unchanged and implementation is queued.

---

## Suggested order for a 3-day window

1. **Day 1 — facts before preferences.** D9 (RK3588 hardware fact), D30 (entropy), D25
   (machine_id hole), D20 (grant reachability), D6 (F1 mechanism of record). These are
   verifiable, and several are security-relevant.
2. **Day 2 — the five blocking rulings.** D1, D2, D3, D4, D5. D1 first: it decides what the
   IPC story *is*, which Spec 20 and every performance claim inherit.
3. **Day 3 — portfolio + description holes.** D26 (ViUI spec hole), D34–D39 (plan
   consolidation), then the mechanical stale-claim edits (D27–D33) delegated in one pass.

Items not on the critical path for code: D11, D14, D15, D17, D28, D31, D32 — pure doc
corrections, safe to batch to a docs agent once the rulings above exist.
