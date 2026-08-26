# Wave 1 review — Phase 10 (W^X post-relocation) + Phase 11 (cellos-sign F1/F5)

Date: 2026-07-30 · Reviewer: haily-reviewer (`--deep`) · Scope: full uncommitted working tree
Constraint: no QEMU / cross-toolchain. All findings are derived from code plus host-executable
experiments (rustc lint probes, scanner probes, ELF program-header parsing of the three
committed `init` images).

**VERDICT:** BLOCKED — three Critical defects survive refutation. Two of them (C1, C2) mean the
phase's stated guarantee is defeated by an attacker-supplied ELF that reaches the loader through
an unsigned, ungated spawn path, and the third (C3) means the guarantee is not yet established
when the cell first becomes runnable on SMP.

---

## Critical

### C1 — Boundary-page merge produces a USER W+X page; `enforce` warns and applies it anyway

`kernel/src/loader/wx.rs:97-108` · `kernel/src/loader/elf.rs:195-209` · `kernel/src/loader/wx.rs:63-74`

`reject_wx_segment` is evaluated **per PT_LOAD** (`elf.rs:85-93`), before any page is allocated.
The merge that actually decides a page's permissions happens later and independently
(`elf.rs:199-200`), OR-ing `final_flags` across every PT_LOAD that touches the page. `wx::enforce`
detects the resulting W+X page, emits `log::warn!` (`wx.rs:98-104`), and then calls
`protect_page(va, flags)` at `wx.rs:108` **with the W+X flags unchanged**. The only consequence of
a W+X page is a log line.

**Failure scenario (inputs → state → wrong outcome).** Author an ELF with two PT_LOADs that share
one 4 KiB page and split the two bits:

| PT_LOAD | p_flags | p_vaddr | p_memsz | pages touched |
|---|---|---|---|---|
| 0 | `PF_R\|PF_X` | `0x0000` | `0x1100` | `0x0000`, `0x1000` |
| 1 | `PF_R\|PF_W` | `0x1200` | `0x0100` | `0x1000` |

Neither segment is individually W+X, so `reject_wx_segment` returns `Ok` for both. Page
`load_base+0x1000` is allocated once by segment 0 with `final_flags = R|X`, then segment 1 hits the
`already_ours` branch and merges to `R|W|X` (`elf.rs:199-202`). `enforce` warns and installs
`VALID|USER|READ|WRITE|EXECUTE|ACCESSED|DIRTY`. Because Cellos runs every cell in **one** address
space, that page is now writable *and* executable from U-mode for **every** cell in the system —
a permanent shellcode trampoline, which is exactly the primitive Phase 10 exists to remove.

**Reachability.** `Syscall::SpawnFromMem` (`kernel/src/task/syscall.rs:2611-2637`) calls
`task::spawn_from_mem` **directly**, bypassing `loader::spawn_gated` — so no Ed25519 signature
check (`kernel/src/loader.rs:115-146`), no manifest-privilege gate, no policy lookup. The shell
declares that syscall (`cells/tools/shell/src/main.rs:34`) and `exec <file>` feeds it arbitrary
file bytes (`cells/tools/shell/src/commands.rs:78-97`), validating only the four ELF magic bytes.
Any file the user can place on the writable partition is a crafted-ELF delivery vehicle.

**Refutation attempts, all failed.**
1. *"The ELF must be signed."* No — `SpawnFromMem` never calls `crate::signing`; the signature gate
   lives only in `spawn_gated`, which this route does not traverse.
2. *"Only privileged cells can spawn from memory."* The interactive shell holds the syscall and
   exposes it as `exec`.
3. *"`reject_wx_segment` catches it."* It is per-segment and runs before the merge exists.
4. *"Linkers page-align across a permission change, so this cannot happen."* True of linker output —
   and irrelevant, because these are attacker-authored bytes. `load_segments` never requires
   PT_LOADs to be page-aligned or non-overlapping; it explicitly supports the shared-page case.
   (Confirmed the merge path is live on real images: in all three committed `init` binaries the
   R-X and R-- segments already share page `0x1000` — riscv `0x0..0x135a` / `0x1360..0x23e0`.)
5. *"The page is only in the attacker's own cell."* False in a single address space — there is one
   page table, so a `USER|WRITE|EXECUTE` leaf is reachable from every cell at EL0/U-mode.

**Fix.** Re-run the W+X test on the *merged* flags and fail the spawn, not warn — i.e. at
`wx.rs:97` return `Err(ViError::PermissionDenied)` instead of `log::warn!`. The alternative (drop
`WRITE` from the merged page) silently breaks a legitimate `.data` boundary, so refusal is correct.

---

### C2 — `SpawnFromMem` spawns with `CellId(0)`, so the W^X fault this phase creates panics the kernel

`kernel/src/task/syscall.rs:2630-2634` · `kernel/src/memory/paging.rs:969-976` ·
`hal/arch/riscv/src/rv64/trap.rs:141-155`

The syscall hands `CellId(0)` to `spawn_from_mem` and nothing reassigns it afterwards (contrast
`loader.rs:190-196`, which does). The scheduler propagates it into hart-local state
(`scheduler.rs:857`), so `current_cell_id() == 0` for the whole life of an `exec`-spawned cell.
Both unserviceable-fault handlers key their "kill the cell, keep the kernel" decision on that
value being non-zero:

- x86_64 `fault_kill_cell` — `if cell_id == 0 { panic!(...) }` (`paging.rs:971-976`), i.e. the new
  code added by this phase.
- riscv64 — `if from_user && cell_id != 0 { vi_terminate_on_fault(...) } else { panic!(...) }`
  (`trap.rs:143-155`).

**Failure scenario.** `exec /data/anything` on a cell that touches its own now-read-only `.text`
(or dereferences null, or overflows its stack). Before this phase such a store simply succeeded,
because every cell page stayed WRITE forever. Phase 10 deliberately converts that into a user-mode
protection fault — and on this spawn route that fault reaches a handler that cannot attribute it
to a cell and therefore takes the **whole kernel** down. The phase's own survivability contract
("Panicking here would turn every cell-level memory bug … into a whole-system halt",
`paging.rs:955-960`) is violated on the one spawn path that accepts untrusted binaries.

**Refutation attempts, all failed.**
1. *"The integration test covers this."* It does not. `tests/integration/tests/wx-text-write.rs:107`
   runs `wx-test` through the shell's `/bin` route, which goes to `sys_spawn_from_path` →
   `spawn_gated` → `loader.rs:190` assigns `CellId(tid)`. `kernel_survives_the_faulting_cell` is
   green on a path that never exercises `CellId(0)`.
2. *"CellId(0) never reaches U-mode."* It does — `spawn_with_stacks(name, CellId(0), …)` builds a
   normal user task, and `pick_next_local` publishes `cell_id 0` to hart-local state before the
   context switch.
3. *"This is purely pre-existing."* The `CellId(0)` assignment is pre-existing; the *fault class*
   is new. Phase 10 is what makes an ordinary cell bug (writing `.text`) fault at all, so it is
   this diff that converts a latent misattribution into a reachable kernel panic.

**Fix.** Assign `CellId(tid)` in the `SpawnFromMem` arm the way `loader.rs:190-196` does — and,
separately, route that syscall through `spawn_gated` so the signature and manifest gates apply.

---

### C3 — The cell is publishable to the work-stealing scheduler before W^X is applied

`kernel/src/task.rs:653` → `kernel/src/task.rs:793-806` · `kernel/src/task/scheduler.rs:285` ·
`kernel/src/task/hart_local/ready.rs:107-166` · `kernel/src/task/smp.rs:150-166`

`spawn_with_stacks` (step 6, `task.rs:653`) sets `TaskState::Ready` and calls `push_ready`
(`scheduler.rs:284-285`), which enqueues the task on the **calling** hart's ready queue. Relocation
and `wx::enforce` run afterwards, at `task.rs:786` and `task.rs:801`. Hart 1's idle loop
(`smp.rs:159-166`) yields on every 10 ms tick; `pick_next_local` finds an empty local queue and
calls `steal_from_busiest` (`scheduler.rs:845-848`), which moves Normal-priority tasks out of
hart 0's queue with no check that the task is fully constructed (`ready.rs:118-166`).

**Failure scenario.** With `-smp 2` (hart 1 brought online by `smp::start_secondaries`,
`main.rs:596`), a spawn whose segment copy + up to 65 536 relocations straddles a 10 ms tick can be
stolen. Hart 1 begins executing the cell while every page is still mapped `WRITE`. The cell's
first instruction fetches and stores populate hart 1's TLB with the **writable** PTE. Step 8 then
runs `protect_page` on hart 0, whose `sfence.vma` (`hal/arch/riscv/src/rv64/paging.rs:17-20`) is
local-hart only. Hart 1 keeps a writable translation for that `.text` page until an unrelated full
fence evicts it. Repeat `exec` in a loop until the race lands.

This is the ordering defect, not the missing shootdown (which you already know about and I am not
re-reporting): even with a perfect IPI shootdown, the cell would still have *run* unrelocated and
unprotected. The honesty problem is that `wx.rs:6-7` states the opposite as a fact — *"That window
closes before the cell executes its first instruction"* — and `wx.rs:11-19`'s ordering contract
lists no step that makes the task runnable, so a reader cannot see the gap. `page_protect.rs:14-16`
does disclose the missing shootdown honestly; `wx.rs` does not disclose this at all.

**Refutation attempts.**
1. *"`push_ready` targets the current hart, so nothing else can pick it up."* True at enqueue time,
   defeated by work stealing 10 ms later.
2. *"Only RT tasks migrate."* Inverted — `steal_from_busiest` steals *only* Normal/Background and
   never RT (`ready.rs:103-104`); a freshly spawned cell is Normal.
3. *"CI is single-hart, so this cannot happen."* CI is not the threat model; `MAX_HARTS = 2` and
   `start_secondaries()` is unconditional on riscv64. Downgrades likelihood, not severity.
4. *"It's pre-existing because relocation was already after `spawn_with_stacks`."* The window is
   pre-existing; the *claim* that it is closed is new, and the phase's guarantee now depends on it.

**Fix.** Move task publication after step 8: create the task in a non-runnable state (or defer
`push_ready`) and mark it Ready only once `wx::enforce` has returned `Ok`. This also closes the
pre-existing "runs before relocation" hazard for free.

---

## Major

**MJ1 — aarch64 `flush_tlb_page` invalidates the wrong translation regime when booted at EL2.**
`hal/arch/arm/src/aarch64/paging.rs:47-60` uses `tlbi vaae1is`, which acts on the **EL1&0** regime.
When `is_el2()`, `el2_mmu_init` installs the *same* root in both `TTBR0_EL2` and `TTBR0_EL1`
(`hal/arch/arm/src/aarch64/el2.rs:96-99` and `:143-152`). `protect_page` therefore guarantees the
new permissions to cells (EL0) but never invalidates the kernel's own EL2 TLB. Today no kernel path
translates a cell VA at EL2 (relocation deliberately writes through the identity alias,
`kernel/src/loader/reloc.rs:107-113`), so this is latent — but `protect_page` is a general-purpose
API whose doc comment (`page_protect.rs:11-13`) promises "the new rights are in force on THIS hart
the moment the call returns", which is false for EL2. Add a `tlbi vae2is` companion under
`is_el2()`, or narrow the doc contract.

**MJ2 — `scripts/sign-cell.py` is still a signing path with no F1/F5 check.**
`scripts/cellos_sign/cli.py:80` states "There is no path to the signing call that skips the check."
`scripts/sign-cell.py:261-267` only guards the **prod** key; `python3 scripts/sign-cell.py --in x
--out x` mints a valid **dev** signature with no policy check at all, and every local/QEMU image is
a dev-key build. The phase's contract ("signing is unreachable without a passing check") holds for
the prod key only. Either make `sign-cell.py`'s `main()` refuse unless an internal
`_CHECKED` sentinel is set by `cellos_sign.signing`, or drop the absolute claim in the docstring.

**MJ3 — Scanner false negatives: string literals are not stripped, in both directions.**
`scripts/cellos_sign/scan.py:31-79`. Verified on the real module:

| input | expected | scanner |
|---|---|---|
| `const P: &str = "/*";` + `unsafe { … }` | 1 unsafe | **0** |
| `let u = "a // b"; unsafe { g() }` | 1 unsafe | **0** |
| `const S: &str = "#![forbid(unsafe_code)]";` | no forbid | **`has_forbid() == True`** |
| `r#"/*"#` + `unsafe { }` | 1 unsafe | **0** |

Combining rows 1 and 3 in one crate root yields a file that passes F1 (attribute layer sees a
"forbid", token layer sees no `unsafe`) while rustc compiles real `unsafe` because there is no
actual attribute. `scan.py:12-14` states "false positives are acceptable, false negatives are not";
this construction is a false negative. I checked all 834 tracked `.rs` files and found **no
accidental** instance today (the two apparent hits, `libs/api/src/services/fs.rs:8` and
`third_party/redoxfs/src/mount/redox/scheme.rs`, are the word "unsafe" inside genuine comments), so
this is a deliberate-bypass vector rather than a live miss. Fix: strip string and raw-string
literals in the same pass as comments, and anchor `FORBID_RE` to a line start.

**MJ4 — `#![forbid(unsafe_code)]` does not deliver what `policy.py` says it delivers.**
`scripts/cellos_sign/policy.py:9-12` claims the attribute layer "catches unsafe the token scan is
not looking at (macro-expanded, or in a path dependency)". Both halves are false, verified with
`rustc --edition 2021` on the pinned nightly: a `macro_rules!` exported from another crate whose
expansion contains `unsafe { … }` compiles cleanly inside a `#![forbid(unsafe_code)]` crate, and
`forbid` is per-crate so it never reaches a path dependency. This is exactly the property
`libs/ostd/src/entry.rs:10-11` relies on for `cell_main!` — the mechanism is sound and introduces
no duplicate-symbol hazard beyond the pre-existing `#[no_mangle] fn main` (confirmed: bare
`#[no_mangle]` under `forbid` *is* a hard error, and the macro form is not), but the same escape
hatch is available to any future `ostd` macro. Meanwhile `CELL_ROOTS = ["cells"]` (`policy.py:30`)
means `libs/ostd` is never token-scanned, and `Crate.path_deps` (`scan.py:122`, populated by
`_dep_dirs`) is collected and then never used. Correct the docstring to state the real boundary —
"cells are forbid-clean; `libs/*` is trusted TCB, out of F1 scope" — and either use `path_deps` or
delete it.

**MJ5 — the signing path does not pass `--strict`, so F5 can be silently skipped at sign time.**
`scripts/lib-sign-cells.sh:59` invokes `cellos-sign --quiet --objcopy … --sign …`. `run_check`
treats a skipped F5 as `ok` unless `--strict` is set (`cli.py:72-75`), so a build host with no
usable `rustc` on `PATH` signs cells while printing `SKIP: F5`. Given the signature is defined as
attesting "built by a pipeline that enforced F1 [and F5]" (`lib-sign-cells.sh:40-44`), the sign
path is precisely where `--strict` should be mandatory — CI's `--check` job has it, the signer
does not.

---

## Minor

**MN1 — `yield_cpu()` can return, and the #PF handler then re-executes the faulting instruction.**
`fault_kill_cell` (`paging.rs:987`) relies on `terminate_current_cell_on_fault` never returning,
but `yield_cpu` returns when `pick_next` yields `None` (`task.rs:437-449`). Control then unwinds to
`x86_64_ec_handler` and `iretq`s back to the same store → unbounded fault/kill/log loop on that
hart. Same shape exists on riscv64 (`trap.rs:148-149` notes "We should not reach here"). Low
likelihood (some task is normally runnable) but the loop is silent and unbounded.

**MN2 — stale SMP assumption on the new fault path.** `task.rs:351-353` justifies
`force_unlock_all_kernel_locks()` with "single-hart kernel, interrupts disabled here". `MAX_HARTS`
is 2 and hart 1 runs a real scheduler loop; force-unlocking a spinlock another hart holds is
memory-unsafe. Pre-existing, but this phase newly routes x86_64 user faults through it.

**MN3 — `config_client::get` leaks unboundedly and holds a lock across a blocking IPC.**
`cells/tools/shell/src/config_client.rs:48-77`. The `Box::leak` is genuinely sound and a real
improvement over the previous lifetime-laundering (which returned a `&str` into a buffer the next
call overwrote), but it leaks one `String` per successful call, and `resp_buf.lock()` is held
across `sys_recv` — a config service that never replies parks the lock forever. The doc is honest
about both; the correct fix (change `ViConfig::get` to return `String`) is correctly scoped out.

**MN4 — the surviving `cmd_fs.rs` unsafe blocks are not equally unavoidable.**
`cells/tools/shell/src/cmd_fs.rs:346-348` (raw fn-pointer fast-IPC call) is inherent. But
`:368-375` dereferences a raw pointer supplied *in an IPC response* — `len` is clamped to
`out.len()`, `ptr` is not validated at all. That is an arbitrary-read primitive conditioned on the
VFS cell being honest, i.e. a trust-boundary violation inside a cell that is supposed to be the
F1 exemplar. Avoidable by returning the bytes through a Grant instead of a bare pointer.

**MN5 — file-set gaps the token layer cannot see.** `tracked_sources` (`scan.py:82-106`) scans the
git index — the right call for CI/local parity, and I confirmed the pathspec is complete (337 files
either way). But cargo builds from the **filesystem**, so an untracked `.rs`, and generated code
pulled in by `include!(concat!(env!("OUT_DIR"), …))` (`cells/demos/viui-demo/src/main.rs:10`), are
compiled and unscanned. `forbid` covers both — except in the **26** `[[crate]]` exemptions
(`scripts/unsafe-allowlist.toml`), which include `app-shell`, `service-vfs`, `service-net`,
`service-compositor` and `service-silo`. For those crates neither layer sees such a file. Worth one
sentence in the allowlist header so the exemption's real blast radius is on the record.

**MN6 — `wx-test` does not test the stated threat.** `cells/tests/wx-test/src/main.rs:58-74` proves
only that a cell cannot write **its own** `.text`. The invariant the phase exists for — cell A
cannot rewrite cell B's `.text` in the shared address space — and `.rodata` non-executability are
untested. A second probe writing a *peer* cell's exported symbol address would cost a few lines and
would catch exactly the C1/C3 regressions above.

---

**Status:** BLOCKED
**Summary:** Phase 10's lowering pass is correctly ordered relative to relocation and correctly
merges boundary-page flags, but the merged W+X result is only warned about, the untrusted
`SpawnFromMem` route reaches the loader with no signature gate and a `CellId(0)` that turns the new
fault class into a kernel panic, and the task becomes stealable before enforcement runs. Phase 11's
gate is well-structured but its two absolute claims — "no signing path skips the check" and
"forbid catches macro-expanded and path-dependency unsafe" — are both false as written.
**Verification:** `python3 scripts/cellos-sign --check --strict` → exit 0 (77 crates / 337 files,
46 allowlisted); `python3 scripts/test_cellos_sign.py` → 18 tests OK; `cargo check -p vicell-kernel
--target aarch64-unknown-none-softfloat -Z build-std=core,alloc` → clean. Host `rustc
--edition 2021` probes: bare `#[no_mangle]` under `forbid` errors; external-macro `#[no_mangle]`
and external-macro `unsafe {}` both compile under `forbid`. Direct probes of
`cellos_sign.scan` reproduced four false-negative vectors. Parsed program headers of
`kernel/src/embedded{,-aarch64,-x86_64}/init` to confirm the boundary-merge path is live on shipped
images. Swept all 834 tracked `.rs` files for accidental stripper-induced false negatives: none.
**Concerns/Blockers:** C1 and C2 both hinge on `Syscall::SpawnFromMem` bypassing `spawn_gated`
(`kernel/src/task/syscall.rs:2611-2637`) — that bypass predates this work and is a
signature/authorization hole in its own right; it should be triaged separately and probably first,
since fixing it narrows C1 and closes C2. No runtime evidence exists for either phase; C3 in
particular is only observable under `-smp 2`, which no test in the tree exercises.
