# CP1 — Cell initial stack with an auxiliary vector, and ostd's libc start-up hook

Status: **design check APPROVED by the owner on 2026-11-04 ("duyệt"), AX-1…AX-8 as §2 recommends** ([record](cp1-cell-initial-stack-auxv-approval.md)); route option B chosen the same day. Implementation needs the Intel WIP lane (S1 closed and released it at `b3b5d51e`; the next lane holder is the owner's call). CP2 after implementation, with the exact delta, evidence and an `docs/abi-boundary-records.md` row.
- Written 2026-11-04 from [phase R3](phase-03-mlibc-posix-enablement.md) Deviation Log item 7.
- Every `file:line` was read in this session. `[INFERENCE]` marks claims not read in repo code.

## 1. Problem

mlibc's static start-up (`__dlapi_enter` → `interpreterMain`, mlibc `options/rtld/generic/main.cpp:288-660` at pin `ae1092eb`) builds the Tcb and the static TLS block from `PT_TLS`, seeds the stack guard from `AT_RANDOM`, records argv/environ, then runs `.preinit_array`/`.init_array`. It reads all of it from a Linux initial stack. Cellos gives it nothing:

| # | Fact | Evidence |
|---|---|---|
| E1 | The Cell linker script starts `.text` at VA 0; the ELF and program headers are in no `PT_LOAD`, and there is no `PT_PHDR` | `libs/cell-build/src/cell.ld.in:18-25`; `readelf -l` of `mlibc-smoke` (x86_64) |
| E2 | The root task's user `sp` is the bare top of its user stack; nothing is written there | `kernel/src/task/scheduler.rs:1015-1020` |
| E3 | ostd `_start` walks `.init_array`, calls `main`, then `Exit(0)` | `libs/ostd/src/startup.rs:89-105` (x86_64; same shape on riscv64/aarch64) |
| E4 | mlibc's own constructors need the start-up first: `init_libc` reads `__dlapi_entrystack()`; `mlibc-smoke` already carries 7 mlibc `_GLOBAL__sub_I_*` constructors | mlibc `options/elf/generic/startup.cpp:34-38`; `.init_array` of `target/x86_64-unknown-none/release/mlibc-smoke` |
| E5 | The PIE entry is `e_entry + load_base` | `kernel/src/task/elf_prepare.rs:196` |
| E6 | Spawn arguments already exist as a reviewed byte stash (`SpawnRequest::argv`) | `kernel/src/loader/spawn_request.rs:11-33`; `kernel/src/loader/governed_spawn.rs:203-221` |

## 2. Items

| ID | Item | Recommendation |
|---|---|---|
| AX-1 | Which Cells get the new initial stack | Every Cell, on all three architectures. Code that ignores the stack is unaffected: ostd's `_start` only realigns `sp` (E3) |
| AX-2 | Layout at the entry `sp` (16-byte aligned) | Linux/SysV: `argc`, `argv[]`, `NULL`, `envp[]`, `NULL`, auxv pairs, `AT_NULL`; then the data the pointers name (program-header copy, `AT_RANDOM` bytes). Registers at entry unchanged |
| AX-3 | argv / environment in v1 | `argc = 0`, empty environment. The `SpawnRequest::argv` stash keeps its own path (E6); mapping it into `argv[]` is a later item |
| AX-4 | auxv entries | `AT_PHDR` → kernel copy of the program-header table on the stack; `AT_PHENT`; `AT_PHNUM`; `AT_PAGESZ` = 4096; `AT_ENTRY` = runtime entry; `AT_BASE` = the Cell's **load base** (Cellos has no interpreter, so the slot carries the image base — recorded as a Cellos meaning); `AT_RANDOM` → 16 bytes only when the kernel has an entropy source, otherwise omitted; `AT_NULL` |
| AX-5 | Size bound | Fixed: ≤ 16 program headers (56 B each), ≤ 9 auxv pairs, 16 entropy bytes, ≈ 1.1 KiB. A Cell with more headers is refused at spawn (loud, not truncated) |
| AX-6 | ostd hook | `_start` looks up a weak `__cellos_libc_start` through the GOT. If linked, it calls `__cellos_libc_start(entry_sp)` **instead of** walking `.init_array` (the libc runs the constructors once); then `main`, then `Exit(0)` as today. Without the symbol, today's sequence is unchanged. A weak reference never extracts an archive member, so the libc side must reference the hook strongly: `mlibc-shim` does that in the same change (checked by `nm` on a linked mlibc Cell: `T __cellos_libc_start`; today's `mlibc-posix-x86` link does not contain it) |
| AX-7 | Tier-1 (LBI) Cells | The kernel writes the block into the Cell's own user stack before first entry; no extra mapping, no authority |
| AX-8 | Governance | The initial-stack layout becomes a Law-1-style user contract (outside `libs/api` like D-R6's XSAVE check): CP1 now, CP2 after implementation, an ABI-boundary record row "Cell initial stack v1" |

## 3. vicell side (done, link-proven only)

`third_party/mlibc/sysdeps/vicell/generic/startup.cpp` defines `__cellos_libc_start(uintptr_t *entry_sp)`. It redirects `AT_PHDR`/`AT_PHNUM` to a filtered copy (drops `PT_DYNAMIC`, `PT_INTERP`; leads with a synthetic `PT_PHDR` whose `p_vaddr` = copy − `AT_BASE`) and calls `__dlapi_enter`. Dropping `PT_DYNAMIC` keeps mlibc from re-applying `.rela.dyn`, which the loader already applied.

## 4. Risks

- **Stack budget.** ≈ 1.1 KiB less for `main` on every Cell. Root stacks are 64 KiB or 256 KiB (`kernel/src/task.rs:154-156`, `:703-713`) `[INFERENCE: root tasks use the same table]`.
- **RELRO.** If a future loader maps RELRO read-only, any libc that re-applies relocations would fault; the vicell filter (§3) is what prevents it for mlibc.
- **Entropy.** Without `AT_RANDOM`, mlibc uses its fixed terminator canary (mlibc `options/internal/gcc/stack_protector.cpp:10-19`).
- **Exit.** ostd's `Exit(0)` after `main` skips mlibc's `exit()` (atexit handlers, stdio flush). C code that needs them calls `exit()` itself. Unchanged by this CP; recorded for the guide.

## 5. File set (all S1-held or lane-gated today)

- `kernel/src/task/scheduler.rs` (root spawn: write the block, set `sp`), `kernel/src/task/elf_prepare.rs` (program-header table, load base), the per-arch entry frames under `hal/arch/{riscv,arm,x86}/**` only if `sp` setup lives there `[INFERENCE: not traced]`;
- `libs/ostd/src/startup.rs` (AX-6); `libs/mlibc-shim/**` for the strong hook reference (not S1-held, lands in the same change);
- tests: a loader unit test for the layout per arch; a witness Cell reading its own auxv (behind H-1's launch rows);
- `docs/abi-boundary-records.md` row (CP2).

## 6. Questions for the owner

Approve AX-1…AX-8 as recommended (or name the items to change).
