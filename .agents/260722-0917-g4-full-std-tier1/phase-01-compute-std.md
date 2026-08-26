# Phase 01 — Compute std: target JSON + cellos-abi + minimal PAL

## Context Links
- Plan: [plan.md](plan.md) · Depends on: [phase-00](phase-00-kernel-prereqs.md)
- rustc-TCB: `docs/specs/16-rustc-tcb.md` (toolchain pin F5) · App tiers: `docs/specs/05-application.md`
- Precedent: Hermit `library/std/src/sys/` (post-monolith layout) + `hermit-abi` crate

## Overview
- **Priority:** P1. **Status:** **mapping table + target JSON RATIFIED 2026-07-23** —
  [design-p01](design-p01-pal-mapping-target-json.md) (16-row facility table, syscall numbers
  verified; x86_64 target JSON draft: soft-float/no-SSE because the kernel saves no FPU state);
  code post-G3.
- Stand up a `std`-capable target with only **compute** facilities: alloc, thread, futex-backed sync,
  time, stdio, env, args, random. **fs/net/process/pipe = `ErrorKind::Unsupported`.**
- **Also (red-team C4/M4):** split ostd's lang items into `ostd-ext`, add a std-compatible entry shim
  that still emits `__ViCell_manifest`/`__ViCell_syscalls`, and stand up the **x86_64 build/sign/boot**
  pipeline. "Compiles unmodified" is not the bar — **booted-and-signed** is.
- **Milestone M1:** `serde_json` + `regex` + `clap` build unmodified via `-Zbuild-std`, are **signed and
  packaged into an x86_64 cell-store, and boot-run** in a Tier 1 cell (not just "compile").

## Key Insights (from research, verified)
- **Modern std PAL is a thin per-facility slice, not a monolith.** `sys/pal/hermit/` = only `mod.rs`
  (122 LOC) + `futex.rs` (49). Real work lives at `sys/<facility>/hermit.rs`. Cellos copies this shape:
  create `sys/<facility>/cellos.rs` + add a `target_os="cellos"` arm to each facility's `mod.rs`
  `cfg_select!`. **Do NOT resurrect the old `pal/<os>/` monolith.**
- **Futex is the highest-leverage primitive.** `sys/sync/{mutex,condvar,rwlock,once,thread_parking}/
  futex.rs` are **platform-agnostic** — they need only `futex_wait`/`futex_wake`/`futex_wake_all`.
  **CORRECTED 2026-07-23 (design-p00):** the kernel futex machinery (`task.rs:1493-1529`) is
  **unreachable from userspace** — `ViSyscall` has no Futex entries, nothing constructs
  `Syscall::FutexWait`, and raw 10 = SpawnFromMem. P0 builds the user ABI fresh (NEW ViSyscall
  FutexWait/FutexWake + timeout + SetTls + ThreadExit — one Law-1 batch) and must also fix the
  existing check-outside-lock lost-wakeup bug. Still Boundary-Law-legal scheduler mechanism.
- **`hermit-abi` = pure FFI/constants (2 files, zero logic).** Cellos analog = a new **`cellos-abi`**
  crate: raw syscall numbers + `#[repr(C)]` structs, `#![no_std]`, no deps. All `io::Result` wrapping
  lives in the std-fork `sys/*/cellos.rs`. Keeps the std fork **decoupled from Law-1 `libs/api`**.
- **No compiler fork needed for M1.** JSON target (`os=cellos`, `panic-strategy=abort`,
  `tls-model=initial-exec`, `has-thread-local=true`, PIE, `linker=rust-lld`) + forked rust-src (edit
  `library/std` only) + stock pinned nightly `-Zbuild-std=std,panic_abort`. Compiler fork is P5 only.
- **process/pipe: zero PAL work** — omit the cellos arm; they fall to `sys/{process,pipe}/mod.rs`
  `_ => unsupported`. Same posture as wasm32-wasip1. **Do not emulate fork** (violates SAS/no-fork).
- **fs/net in P1: also Unsupported** — omit their cellos arms so they fall through; P2 adds them.
- TLS destructors are **program-structure-enforced** (`destructors::run()` in `runtime_entry` +
  `thread_start`), not OS-callback. The P0 thread trampoline + cell-exit path MUST call it.
- **[C4 — ostd is NOT a free bystander; "beside std like hermit-abi" is a false analogy.]** ostd carries
  **singleton lang items** hermit-abi does not: `#[global_allocator]` (`libs/ostd/src/heap.rs:68`),
  `#[alloc_error_handler]` (`heap.rs:76`), `#[panic_handler]` (`startup.rs:120`), and `_start`/ENTRY
  (`startup.rs:24`). A std cell that links ostd for `ctx.vfs()`/`ctx.net()` pulls **duplicate lang items**
  → "cannot define multiple global allocators" link error. **Fix: split `ostd-ext`** — the ergonomic
  helpers with lang items `#[cfg]`-gated OFF for `target_os="cellos"` (std's PAL provides allocator/panic
  runtime). This is a real refactor, budgeted here.
- **[C4 — a std cell emits no manifest/syscalls and fails signing.]** Only `app_entry!`/`declare_manifest!`
  emit `__ViCell_manifest` + `__ViCell_syscalls` (`runtime.rs:224-228`); the `.ld` KEEPs them;
  `gen_disk.ps1:174` signs over them. A cell entered via std's `lang_start` emits **neither** → kernel
  manifest gate + signing pipeline reject it. **Fix: a std entry shim (an `os::cellos` `declare_manifest!`
  equivalent) that is the sole ENTRY, emits both sections, then calls `lang_start`.**
- **[M4 — x86_64 build/sign/boot is unbudgeted.]** `.cargo/config.toml` default target = riscv64;
  `rust-toolchain.toml` pins **stock** nightly + rust-src (not a fork); `gen_disk.ps1:117,162,174` builds
  with `-Zbuild-std=core,alloc` + `riscv-none-elf-objcopy` + riscv signing. A std cell needs
  `-Zbuild-std=std,panic_abort` against a **forked rust-src via `rustup toolchain link`** (a different
  sysroot than the pinned rust-src) and an **x86_64 objcopy/sign/boot** branch. `gen_disk`'s riscv path
  cannot package a signed x86_64 std ELF.

## Requirements
- **Functional:** `println!`, `Vec`/`String`/`HashMap`, `thread::spawn`+`join`, `Mutex`/`Condvar`,
  `Instant::now`/`SystemTime::now`, `env::var`/`args`, `HashMap` default RandomState (needs random) all
  work. `File::open`/`TcpStream::connect`/`Command::new().spawn()` return `Unsupported` cleanly.
- **Non-functional:** builds only on the pinned nightly (`rust-toolchain.toml`, F5); `#![forbid(unsafe)]`
  still holds for the *cell* (unsafe lives in the std fork + cellos-abi, outside the cell TCB per spec 16 §4).

## Architecture / data flow
```
cell app (std)  ──▶ std::sys::<facility>::cellos  ──▶ cellos-abi (raw syscall)  ──▶ kernel
   Vec/HashMap  ──▶ sys/alloc/cellos.rs           ──▶ cell heap (bump/global)
   thread::spawn──▶ sys/thread/cellos.rs          ──▶ sys_spawn(thread path, P0)  + join
   Mutex/Condvar──▶ sys/sync/*/futex.rs (FREE)    ──▶ sys/pal/cellos/futex.rs ──▶ Futex 9/10 (+timeout)
   Instant/Time ──▶ sys/time/cellos.rs            ──▶ GetTime 120 (op0/1 mono; op2/3 wall)
   print/eprint ──▶ sys/stdio/cellos.rs           ──▶ sys_log (11); stdin=Unsupported
   env/args     ──▶ sys/env,args/cellos.rs        ──▶ spawn-args stash / in-mem map
   RandomState  ──▶ sys/random/cellos.rs          ──▶ GetRandom 214 (loop; 64B/call cap)
```

## Related Code Files
- **Create (new crate):** `libs/cellos-abi/` — `src/lib.rs` (syscall nums, extern/asm wrappers),
  `src/errno.rs`-equivalent (`ViError` codes ↔ `io::ErrorKind` table). `#![no_std]`, no deps.
- **Create (std fork, `library/std/src/sys/`):** `pal/cellos/mod.rs`, `pal/cellos/futex.rs`,
  `alloc/cellos.rs`, `random/cellos.rs`, `env/cellos.rs`, `thread/cellos.rs`, `time/cellos.rs`,
  `stdio/cellos.rs`, `args/cellos.rs`.
- **Modify (std fork):** `cfg_select!` in `sys/{alloc,random,env,thread,time,stdio,args}/mod.rs`
  (add `target_os="cellos"` arm); `sys/thread_local/mod.rs` (native backend via `target_thread_local`);
  compiler target: ship `targets/x86_64-unknown-cellos.json` (repo `targets/` dir).
- **Modify (kernel, small):** `kernel/src/task.rs` futex_wait → add optional `timeout_ticks`;
  `libs/api` syscall enum only if the futex ABI changes shape → **Law 1: 2× confirm** (prefer a new op
  arg that keeps the discriminant stable).
- **Create:** `cells/apps/std-smoke/` — a Tier 1 cell using serde_json+regex+clap.
- **Create/refactor (C4):** `libs/ostd-ext/` (ergonomic helpers, lang items `#[cfg]`-gated off for
  `target_os="cellos"`); an `os::cellos`-side `declare_manifest!`-equivalent + std entry shim that emits
  `__ViCell_manifest`/`__ViCell_syscalls` and is the sole ENTRY before `lang_start`.
- **Modify (M4 build pipeline):** `.cargo/config.toml` (x86_64 std target profile); `gen_disk.ps1`
  (x86_64 objcopy + signing + cell-store branch, alongside the riscv path at :117,162,174); toolchain-
  link step for the forked rust-src sysroot; an x86_64 boot-smoke that runs the signed std cell.

## Implementation Steps
1. **(Now)** Write the PAL mapping table (facility → cellos-abi call → syscall#) + target JSON draft;
   review during G3 window.
2. Scaffold `libs/cellos-abi` with syscall numbers from the verified ostd map (Send 0, Recv 1, TrySend 4,
   TryRecv 7, RecvTimeout 201, Spawn/thread path, Exit 60, Yield 104, Log 11, GetTime 120, GetRandom 214,
   Futex 9/10, spawn-args stash).
3. Fork rust-lang/rust at the pinned nightly; add the 9 `sys/*/cellos.rs` files + cfg arms.
4. Wire `runtime_entry` → set up main-thread TLS (P0), call user `main`, run `destructors::run()`, `sys_exit`.
5. Add futex timeout in kernel; wire `pal/cellos/futex.rs` (wait+timeout, wake, wake_all).
6. **(C4)** Split `ostd-ext`; cfg-gate lang items off for cellos; write the std entry shim emitting
   `__ViCell_manifest`/`__ViCell_syscalls`; verify a std cell links with no duplicate lang items.
7. **(M4)** Link the forked rust-src as a `rustup` toolchain; add x86_64 build+objcopy+sign+cell-store
   branch to `gen_disk.ps1`; build `std-smoke` via `-Zbuild-std=std,panic_abort
   -Zbuild-std-features=compiler-builtins-mem --target x86_64-unknown-cellos.json`.
8. **Sign** the std cell; confirm it passes the kernel manifest gate; **boot** it in QEMU x86_64; assert
   serde round-trip, a regex match, and clap arg parse print to console. M1 is done only when **booted+signed**.

## Todo List
- [x] PAL mapping table + target JSON (now-able) — drafted 2026-07-23,
      [design-p01](design-p01-pal-mapping-target-json.md)
- [ ] `libs/cellos-abi` crate
- [ ] rust-src fork: pal/cellos/{mod,futex}
- [ ] sys/{alloc,random,env,thread,time,stdio,args}/cellos.rs + cfg arms
- [ ] kernel futex timeout arg (+ Law-1 review if ABI shape changes)
- [ ] thread_local native + destructors::run() in runtime_entry
- [ ] (C4) `ostd-ext` split; lang items cfg-gated off for cellos; no duplicate-lang-item link error
- [ ] (C4) std entry shim emits `__ViCell_manifest` + `__ViCell_syscalls`; passes manifest gate
- [ ] (M4) forked-rust-src toolchain link + x86_64 objcopy/sign/cell-store branch in gen_disk.ps1
- [ ] std-smoke cell builds via build-std, is **signed**, and **boots** on x86_64
- [ ] QEMU: serde_json + regex + clap run → `STD-COMPUTE: PASS`
- [ ] fs/net/process/pipe confirmed `Unsupported` (not panic)

## Success Criteria
- QEMU boot on **x86_64** of a **signed, manifest-bearing** std cell (passed the kernel manifest gate):
  `std-smoke` deserializes a JSON blob (serde_json), matches a regex (regex), parses argv (clap), prints
  results via console. Serial oracle: `STD-COMPUTE: PASS`. **M1 gates on booted-and-signed, not compiles.**
- `File::open`/`TcpStream::connect`/`Command::spawn` return `io::ErrorKind::Unsupported` (asserted).
- A std cell linking `ostd-ext` produces **no duplicate lang-item link error**.
- aarch64 target JSON added and `std-smoke` builds (run-verify may trail x86_64).

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| **[RESEARCH GAP]** exact `sys/*/mod.rs` cfg-dispatch shape drifts by nightly | M×M | Pin nightly (F5); trace `cfg_select!` arms in the checked-out fork before editing; layout verified for main, reconfirm at pinned rev |
| build-std against a *forked* rust-src needs a custom sysroot/toolchain-link | M×M | Build toolchain from fork via x.py + `rustup toolchain link` (documented Hermit path); OR vendor rust-src — decide in step 1 |
| Kernel futex has no timeout → `Condvar::wait_timeout` blocks forever | M×H | Add `timeout_ticks` arg to futex_wait (step 5); until then, `wait_timeout` maps to `Unsupported`, gated by a test |
| GetRandom 64B/call cap → `RandomState` seed slow / partial | L×M | Loop in `random/cellos.rs` until filled (matches Hermit `read_entropy` loop) |
| Two tick-unit systems (mtime ticks vs ms) mismatched in `time/cellos.rs` | M×M | Use `MTIME_TICKS_PER_MS` (ostd lib.rs:78) constant; unit test Instant delta vs GetTime op1 |
| Thread inherits parent allocator/allowlist but PAL needs syscalls not in manifest | M×H | `std-smoke` manifest allowlist must include Futex/Spawn/GetRandom/GetTime; fold std-thread syscall set into `app_entry!` (runtime.rs:47-110) — track as P0/P1 seam |
| **[C4] Duplicate lang items** (ostd + std both define allocator/panic) → link failure | H×H | `ostd-ext` split with lang items cfg-gated off for cellos; std PAL owns allocator/panic; verified by a link test before M1 |
| **[C4] std cell emits no `__ViCell_manifest`/`__ViCell_syscalls`** → rejected by manifest gate + signing | H×H | std entry shim (os::cellos declare_manifest! equiv) is sole ENTRY, emits both sections; gate on passing signing before M1 |
| **[M4] x86_64 sign/boot path does not exist** (gen_disk is riscv objcopy) | H×H | Add x86_64 objcopy/sign/cell-store branch + forked-rust-src toolchain link + x86_64 boot smoke; M1 = booted-and-signed |

## Security Considerations
- The std fork + cellos-abi contain `unsafe` — they are **outside the cell TCB** (spec 16 §4: std is
  Cell-visible only, TCB does not extend to std internals). The cell keeps `#![forbid(unsafe_code)]`.
- Toolchain pin (F5) is load-bearing: an unpinned nightly silently changes std internals + ABI.
- `Unsupported` for fs/net/process is a **doctrine firewall**, not a limitation — POSIX-shaped crates
  fail early, routing the workload to Tier 3 (Scope Doctrine).

## Next Steps
- Unblocks P2 (fs/net facilities), P4 (os::cellos), P3 (async needs std::net from P2 + thread from here).
