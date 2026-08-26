# D1 — fast_ipc vs Spec 17: analysis and measurement plan

**Date**: 2026-07-31 · **Question from the docket**: is `kernel/src/fast_ipc.rs` the
architecture of record with Spec 17 as fallback, or is Spec 17 the model and fast_ipc a
named exception? · **Method**: read both implementations and every call site; verify
reachability statically before proposing any measurement.

## Answer first

The question as posed cannot be answered by choosing between two working designs, because
**the fast path has never executed a single call in the shipped system.** It is unreachable
for three independent reasons, all verified below. So there is no empirical basis for
"fast_ipc is the architecture", and the `~2–3 cycles` figure that two normative documents
lean on describes code that cannot run.

Recommendation: **Spec 17 is the model of record.** fast_ipc becomes an enumerated,
Tier-1-only optimization that must either be wired with a bounded design or deleted. But
the interesting finding is that the debate has been framed around the wrong quantity — see
§4.

## 1. Verified: the fast path is dead code

### 1a. Two disjoint copies of the state

`libs/ostd/src/fast_ipc.rs:42` and `kernel/src/fast_ipc.rs:38` each declare their own
`static VFS_HANDLER_PTR`. Cells link `ostd` statically, so every cell ELF carries its own
copy.

- VFS registers into **its own** copy: `cells/services/vfs/src/main.rs:164` calls
  `ostd::fast_ipc::register_vfs(vfs_fast_handler)`.
- Shell reads **its own** copy: `cells/tools/shell/src/cmd_fs.rs:347` calls
  `ostd::fast_ipc::call_vfs(...)`.

Different memory. The shell's pointer is always null, `call_vfs` returns 0
(`ostd/fast_ipc.rs`, mirroring `kernel/src/fast_ipc.rs:144-146`), and
`cmd_fs.rs:351-361` always takes the `sys_send`/`sys_recv` fallback. The in-code note at
`libs/ostd/src/fast_ipc.rs` states this honestly as a "PIE limitation".

### 1b. The bridge that would fix 1a does not exist

`kernel/src/fast_ipc.rs:11-13` describes the fix: cells reach the kernel's canonical
instance "by name through the loader's global-symbol-table resolution (see
`loader::dynsym`)". Verified against the tree:

- `resolve_export` has exactly **one** occurrence repo-wide — its own definition at
  `kernel/src/fast_ipc.rs:170`. Nothing calls it.
- `R_RISCV_JUMP_SLOT` appears once, as a constant at `kernel/src/loader/reloc.rs:18`, in a
  `#[allow(dead_code)]` module. It is never matched in the relocation loop.
- No `loader::dynsym` module exists.

So the loader cannot redirect a cell's `call_vfs` to the kernel copy. The module
documentation describes machinery that was never built.

### 1c. Even if wired, the kernel's `call_vfs` is uncallable from a cell

`kernel/src/fast_ipc.rs:158` executes `SieGuard::disable()` → `csrrci sstatus, 0x2`, a
privileged CSR write, and its own Safety note says "called from S-mode trap handler
context". Cells run **U-mode**: `kernel/src/task.rs:749` sets `trap_frame.sstatus = 0x20`
(SPP=0), against `0x42120` (SPP=1) for kernel threads at `:820`. A U-mode cell reaching
that instruction takes an illegal-instruction trap. And the instruction fetch would fault
first: the `USER` flags in `kernel/src/memory/paging.rs` cover MMIO windows only
(VirtIO `:215`, platform `:270`, GPIO `:297`), and `:199-200` explicitly keeps kernel UART
non-USER — kernel `.text` is not USER-mapped.

**Consequence for the docs.** `00-context.md:185` and `16-rustc-tcb.md:230` use the 2–3
cycle figure to claim Cellos IPC is cheaper than seL4's measured 300–400 cycles. That
comparison currently rests on nothing measurable. It is not "unmeasured" — it is
unmeasurable in the present privilege architecture.

## 1bis. Correction to this report (added after reading Spec 17 §11)

Two claims in §2 below were written before I read Spec 17 §11, and are wrong as stated.
Left in place with this correction rather than silently edited, because the corrected shape
is what decides A vs B.

**The dichotomy is false.** Spec 17 §11.4 "Direct (non-`ecall`) service calls" was
**ratified 2026-07-30**. Spec 17 does not compete with fast_ipc — it *governs* it, and
imposes the binding rule: "A fast-path handler MUST authorize exactly as its `ecall`
counterpart does." So the docket question "which is the model of record" has already been
answered in the affirmative for Spec 17; what remains is only whether the fast path is a
transport worth keeping under it.

**Attestation is not lost — it was closed yesterday.** §2b below says a fast path cannot
attest. That is true of a *cell→cell* U-mode call (§3), but not of what is implemented:
`kernel::fast_ipc::call_vfs` **is kernel code**, so it can and does call
`attested_identity_of`, and `vfs_fast_handler` (`cells/services/vfs/src/main.rs:118-152`)
gates fully — unattributable caller → `Err(3)`, then `has_met`, `is_sealed`, `can_read`,
identical to the message path. Phase 02 covered this path deliberately.

So the real fork is not "fast means unattested". It is:

| Design | Attestation | Privilege | Tier 2 |
|---|---|---|---|
| (i) kernel-hosted direct call — **what exists** | kept (kernel resolves it) | **broken**: cell must execute kernel code containing `csrrci` from U-mode (§1c) | impossible — the *handler* is another cell's text, unmapped in a domain table |
| (ii) cell→cell U-mode call — §3 | **lost** (no oracle) | fine | impossible — same reason, both directions |

Both are impossible for Tier 2; (i) is impossible today for privilege reasons; (ii) is
impossible for authorization reasons. That is the whole decision space.

### 2a. Tier 2 makes a universal direct call impossible

Spec 18 (accepted 2026-07-30) gives an unsigned native cell its own page table, mapping
only its own pages. The VFS handler's code pages are not in that table, so a direct call
cannot be represented at all — not slower, *impossible*. Whatever fast_ipc becomes, it can
only ever apply to Tier 1. This alone settles "primary vs exception": it cannot be primary.

### 2b. No kernel in the loop means no attestation — and `GetFile` is unrevocable

The kernel implementation is careful about this and says so at
`kernel/src/fast_ipc.rs:123-133`: identity must be derived from live scheduler state,
"never from an argument, because every argument on this path is chosen by the cell being
authorized", precisely because `GetFile` returns a raw `DataPtr` — "permanent, unrevocable
read authority in a single address space".

That safeguard exists only because the kernel copy *is* kernel code and can call
`attested_identity_of`. A genuine cell→cell U-mode direct call (the only form Tier-1 could
actually support, §3) has no such oracle: the callee sees only caller-chosen arguments.

**The tension is structural**: the path is fast because the kernel is absent, and
authorization requires the kernel to be present. One or the other.

### 2c. And a real-time conflict

Both copies disable S-mode interrupts for the whole handler
(`kernel/src/fast_ipc.rs:154-158`) because the VFS FAT16 backend holds a spinlock. That
means a filesystem operation — potentially including block I/O — runs with interrupts off.
Against Spec 12's RT guarantees and the 10 ms tick, this is a latency hazard that the
message path does not have. If fast_ipc is ever wired, max interrupt-off duration becomes a
release-gating number, not a footnote.

## 3. The only privilege-legal shape for a direct call

Worth stating because it is not what the code does: a direct call does **not** need kernel
privilege if the callee is a *cell*. Shell (U-mode) calling the VFS handler (U-mode) is an
ordinary indirect call. What must move is the *pointer table* — into a page both cells can
read (USER-readable, kernel-written). Then no `csrrci`, no kernel text access, no privilege
violation.

The current design put the table in the kernel to solve the per-ELF-static problem (1a),
and in doing so made the call privileged (1c). A shared USER page solves 1a without 1c —
but it also removes the kernel from the path, which re-opens 2b in full.

## 4. The debate is framed around the wrong quantity

"3 cycles vs 100 cycles" compares an indirect call to an `ecall` trap. That is not where the
cost of a VFS request lives. Comparing the two shipped paths at the call site:

| | Fast path (`cmd_fs.rs:345-349`) | Message path (`cmd_fs.rs:351-361`) |
|---|---|---|
| Request marshalling | **none** — passes `&VfsRequest<'_>` by reference | `api::ipc::encode` (postcard) into a 512 B buffer |
| Transition | indirect call (~3 cycles) | `ecall` round-trip (~100+) |
| Identity | `attested_identity_of` → **takes `SCHEDULER.lock()`** (`task/syscall.rs:510`) | same lock, inside the syscall |
| Reply | handler writes `out` directly | copied into recv buffer, postcard-decoded |

Two corrections fall out:

1. **The saving is not the jump — it is the serialization.** Skipping postcard
   encode+decode of a request containing a `&str` is worth hundreds of cycles; the `ecall`
   trap is worth ~100. The jump's 3 cycles are noise. **Passing a Rust reference instead of
   serializing is the actual SAS payoff** — and that reframes the whole question.
2. **The fast path is not lock-free.** It acquires the same global `SCHEDULER` spinlock the
   syscall path does (an uncontended atomic RMW plus fence already dwarfs 3 cycles, and it
   serializes across harts on SMP). Any measurement must include it.

This suggests a **third option the docket did not list**: keep the kernel mediating
*control* (attestation, capability check) while letting the *payload* travel by reference
for Tier-1 cells — which is largely what grant pages already do
(`libs/api/src/services/ipc.rs` `ReadGrant`/`WriteGrant`, zero-copy). If most of fast_ipc's
advantage over the message path is "don't serialize", and grants already deliver that with
the kernel in the loop, then fast_ipc's marginal value is one `ecall` (~100 cycles) on a
request whose total cost is thousands — and the honest answer may be to delete it.

## 5. What to measure — and note the measurement cannot decide §2

Measurement can size the prize; it cannot overturn 2a (Tier 2) or 2b (attestation). Run it
only if the decision is "keep as a Tier-1 exception" and a number is needed to justify the
maintenance cost.

Existing harness: `cells/tests/bench` already has `IpcSendRecvBench` and a runner reporting
p99, with PDR targets printed at `main.rs:206` (`ipc<50µs`, `syscall<10µs`) and an
idle-vs-load comparison at `:143-159`. So the infrastructure exists; what is missing is a
fast-path arm — which cannot exist until §1 is fixed.

**Experiment 1 — baseline the real cost (runnable today, no fast path needed).** Break down
one `GetFile` on the message path into: postcard encode · `ecall` entry · `SCHEDULER` lock ·
VFS handler body · reply copy · decode. Report cycles per stage, p50/p99, on QEMU and on a
board. *This alone answers whether the prize is 5% or 40% — and it needs no new mechanism.*

**Experiment 2 — fast-path arm (requires wiring §1 behind a feature flag).** Same workload,
same request, both paths, measured end-to-end. Report the delta, and separately the delta
with identity resolution removed, to show how much of the win the `SCHEDULER` lock eats.

**Experiment 3 — RT admissibility (gating, not informational).** Max interrupt-off duration
during a fast-path `GetFile` against the RT budget, for the largest supported request and a
cold FAT16 path. If this exceeds the RT deadline, fast_ipc is inadmissible for RT
configurations regardless of throughput.

**Environment blocker.** The build machine has no QEMU and no cross toolchain — the same
constraint recorded in the phase 09/10/11 deviation logs. Experiments 1–3 need a QEMU lane
or hardware; they cannot be run from this session.

## 5bis. What each path actually provides

### Spec 17 message path — the general-purpose contract

- **Universal.** Every cell, every tier. Tier-2 domain cells can only use this; Spec 20
  extends the same shape to remote. It is the only transport that survives every decision
  already accepted.
- **Attested.** `RECV_ATTEST_CALLER` writes `CallerIdentity` (cell_id + **generation** +
  tid) into the recv-buffer tail, after the payload, so a sender cannot pre-place a forgery
  (§11.2). `generation` is what stops a successor cell under a recycled id from inheriting
  its predecessor's handles (§11.3).
- **Selective receive** by sender mask (§2), one governed byte-0 discriminant namespace
  (§3), fail-loud (§7), defined blocking discipline with the input-queue exception and
  backpressure (§6).
- **Interrupts stay enabled.** No RT hazard.
- **Costs**: postcard encode + `ecall` + decode; 4096 B frame with ~480 B practical payload
  per reply, so bulk data must chunk or move to grants.
- **Composes with grants** for bulk: `ReadGrant` (per 4 KB page), `ReadFileGrant` (resolves
  a path straight into a caller-owned grant, up to grant size).

### fast_ipc — what it adds, precisely

- **The request is not serialized.** `&VfsRequest<'_>` travels by reference, including its
  `&str` path and (for `Write`/`Append`) its `&[u8]` content. No 480 B ceiling on the
  request side, no copy. *This is the genuine SAS payoff and the only thing neither the
  message path nor grants give.*
- **No trap** (~100 cycles saved).
- **Attestation retained** (§1bis).
- **Degrades gracefully**: returns 0 → caller retries the ecall path
  (`cmd_fs.rs:351-361`).
- **The reply is still postcard-encoded** — `vfs_fast_handler` ends with
  `api::ipc::encode(&resp, out)`. So the saving is request-marshalling plus the trap, not
  the whole round trip. My §4 table overstated this.

### fast_ipc — what it costs

- **One operation.** Only `GetFile`; every other variant returns `Err(0xFE)` "must use ecall
  path" (`vfs/main.rs:148`).
- **Interrupts off for the handler's duration**, which in turn forces the design to
  *decline* any cell it has not already served over the ecall path — resolving the seal
  needs a syscall it cannot make with interrupts disabled (`vfs/main.rs:107-113`). So it
  already accepts "one ecall per cell" as warm-up.
- **Unreachable today** (§1a–1c) and **impossible for Tier 2** (§1bis).

### The decisive fact

fast_ipc's single operation is `GetFile`, and `GetFile` is itself on the way out under two
already-accepted decisions: Spec 18 states `DataPtr`-style raw pointers are
"unrepresentable across the tier boundary", and phase 06 leaves "`GetFile` still returns a
raw pointer (4 callers)" as explicit remaining work. A raw `DataPtr` into VFS's RamFS
cannot be dereferenced from a domain-table cell at all.

**So keeping fast_ipc means maintaining a mechanism whose only purpose is an operation that
two accepted decisions are retiring.** That is the argument for B, and it does not depend on
any measurement.

**A becomes the right answer only if** the by-reference *request* niche is wanted for its
own sake — medium payloads (roughly 480 B to a few KB) where the message path must chunk and
a grant's alloc/share/free is too much overhead, e.g. `Write { path, content }`. That is a
real gap in the transport lineup. But claiming it requires new work: solving the privilege
problem (§1c), a resolution for the RT hazard, and accepting Tier-1-only scope. It is a
*new feature proposal*, not a defence of the existing code.

## 5ter. MEASURED — and it refutes §4 of this report

Measured 2026-07-31 on QEMU TCG, RV64, from a clean `main` worktree. Environment note: the
"no QEMU / no cross toolchain" premise recorded in the phase 09/10/11 deviation logs is
false on this host — see `qemu-build-unblock-260731.md`.

| Quantity | p50 per op | Source |
|---|---|---|
| `encode_request` (postcard, `GetFile(&str)`) | **271 ns** | added scenario, 10 samples × 1000 ops |
| `decode_reply` (postcard, `DataPtr`) | **364 ns** | same |
| bare `ecall` round trip (`sys_get_time`) | **1 902 ns** | same |
| **full IPC round trip** | **48 500 ns** | **pre-existing** `ipc_send_recv`, n=1000 |
| context switch | 36 400 ns | pre-existing `context_switch`, n=1000 |

**§4 of this report was wrong.** It argued the saving is serialization, not the jump.
Marshalling both ways is 271 + 364 = **635 ns, or 1.3 % of a 48.5 µs round trip**.
Serialization is nearly free. What costs is the **rendezvous**: two traps, two context
switches and a scheduler round trip. A single bare syscall (1.9 µs) already costs seven
times a request encode.

Recomputing the saving with these numbers:

- message path ≈ 48.5 µs + handler body
- direct call ≈ identity resolution + handler body

A RamFS `GetFile` handler at 10 µs gives a saving of 48.5 / 58.5 ≈ **83 %**; at 1 µs it is
≈ 98 %. That is an order-of-magnitude effect, not a micro-optimisation — the opposite of
what §4 predicted before measuring.

**Two caveats, both pointing the same way.** QEMU TCG makes traps *cheap relative to
compute* compared with real silicon, so the ratio on VF2/Pioneer should be at least this
favourable to a direct call. And `ipc_send_recv` p99 = 86.6 µs **fails the 50 µs PDR
target** (the suite reports FAIL) — worth its own finding independent of D1.

**What this does not change.** The three constraints from §1bis and §2 are not measurable by
benchmark and all still hold: a U-mode cell cannot execute the kernel's `call_vfs` (it
contains `csrrci`); a direct call is unrepresentable for Tier 2 because the handler is
another cell's text; and running the handler with interrupts disabled conflicts with the RT
budget. So the measurement says *the prize is large*, not *the old code should come back*. A
re-implementation would be a new design that answers all three.

**Method note.** The custom round-trip scenario initially deadlocked through an author
error: `bench-probe` is a separate binary with its own role dispatch, and the `resp-echo`
role was added only to `main.rs`, so the peer exited and the caller blocked forever in
`sys_send`. Fixed. The conclusion does not rest on it — the 48.5 µs denominator comes from
the repo's own pre-existing `ipc_send_recv` benchmark, and the custom scenario only adds
the 0.64 µs of marshalling already measured separately.

**Also learned about the bench cell**: it declares its syscalls explicitly
(`cells/tests/bench/src/main.rs:12-25`) and holds neither `LookupService` nor `RecvTimeout`,
so it cannot discover the VFS tid — which is why the existing `ipc_send_recv` scenario
deliberately avoids depending on it, and why the denominator here is a typed echo peer
rather than the real VFS.

## 6. RULING — decided 2026-07-31

**Spec 17 is the model of record. `fast_ipc` is to be rewritten for Tier 1, not restored.**
(User ruling, 2026-07-31, after the measurement in §5ter.)

This is neither of the options originally drafted below. Option B (delete) was superseded by
the measurement: an 82–98 % saving on a service call is too large to discard. Option A (keep
the existing code as an enumerated exception) is not viable either, because the existing code
is unreachable by construction — two disjoint statics, a bridge that was never built, and a
privileged instruction on a path a U-mode cell must take.

### What the ruling commits to

1. **Spec 17 governs.** §11.4 already binds any direct path to "authorize exactly as its
   `ecall` counterpart does". A rewrite is a transport *under* Spec 17, never a parallel
   model.
2. **Tier 1 only, stated normatively.** Spec 18's tier boundary is the scope limit: a direct
   call is unrepresentable for Tier 2 because the handler is another cell's text and
   `DataPtr` cannot cross a domain page table.
3. **The three constraints in §5ter are design requirements, not risks to accept.** A rewrite
   must answer privilege, Tier-1 scoping, and the interrupt-off/RT budget explicitly, with
   the max interrupt-off duration measured and release-gating.
4. **Honest numbers.** The 2–3-cycle claim in `00-context.md:185` and `16-rustc-tcb.md:230`
   is replaced by the measured figures or marked aspirational. The comparison against seL4's
   300–400 cycles cannot stand on an unrunnable path.
5. **Remove the scaffolding.** `resolve_export` and `R_RISCV_JUMP_SLOT` describe a bridge
   that does not exist; leaving them is precisely what let the module doc mislead. Per
   Spec 21, whatever remains carries an `impl` or `absent` anchor.
6. **`GetFile` is not the target op.** It returns a raw `DataPtr` that Spec 18 already
   declares unrepresentable across tiers and phase 06 lists for removal. A rewrite should
   pick ops that survive that removal.

### Open design question for the rewrite

The privilege constraint forks the design, and neither branch is free:

- **Kernel-hosted dispatch** keeps the identity oracle (`attested_identity_of` from live
  scheduler state) but requires the caller to enter the kernel — which is the trap the
  design exists to avoid. Something cheaper than a full `ecall` must carry it.
- **Cell→cell dispatch through a shared USER page** removes the trap entirely but loses
  attestation, so it can only serve operations that need no caller authorization — a much
  narrower set than `GetFile`.

Sizing that fork is the first task of the rewrite, and it needs a hardware measurement:
`ecall` on TCG (1.86 µs) is cheap *relative to compute* compared with real silicon, so the
kernel-hosted branch may look better on VF2/Pioneer than these numbers suggest.

---

## 6bis. Original options (superseded by §6, kept for the record)

Pick one:

- **(A) Recommended.** Spec 17 is the model of record. fast_ipc is documented as a
  Tier-1-only, enumerated exception for specific service pairs (VFS today), explicitly
  outside the Tier-2 contract. The 2–3 cycle figures in `00-context.md:185` and
  `16-rustc-tcb.md:230` are marked aspirational-and-unmeasured, or removed, until
  Experiment 1 produces a number. `resolve_export`/`R_RISCV_JUMP_SLOT` are either wired
  behind a feature flag or deleted as scaffolding — leaving them is what let the module doc
  describe machinery that does not exist.
- **(B)** Same as A, but delete fast_ipc outright: two copies of a dead mechanism with a
  documented-but-absent bridge is a liability, and grants already provide the zero-copy
  payload path with the kernel retained for attestation. Reconsider only if Experiment 1
  shows serialization dominates.
- **(C) Not recommended.** Elevate fast_ipc to primary. Requires cells in S-mode or
  USER-mapped kernel text (surrendering the only hardware wall Cellos has), abandons
  attestation for `GetFile`, and is unrepresentable for Tier 2.

Either A or B also implies a Spec 21 anchor: whatever survives gets an `impl` or `absent`
anchor so the next reader cannot be told about a bridge that isn't there.
