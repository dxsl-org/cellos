# Cellos Spec Inventory — Unresolved & Self-Contradictory Architectural Decisions

**Date**: 2026-07-30 · **Scope**: `docs/specs/00-context`…`20`, `docs/security-model.md`,
`docs/system-architecture.md`, `docs/project-overview-pdr.md` · code cross-checked against
`kernel/`, `libs/`, `cells/`, `hal/`, `scripts/`.

Each entry = the decision at stake, the conflicting evidence, and a yes/no or pick-one
question. Nothing below is a recommendation to implement.

---

## BLOCKING — decide these first; other specs cannot be made consistent until they resolve

### B1. Which tier is the answer for untrusted third-party native code?

- `docs/specs/18-cell-trust-tiers.md:41` — "Invariant: **there is no 'unverified native code
  inside the shared SAS view' tier.**" Tier 2 = unsigned native ELF behind a private page table.
- `docs/specs/18-cell-trust-tiers.md:94-97` — "absent/invalid → domain mapping (new; requires the
  per-domain page-table mechanism of Spec 19 §2). Until that mechanism ships, unsigned cells in
  production posture are refused".
- `docs/security-model.md:74` — "**Do NOT use Cellos to run untrusted third-party code until
  Tier 3 VM is implemented.**"; `:281-282` — "Untrusted third-party code belongs in **Tier 3**.
  Tier 2 runs unsigned native cells in a private MMU protection domain" (present tense).
- `docs/specs/05-application.md:19`, `docs/specs/12-reliability.md:52`,
  `docs/system-architecture.md:957,970` — all repeat the Tier-2 sentence in present tense.
- Code: **ABSENT.** One shared root table only — `kernel/src/memory/paging.rs:34`
  `static KERNEL_ROOT: Spinlock<Option<PhysAddr>>`; no `satp`/`TTBR0`/`CR3` write in any context
  switch (`hal/arch/riscv/src/rv64/asm/switch.S`, `hal/arch/arm/src/aarch64/context.rs`,
  `hal/arch/x86/src/x86_64/context.rs`).

**Decide:** Is Tier 2 (a) a shipped capability, (b) an accepted-but-unbuilt design, or (c) the
*sole* future answer for untrusted native code — superseding the "Tier 3 only" statements in
`security-model.md:74` and `:281`? Pick one; every other source is then a mechanical edit.

### B2. Is inter-cell IPC the vtable fast path, kernel-mediated messaging, or a declared hybrid?

- `docs/specs/01-core.md:14` — "**IPC** | Message Passing | **Direct Function Call**";
  `docs/specs/03-runtime.md:10` — "~2-3 chu kỳ CPU".
- `docs/specs/17-ipc-wire-contract.md:16-17` (Ratified) — "Cellos IPC is **kernel-mediated message
  passing** between cells (not the direct vtable call that `specs/01` aspires to)".
- `docs/system-architecture.md:122` — "Direct vtable IPC is planned for Phase 27"; `:986` — "IPC is
  syscall-based, not direct vtable call | 10–100× latency vs. spec | **Open**".
- Code: **a fast path exists today.** `kernel/src/fast_ipc.rs:1-7` — "a trusted Cell calling a
  service handler is just an indirect call (~3 cycles) versus ~100+ for an `ecall` round-trip";
  `register_vfs`/`call_vfs` resolved through the loader global symbol table; peer
  `libs/ostd/src/fast_ipc.rs`.
- The `~2–3 cycles` figure is load-bearing in the LBI-vs-MMU argument:
  `docs/specs/00-context.md:185` and `docs/specs/16-rustc-tcb.md:230` ("Cellos has cheaper IPC"
  vs seL4 300–400 cycles).

**Decide:** Pick one — (a) fast_ipc is the architecture and kernel-mediated messaging is the
fallback, or (b) fast_ipc is a named, enumerated exception for specific trusted service pairs
(VFS today) and Spec 17 remains the model of record. Then: does the 2–3-cycle number in
`00-context.md:185` / `16-rustc-tcb.md:230` cite fast_ipc (measured) or stay marked aspirational?

### B3. What is the kernel/TCB size of record, and is the budget still binding?

Six different numbers across normative docs; measured value exceeds the ratified budget ~4×.

| Source | Claim |
|---|---|
| `docs/specs/15-kernel-boundary.md:299` | "G1 (now) \| ≤ 7,000 LOC core \| ~5,600 LOC" |
| `docs/specs/15-kernel-boundary.md:319` | same era, "~7,200" |
| `docs/specs/16-rustc-tcb.md:136` | "Cellos kernel … ~11.5K" |
| `docs/specs/13-peripherals.md:18` | "kernel giữ <10K LOC" |
| `docs/project-overview-pdr.md:515` | "**Maintainability** \| < 6000 LOC kernel" |
| `docs/project-overview-pdr.md:425` | "Kernel LOC \| < 10000 \| ~22,600 \| ❌ Exceeded" |
| measured | **27,856** lines of `.rs` under `kernel/src` (excl. `third_party`) |

**Decide:** (1) one definition — all of `kernel/src`, or "core excluding migrating drivers +
hypervisor"? (2) one number in one owning doc. (3) Is Spec 15 §5's `≤5,000 core` G2 target still
binding, or withdrawn? Spec 15 is Ratified, so its "Current ~5,600" row is presently false.

### B4. Is Instant-On snapshot shipped, and is it compatible with KASLR?

- `docs/project-overview-pdr.md:28` — "Delivered in Phase 29 (Heap Snapshotting / Instant On) —
  ✅ COMPLETE (2026-06-07)."
- `docs/specs/03-runtime.md:96-102` — prerequisites, all unchecked: "Metadata Registry hoàn chỉnh",
  "**Direct IPC vtable (Phase 27)**", "FAT16 write path", "**Fixed physical layout đã confirmed
  (no physical ASLR)**".
- `docs/security-model.md:95-98` — "`limine.conf` sets `KASLR=yes` so consecutive boots load the
  kernel at **different physical bases**." Directly negates the fourth prerequisite.
- `docs/specs/03-runtime.md:92` tries to reconcile via a "PA-relative reloc table", but §4.5 still
  demands *no* physical ASLR.
- Metadata Registry: **ABSENT** in code (see U3).

**Decide:** Pick one — snapshot is (a) shipped and `03-runtime.md §4.5` is a stale prerequisite
list to delete, or (b) shipped only with KASLR off (mutually exclusive features, to be stated
normatively), or (c) not actually complete. This gates whether the PDR's headline differentiator
claim survives.

### B5. Cell-count target: 64 (Spec 19) or 1000+ (PDR)?

- `docs/specs/19-hardware-isolation-layers.md:57-58` (Accepted) — "**Isolation unit — the cell**:
  tens of them (`MAX_CELLS = 64`, revisit upward to ~256 …)"; `:78` rejects "**Raising `MAX_CELLS`
  toward BEAM scale** — wrong axis".
- `docs/project-overview-pdr.md:516` — "**Scalability** \| Support 1000+ Cells".
- Code: `kernel/src/memory/cell_quota.rs:15` `pub const MAX_CELLS: usize = 64;` — **verified
  present**, matches Spec 19.

**Decide:** Is the PDR's 1000+ NFR withdrawn and replaced by Spec 19 §3's "≥10,000 concurrent
actor-futures across ≤64 cells"? Yes/no.

---

## INCONSISTENT — two sources disagree; one must be corrected

### I1. Is F1 (`#![forbid(unsafe_code)]` on every Cell) absolute, or absolute-with-allowlist?

- `docs/specs/16-rustc-tcb.md:212` (Ratified) — "**F1** \| Every Cell crate MUST carry
  `#![forbid(unsafe_code)]`"; `:219` in `00-context.md` — "**tuyệt đối** trên mọi Cell".
- `docs/security-model.md:14` — "enforced by `cargo-geiger` in CI"; `:57` — "`cargo-geiger` CI gate
  fails if any Cell contains `unsafe`; zero-tolerance policy \| ✅ Mitigated".
- `docs/project-overview-pdr.md:533` — "✅ Zero unsafe code in Cells".
- `docs/specs/18-cell-trust-tiers.md:17-18` — "Policy F1 … is not enforced by any pipeline — at the
  time of this ADR only 25 of 71 cell crates carry the attribute."
- Code: cargo-geiger is **gone** — `.github/workflows/ci.yml:802` and
  `.github/workflows/security.yml:47` both say "Replaces the cargo-geiger gate". The real gate is
  `scripts/check-cells-unsafe-ratchet.py:4-6` — "Reality: Driver Cells need `unsafe` for MMIO/DMA,
  and a handful of FFI/runtime cells (mlibc, Lua, DOOM) carry documented exemptions" — with a
  ~40-file `ALLOWLIST`. Attribute present in **21 of 76** cell crates (worse than Spec 18's 25/71).

**Decide:** Is the unsafe ratchet + published allowlist the enforcement mechanism of record for F1
(pick-one: yes → F1 text becomes "absolute outside the reviewed allowlist", and
`security-model.md:14,57` + `pdr:533` are corrected; no → the allowlisted cells are out of
compliance and Tier 1 admission must say so).

### I2. Is hardware W^X on today, or a future layer?

`docs/specs/02-memory.md:42-45` (Definitive) — "**Protection Policy (W^X)** … hardware page-level
protection vẫn được bật: **Text**: RX. **Data**: RW. **Read-only**: R." vs
`docs/specs/19-hardware-isolation-layers.md:11-13` — "every cell page is mapped `USER+WRITE` in the
shared table … the p_flags W bit is currently ignored"; W^X = Layer A, "phase 10". Code confirms
Spec 19: `kernel/src/loader/elf.rs:100-118` — "All cell pages are mapped WRITE so the kernel can
apply PIE relocations … hardware-enforced W^X … is a G2 item"; only `Flags::EXECUTE` is conditional.
**Decide:** Rewrite `02 §5` as a target rather than current state — yes/no. If yes, is Spec 19's
Layer A (post-relocation tightening) the whole of §5's content?

### I3. Spec 17 §10 is marked Ratified but the entire mechanism is absent from code

- `docs/specs/17-ipc-wire-contract.md:219` — "## 10. Readiness notifications (G4 P2.5) — **Ratified
  2026-07-23**"; `:242-254` states normative collision invariants; `:257-260` — "`NetRequest::
  NotifyRegister { … }` (variant 17) and `NotifyDeregister { cap_id }` (variant 18)".
- `docs/specs/17-ipc-wire-contract.md:74-75` reserves byte-0 `0x11 NET_READY` and `0x12
  REACTOR_WAKE` in the global registry.
- Code: **ratified but unimplemented.** No `NET_READY`, no `REACTOR_WAKE` anywhere in `libs/api`,
  `libs/ostd`, `cells/services/net`. `NetRequest` at `libs/api/src/services/ipc.rs:111` ends at
  `L2Recv` — 17 variants (indices 0–16), no `NotifyRegister`/`NotifyDeregister`. `NetResponse` has
  6 variants (satisfies the ≤16 rule vacuously).
- Verified present: `INPUT_EVENT_OPCODE = 0x10` (`libs/api/src/services/input.rs:184`),
  `WIRE_ASCII = 0x04` (`kernel/src/task/drivers/console_drv.rs:180`), `APP_MSG_MAGIC = 0xAC`
  (`libs/ostd/src/app.rs:40`), `IPC_BUF_SIZE = 4096` (`libs/api/src/services/ipc.rs:21`),
  `INPUT_EVENT_QUEUE_DEPTH = 512` / `HOTSWAP_MSG_QUEUE_DEPTH = 64` (`kernel/src/task/tcb.rs:28,17`).

**Decide:** Does §10 stay "Ratified" as a reserved-but-unbuilt contract (byte-0 `0x11`/`0x12` held
against future collisions), or drop to Draft until G4 P2.5/P2.6 lands? Only one of the two makes
the §3 registry honest.

### I4. `/srv` — StubBackend per the ADR, RedoxFS in the code

- `docs/specs/09b-vfs-native-fs-adr.md:91-92` — "Until then `/srv` serves a no-op `StubBackend`";
  `:86-89` gates implementation on **all three** of: NVMe driver ships, a G2 board (C930/P870) is
  available, benchmark defined.
- `docs/specs/09-vfs.md:50` — Native FS at `/srv`, "Implement tại **G2** cùng NVMe. Hiện stub
  `StubBackend`".
- Code: `cells/services/vfs/src/manager.rs:60-64` — "`/srv`: RedoxFS CoW B-tree filesystem on **MBR
  partition P5**" → `mounts.add_backend(Box::new(RedoxFsBackend::mount("/srv")))`.
  `backend_stub.rs` still exists but is not the `/srv` mount.

**Decide:** Were the ADR's three trigger conditions waived? Pick one: RedoxFS-on-MBR-P5 is the
accepted G1 configuration (ADR amended), or the mount is premature and reverts to StubBackend.

### I5. Is Tier-1 trust a signature or a path prefix?

`docs/specs/12-reliability.md:63-65` — "Today 'trusted' = *path is under `/bin/`* (a directory, not a
crypto boundary). **Ed25519 signing is spec-only.**" vs `docs/security-model.md:88-90` — "The Tier 1
'signed cells only' guarantee is now **enforced**: Ed25519 signature verification runs at the loader
spawn gate". Code sides with security-model: `kernel/src/loader.rs:118-140` extracts `__ViCell_sig`
and calls `verify_cell()`; `kernel/src/signing.rs:38-40` `signing_required()` gates refusal. ⚠️ keys
are a dev seed / `[0u8;32]` placeholder (`security-model.md:186-188`).
**Decide:** Is `12 §2`'s dependency note superseded (yes/no) — and does a dev-seed key satisfy
"Tier 1 = signed only" for Spec 18's admission argument?

### I6. Scheduler: round-robin with no priorities, or 3-level preemptive?

`docs/system-architecture.md:971` "Round-Robin Scheduler" and `:987` "Round-robin scheduler, no
priority levels | RT tasks can starve | **Open** — Phase 25", vs `docs/specs/12-reliability.md:79`
crediting "3-level priority preempt + zero-latency SSIP". Code sides with Spec 12:
`kernel/src/task/scheduler.rs:131-172,635,794-795` (`api::TaskPriority::RealTime`, RT-hart routing).
**Decide:** Close `system-architecture.md:987` and redefine Phase 25 as EDF/CPU-budget only — yes/no.

### I7. TLSF RT pool: "not implemented" (gap table) vs present in code

`docs/system-architecture.md:988` — "TLSF allocator not implemented \| RT allocation guarantee
broken \| **Open** — Phase 25", vs `docs/specs/02-memory.md:26-27` (RT Pool as definitive) and code
**verified present**: `kernel/src/memory/rt_heap.rs` implements `RtTlsf`.
**Decide:** Delete the gap row (yes/no), and state whether `02-memory.md:27`'s O(1) guarantee has
been measured or is still unvalidated.

### I8. Input delivery: direct `on_event` call, or the kernel input queue?

`docs/specs/06-graphics.md:24` — "**Direct Call**: Gọi trực tiếp hàm `on_event(event)` … **không
qua hàng đợi** trung gian (Queue)", vs `docs/specs/17-ipc-wire-contract.md:132-141` (Ratified),
which makes the input path an explicit try-send-drop exception with `INPUT_EVENT_QUEUE_DEPTH = 512`
queued in the kernel's `pending_msgs` (verified `kernel/src/task/tcb.rs:28`).
**Decide:** Is `06 §2`'s direct-dispatch model withdrawn in favour of Spec 17 §6? Yes/no.

### I9. Cell↔cell memory protection attributed to page faults

`docs/specs/06-graphics.md:53` — "Mọi hành vi truy cập trái phép vùng nhớ đồ họa sẽ kích hoạt
`Page Fault` … bị **Poisoned**"; `docs/specs/09-vfs.md:87` makes the same claim for FS backends.
Against: `docs/specs/19-hardware-isolation-layers.md:10-12` ("every cell page is mapped
`USER+WRITE`") and `docs/specs/16-rustc-tcb.md:56-58` ("`rustc` is the only wall").
**Decide:** Strike the page-fault protection claim from `06 §5` and `09 §6` — yes/no.

### I10. Which hardware supplement is the cell↔cell wall: MTE/MPK/PMP, or per-domain page tables?

- `docs/specs/05-application.md:63-69` — "Hardware supplement (Tier 1, G2 roadmap)": ARM64 MTE,
  x86 MPK, RISC-V PMP. `docs/specs/16-rustc-tcb.md:112-114` names the same set for side channels.
- `docs/specs/19-hardware-isolation-layers.md:72-74` (Accepted) rejects them: "**MTE/PKU as the
  primary cell↔cell wall** — absent on all current deployment hardware"; `:14-15` — "VF2, Pioneer …
  and RK3588 (Cortex-A76/A55, **ARMv8.2**) … none has MTE (needs v8.5+) or x86 PKU."
- Contradictory MTE hardware claim: `docs/system-architecture.md:822` — "MTE ✅ (**ARMv8.5,
  RK3588**)" vs Spec 19's ARMv8.2 for the same SoC.
- `docs/specs/12-reliability.md:37-41` separately records PMP as "M-mode-only … viable only as a
  *static boot-time* guard", contradicting `05-application.md:69`'s "PMP … M-mode fence cho
  high-value Cells".

**Decide:** (1) Does `05-application.md §2.1` get replaced by a pointer to Spec 19's Layer A/B/C?
(2) Is RK3588 ARMv8.2 or v8.5 — one of the two documents is factually wrong about deployment
hardware, and Spec 19's whole "must be built from page tables" argument depends on the answer.

### I11. Cluster status: "planned, not implemented" vs "transport built"

- `docs/system-architecture.md:838-840` — "Cross-Machine Communication & Clustering (📋 **PLANNED**
  …) Status: **planned, not implemented** (all 📋)".
- `docs/specs/20-unified-ipc-contract.md:31` — "Transport / NodeId / relay / Noise KKpsk0 … ✅ built
  (`net-broker/src/{transport,relay,identity}.rs`)".
- Code sides with Spec 20: `cells/services/net-broker/src/transport.rs:40` `MAX_SESSIONS = 4` with
  LRU eviction at `:273-289`; `enrollment.rs`; `routing.rs`. Forwarding is the stub half —
  `cells/services/net-broker/src/main.rs:151` `dispatch()` is a TODO;
  `routing.rs:154` returns `self.self_tid` ("we forward via Noise (P08)"), module marked dead-code.

**Decide:** Which document owns cluster status? (pick-one), and does `system-architecture.md` adopt
Spec 20 §1's split table (transport built / forwarder stub) verbatim?

### I12. `viFS1`/`viFS2` naming retired in Spec 09, still normative in 00-fork and 11

`docs/specs/09-vfs.md:55-56` — "❌ **Dual-VFS viFS1/viFS2 bị loại bỏ**: TFS upstream đã chết …
`VIFS1` trong kernel từ nay hiểu là **BootFS/initramfs**", vs `docs/specs/00-fork.md:69-70` which
still assigns "**viFS1 (Classic)** | `RedoxFS`" and "**viFS2 (Modern)** | **TFS (B-tree)** … dùng
làm phân vùng hệ thống chính", and `docs/specs/11-shell.md:94,107` (shell built on viFS1/viFS2).
**Decide:** Are `00-fork §6` and `11-shell` superseded by Spec 09 (yes/no)? `11-shell.md` reads as
an informal narrative — confirm whether it is normative at all.

---

## UNDERSPECIFIED — direction agreed, mechanism not pinned down

### U1. Spec 20 depends on four things that do not exist and is not ratified

- Status `docs/specs/20-unified-ipc-contract.md:3` — "**Not normative until ratified.**" Checklist
  `:205-216` has 5 unchecked items; `:190` Q7 (ingress quota) "**open for prototype measurement**".
- Named hard prerequisites, all **ABSENT** in code:
  - kernel-attested sender (`:33` "phase 02 not landed") — no `path_hint`/attested-sender path in
    `kernel/src/loader/`.
  - broker-scoped, non-SpawnCap watch primitive (`:145-147` "This primitive is a Law-1 addition and
    a hard prerequisite; §2.4 does not work without it") — `NotifyOnExit` (204) is SpawnCap-only:
    `kernel/src/task/syscall.rs:1871` `if !has_spawn { return Err(PermissionDenied) }`.
  - `CellAddr` / `RemoteAddr` / `call_remote` types — none exist; only
    `CellNetId` at `libs/api/src/services/cluster.rs:120`.
  - yielding broker connect/handshake state machine (`:214`).

**Decide:** Which of the four Law-1 ABI additions is approved for the ABI now, and does Spec 20
ratify before or after phase 02 lands? (Spec 20 currently *amends* the Ratified Spec 17 while
itself being non-normative — that ordering needs a ruling.)

### U2. `machine_id` binding is required by Spec 20 but absent from canonical Spec 14

- `docs/specs/14-distributed.md:95-96` — "**Tiebreak:** lower `machine_id` wins Primary" with no
  binding requirement; `:133` — "All numeric constants trace to this doc."
- `docs/specs/20-unified-ipc-contract.md:83-86` — "`machine_id` … MUST be derived from the NodeId …
  never accepted from the wire (`enrollment.rs:48,68-76` currently decodes it unbound → **spoofable
  to win Primary**)."
- Code confirms the hole: `cells/services/net-broker/src/enrollment.rs:68-76` decodes
  `machine_id: u64::from_le_bytes(...)` with no check against the Noise-authenticated node id.

**Decide:** Amend Spec 14 to make NodeId-derived `machine_id` normative *now*, independent of
Spec 20's ratification — yes/no. (Spec 14 is the canonical constants doc, so the fix belongs there.)

### U3. The Metadata Registry is referenced as existing by five specs and does not exist

- `docs/specs/02-memory.md:29-36` — "Registry: Một bảng băm theo dõi `[Address Range] ->
  {OwnerID, State}`" with states `Owned` / `AsyncLocked`.
- Dependents: `03-runtime.md:22-24` (Async Pinning Registry / unload shield), `03-runtime.md:98`
  (snapshot prerequisite), `07-networking.md:47` (port OwnerID), `08-power.md:37` (hibernate
  pointer deflation scans the registry), `10-testing.md:15` (leak tests target the registry).
- Code: **ABSENT** — no `AsyncLocked`, no metadata-registry type anywhere in `kernel/`, `libs/`.

**Decide:** Pick one — the Metadata Registry is (a) a named G2 deliverable with an owning spec, or
(b) withdrawn in favour of per-Task grant tables. Five specs' mechanisms are unresolvable until
this is answered, including B4.

### U4. Is runtime grant creation available, and does Spec 12 §4.4's safety argument still hold?

- `docs/specs/00-context.md:188` — "Grant API (syscalls 208–212) là analogue của exchange heap";
  `docs/system-architecture.md:893` lists GrantAlloc/GrantShare/GrantSlice/GrantFree/BlkReadAsync
  as implemented.
- `docs/specs/12-reliability.md:215-217` closes the async-pin GC item as MOOT partly *because*
  "Grant/lease IPC cannot be created at runtime — `ostd::sys_grant` is a stub … so
  `grant_table`/`leases` are always empty".
- Code: **still a stub** — `libs/ostd/src/syscall.rs:855`
  `pub fn sys_grant(...) -> SyscallResult { SyscallResult::Err(SyscallError::Unknown) }`.

**Decide:** Are grants reachable from cells today via GrantAlloc/GrantShare (making `sys_grant`
dead code that should be deleted), or is runtime grant creation genuinely unavailable? If the
former, Spec 12 §4.4's "verified leak-free by construction" conclusion has lost one of its three
legs and needs re-derivation.

### U5. `catch_unwind` panic recovery is required by three specs and does not exist

- `docs/specs/01-core.md:43-47` — "Kernel wrap mọi inter-cell call bằng `catch_unwind`" + hardware
  reset + hot re-linking on panic. `docs/specs/10-testing.md:21` — fault-injection cell exists "để
  test cơ chế `catch_unwind`". `docs/specs/00-fork.md:98` — "bọc chúng lại bằng `catch_unwind`".
- `docs/specs/12-reliability.md:87-91` already flags this: "**None of that is implemented.** …
  until then §5 is aspirational, not descriptive."
- Code: **ABSENT** — no `catch_unwind` in `kernel/`, `libs/`, `cells/` (no_std, abort-on-panic).

**Decide:** Rewrite `01-core.md §5` to the actual model (panic → `terminate_current_cell_on_fault`
→ supervisor restart) — yes/no; and delete the `catch_unwind` requirement from `10 §3` and
`00-fork §C`.

### U6. Layer B needs an ADR that nobody owns yet

`docs/specs/19-hardware-isolation-layers.md:41-42` — "Requires an **ADR-level design pass on grant
mapping and the Spec 17 wire contract** before implementation"; `docs/specs/18-cell-trust-tiers.md:103`
— "Spec 02 … and Spec 17 … need addenda when Tier 2 lands"; `:98-100` — grants to a Tier-2 cell map
into the domain table explicitly and "`DataPtr`-style raw pointers (`GetFile`) are unrepresentable
across the tier boundary".
**Decide:** Is that grant-mapping ADR in scope for this window, and does the `DataPtr`/`GetFile`
removal become a *prerequisite* of Spec 18 rather than a consequence?

### U7. Kernel boundary law vs the specs that put policy or absent mechanisms in the kernel

- `docs/specs/09-vfs.md:74` — page-cache "Eviction được quản lý tập trung bởi **Kernel**", against
  `docs/specs/15-kernel-boundary.md:23-29` (Liedtke test) and §3.4 (no policy in kernel).
- `docs/specs/04-hardware.md:38-41` — deadlock watchdog scanning a kernel "Resource Graph" and
  panicking + reloading the lowest-priority Cell. Code: **ABSENT**; shipped detectors are the CPU
  watchdog + heartbeat (`kernel/src/audit.rs:52`, `kernel/src/task/scheduler.rs:794`).
- `docs/specs/04-hardware.md:35-36` — SMP work stealing + `spawn_pinned`. Code: scaffolding only —
  `kernel/src/task/hart_local.rs:74` "For Phase 02 (single active hart) …", `:111` "Phase 03 (SMP)
  will update".

**Decide, pick one each:** (a) page-cache eviction in the kernel or in the VFS cell; (b) is `04 §6`'s
cycle detector still in the architecture, or replaced by Spec 12 §4.2; (c) is `04 §5` re-marked G2?

### U8. Which architecture is the certification/production lane?

- `docs/specs/16-rustc-tcb.md:165-168` — riscv64 "⚠️ **Not yet qualified**" by Ferrocene; "**Do not
  make safety claims for RISC-V builds** … G1 RISC-V is development/demonstration only"; F7 at
  `:218`. Adoption point: "Before G2 production release on ARM64 hardware (RK3588)" (`:170`).
- `docs/project-overview-pdr.md:103` — "**Build Target**: `riscv64gc-unknown-none-elf` (primary)";
  `:391` — "**Primary**: QEMU virt machine (RV64 target)".
- `docs/specs/04-hardware.md:53` — G1 dev/test is "QEMU ARM virt (**QEMU-first**)".

**Decide:** Pick one production/certification lane for G2 — ARM64 (Ferrocene-qualified) or RV64 —
and make the PDR's "primary target" line agree.

### U9. WASM removal is an accepted consequence that has not happened

`docs/specs/18-cell-trust-tiers.md:104` — "`cells/drivers/wasm` and wasmi **leave the workspace**;
docs no longer describe WASM." Code: `cells/drivers/wasm/Cargo.toml` still present; `wasmi`,
`wasmi_core`, `wasmi_ir`, `wasmi_collections` still in `Cargo.lock`.
**Decide:** Is the removal a scheduled task with an owner (yes/no), or is the Spec 18 consequence
list aspirational and should say so?

---

## STALE — a claim about what works today that the code contradicts

### S1. `system-architecture.md` "ViUI awaiting G2" vs shipped ViUI v2 — and no spec for the shipped design
- `docs/specs/14-viui.md:3` — "**Status**: Architectural Decision — **awaiting G2 implementation**";
  `:270-279` puts P01–P08 (dual-facade egui/iced) at "G2 start".
- `docs/system-architecture.md:939` — "ViUI v2 — Reactive Signal Tree + Dual-Layer DSL — ✅ **ALL 7
  PHASES COMPLETE 2026-06-16** (production-ready)". Code: `libs/viui/`, `libs/viui-macros/`,
  `tools/viui-build/` all present.
- **Consequence:** the shipped architecture (Reactive Signal Tree + DSL) has *no* spec; Spec 14
  describes a different, unbuilt design. This is a genuine architecture-description hole, not just
  a status marker.

### S2. Spec 15 §3 driver/orchestration exceptions understate what is still in the kernel
- `docs/specs/15-kernel-boundary.md:238-239` — hotswap "~400" and snapshot "~350" LOC, "Correct
  home: Supervisory Cell". Code: `kernel/src/cell/hotswap.rs` = **547** LOC,
  `kernel/src/snapshot.rs` = **411** LOC — both still in kernel, both grown.
- `:212-213` — `mmc.rs` "~200", `pcie_ecam.rs` "~100". Code: **149** and **728** LOC (pcie_ecam is
  7× the stated size and `:213` says "simplify to store-only").

### S3. Spec 12 §3 axis-2 heartbeat adoption
- `docs/specs/12-reliability.md:77` — "heartbeat is opt-in (**only net adopts it so far**)".
- Code: `sys_heartbeat` (207) at `kernel/src/task/syscall.rs:587`, called by ~13 cells (net-broker,
  net, input, init, robot-dashboard, http-smoke, …). The reliability score table is driven by this
  row.

### S4. Spec 05 §4.5 x86 hypervisor "ENOSYS stub"
`docs/specs/05-application.md:349,379` — "`hal-x86` (ENOSYS stub)"; "VT-x impl deferred to G2". Code:
trait methods do return `NotSupported` (`hal/arch/x86/src/hypervisor.rs`), **but** root operation is
implemented — `hal/arch/x86/src/x86_64/vmx.rs` (154 LOC, full `enter_root()` VMXON sequence) and
`svm.rs` (197 LOC, EFER.SVME + VM_HSAVE_PA). The spec's binary stub/shipped framing has no slot for
this state.

### S5. Entropy fallback
`docs/system-architecture.md:869` — "`sys_get_random` **falls back to predictable xorshift32** when
VirtIO-RNG is absent" vs `docs/specs/17-ipc-wire-contract.md:161-163` — "now fail-closed behind
`dev-weak-rng`". One of the two describes shipped behaviour.

### S6. Spec 09 littlefs "G1 tail"
`docs/specs/09-vfs.md:49` — littlefs at `/data`, stage "G1 tail", "Bắt buộc trước robot demo". Code:
shipped — `cells/services/vfs/src/backend_littlefs.rs`, `libs/api/src/abi/manifest.rs:181`.

### S7. PDR internal contradictions (same document, opposing claims)
- `:68-69` "ARM AArch64/32-bit — **PLANNED**; x86_64 — **PLANNED**" vs `:153-155` "[x] ARM AArch64
  … [x] x86_64" and `:430` "Multi-Arch HAL | ✅ All 3".
- `:402` "Filesystems | FAT32 | ✅ **Read-only** working" vs `docs/system-architecture.md:891,901`
  (FAT32 write/read/delete, `/data/*` persistent).
- `:535` "✅ Full test coverage (80%+)" vs `:181` "limited unit tests" with all `:184-188`
  acceptance boxes unchecked.
- `:537` "✅ Reproducible builds (bit-for-bit identical)" — `docs/specs/16-rustc-tcb.md:183-187`
  only claims a toolchain pin; no reproducibility harness found in `.github/workflows/` or
  `scripts/`.
- `:201-203` "Complete VFS Service … **Current Status**: RamFS with basic `/bin/` access" vs the
  shipped MountTable (BootFS/RamFS/FAT32/littlefs/RedoxFS).

### S8. `docs/specs/10-testing.md:16-17` SASan
"**SASan** (Single Address Space Sanitizer): Công cụ … phát hiện một Cell cố tình truy cập vào vùng
nhớ của Cell khác" — no implementation, no tool by that name anywhere in the repo, under a
"Definitive" status.

---

## Verified-present (checked, no action needed)

`MAX_CELLS = 64` (`kernel/src/memory/cell_quota.rs:15`) · `IPC_BUF_SIZE = 4096`
(`libs/api/src/services/ipc.rs:21`) · `INPUT_EVENT_QUEUE_DEPTH = 512` /
`HOTSWAP_MSG_QUEUE_DEPTH = 64` (`kernel/src/task/tcb.rs:28,17`) · `MAX_SOCKETS = 18`
(`cells/services/net/src/socket_table.rs:15`) · `MAX_SESSIONS = 4` + LRU eviction
(`cells/services/net-broker/src/transport.rs:40,273-289`) · Ed25519 verify-at-spawn
(`kernel/src/loader.rs:118-140`, `kernel/src/signing.rs:38-40`) · TLSF RT heap
(`kernel/src/memory/rt_heap.rs`) · hypervisor aarch64-gated with NotSupported on other arches
(`kernel/src/hypervisor/registry.rs:8`) · Spec 14 constants (all trace to
`docs/specs/14-distributed.md`).
