# P1 Design — PAL mapping table + `x86_64-unknown-cellos` target JSON draft

> **Status:** **RATIFIED 2026-07-23** (user approved with design-p00 the same day); code post-G3.
> Companion: [design-p00](design-p00-kernel-prereqs-note.md) (thread/futex/TLS ABI this table
> depends on). Syscall numbers verified against `libs/api/src/abi/syscall.rs` `From<usize>`
> this session; NEW = proposed in design-p00 N1/N3 (Law-1 batch, numbers re-audited at impl).

## 1. PAL mapping table (facility → `sys/<facility>/cellos.rs` → cellos-abi → syscall)

| std facility | PAL file (std fork) | cellos-abi call | Syscall # | Notes |
|---|---|---|---|---|
| `alloc` (Vec/String/HashMap) | `sys/alloc/cellos.rs` | none — static-region `GlobalAlloc` | — | Same model as ostd's cell heap (`static mut` region in `.bss`, linked-list allocator; RELRO gotcha: **no `LockedHeap`** — memory `project-cell-heap-and-linker`). Region size from a shim const; charged to cell quota via `.bss` |
| `thread::spawn` | `sys/thread/cellos.rs` | `spawn(entry, arg, tls_base)` | `Spawn = 5` (+ additive a2, P0/N5) | PAL allocates TLS block from heap, kernel allocates in-slot guarded user stack |
| `thread::join` | 〃 | `wait(tid)` | `Wait = 8` (**allowlist bit 9** — goes in the manifest) | Blocks until tid dies, returns exit code. Review-corrected: raw 3 = `Reply`, NOT Wait |
| worker thread exit | 〃 (thread_start tail) | `thread_exit(code)` | **NEW `ThreadExit = 243`** | Runs `destructors::run()` first (P0/N7); refcount-- in kernel |
| `Mutex/Condvar/RwLock/Once/park` | `sys/sync/*/futex.rs` — **platform-agnostic, zero edits** | `futex_wait(addr, val, timeout_ticks)` / `futex_wake(addr, count)` | **NEW `FutexWait = 240` / `FutexWake = 241`** | `pal/cellos/futex.rs` ≈ 50 LOC (Hermit's is 49). `wake_all` = count `usize::MAX`. Timeout in **SCHEDULER ticks (10 ms)** — same clock as `RecvTimeout`; PAL converts `Duration → ceil(ms/10)` (P0/N1, review-corrected from MTIME) |
| main-thread TLS init | entry shim (`os::cellos` runtime) | `set_tls(base)` | **NEW `SetTls = 242`** | After building main TLS image from `__tdata_*` linker symbols (P0/N6) |
| `Instant::now` | `sys/time/cellos.rs` | `get_time(op=0/1)` | `GetTime = 120` | **Monotonic unit is PER-ARCH** (review-corrected — `MTIME_TICKS_PER_MS=10_000` is riscv-only): riscv 10 MHz MTIME · aarch64 62.5 MHz CNTPCT · **x86_64 HPET nanoseconds** (`kernel syscall.rs:2617-2641`). cellos-abi exposes a per-target `MONO_TICKS_PER_SEC` const; `sys/time/cellos.rs` converts with it |
| `SystemTime::now` | 〃 | `get_time(op=2/3)` | `GetTime = 120` | Wall clock (Goldfish RTC / CMOS — memory `project-rtc-wall-clock`) |
| `print!/eprint!` (stdout/stderr) | `sys/stdio/cellos.rs` | `log(ptr, len)` | `Log = 11` | Both streams → kernel log ring (console); stdin → `Unsupported` |
| `env::args` | `sys/args/cellos.rs` | `state_restore(ARGV_STASH_KEY, buf)` | `StateRestore` (spawn-args stash — `ostd/args.rs:35`, `syscall.rs:1099-1107`) | Read once at init, cache; spawner publishes via `sys_set_spawn_args` |
| `env::var/set_var` | `sys/env/cellos.rs` | none — in-memory map | — | No kernel env; process-local map seeded empty (wasm/hermit posture) |
| `HashMap` RandomState | `sys/random/cellos.rs` | `get_random(buf)` loop | `GetRandom = 214` | 64 B/call cap → loop until seed filled (Hermit `read_entropy` pattern) |
| `process::exit` / `main` return | entry shim | `exit(code)` | `Exit = 60` | Whole-cell death incl. sibling threads (P0/N3) |
| `thread::yield_now` | `sys/thread/cellos.rs` | `yield()` | `Yield = 104` | |
| `abort` (panic=abort) | PAL abort | `exit(101)` | `Exit = 60` | Whole-cell abort — never-die supervisor observes (P0/N3) |
| `fs` / `net` / `process::Command` / `pipe` | **no cellos arm** | — | — | Fall through to `unsupported` in `sys/*/mod.rs` (P1 posture; fs/net arrive in P2). **Doctrine firewall** |

**cellos-abi crate contract** (`libs/cellos-abi/`, `#![no_std]`, zero deps, pure
numbers + `#[repr(C)]` + raw `syscall()` asm wrappers per arch): decoupled from Law-1
`libs/api` — it duplicates the *numbers* (compile-time-asserted equal to `ViSyscall` values in
a test build) so the std fork never links `libs/api`. Error mapping table `ViError`/negative
returns → `io::ErrorKind` lives here too (one place, both P1 and P2 reuse).

## 2. Target JSON draft — `targets/x86_64-unknown-cellos.json`

```json
{
  "llvm-target": "x86_64-unknown-none",
  "arch": "x86_64",
  "os": "cellos",
  "vendor": "unknown",
  "target-pointer-width": "64",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
  "max-atomic-width": 64,
  "panic-strategy": "abort",
  "relocation-model": "pic",
  "position-independent-executables": true,
  "static-position-independent-executables": true,
  "crt-static-default": true, "crt-static-respected": true,
  "executables": true,
  "linker": "rust-lld", "linker-flavor": "ld.lld",
  "features": "-mmx,-sse",
  "rustc-abi": "x86-softfloat",
  "disable-redzone": true,
  "tls-model": "initial-exec",
  "has-thread-local": true,
  "singlethread": false,
  "supported-sanitizers": [],
  "requires-uwtable": false, "default-uwtable": false
}
```

Rationale for the load-bearing fields:
- **`-mmx,-sse` + `rustc-abi: x86-softfloat` + `disable-redzone`:** cells today build for
  `x86_64-unknown-none` (same defaults) and the kernel does NOT save FPU/SSE state on context
  switch (verified) — enabling SSE would corrupt sibling threads' FP state. serde/regex/clap
  are fine on soft-float. Review-corrected mechanism: modern rustc uses `rustc-abi:
  "x86-softfloat"` (the repo's own `kernel/x86_32-unknown-none.json` already does), not the
  deprecated `+soft-float` feature; `llvm-target` carries no `-elf` suffix (matches the
  built-in `x86_64-unknown-none`). Verify both against the pinned nightly before P1 step 3.
  Revisit SSE (xsave lazy-save) only if a real workload needs it — separate decision, not G4.
- **PIC/PIE static:** cells are PIE, loaded at a `va_alloc` slot (`relocation-model=pie`
  already mandated — memory `project-pie-cell-boot-fixes`); P0/N2's futex check *depends* on
  the slot model.
- **`tls-model: initial-exec` + `has-thread-local`:** static TLS via linker symbols (P0/N6);
  no dynamic linking exists, so initial-exec is exact, not an optimization.
- **`panic-strategy: abort`:** locked brainstorm decision; `-Zbuild-std=std,panic_abort`.
- **`os: cellos`:** the cfg key every `sys/*/mod.rs` arm and the `ostd-ext` lang-item gate
  (C4) switch on; also what makes `std::os::unix` absent (doctrine firewall).
- aarch64 twin (`aarch64-unknown-cellos.json`): same shape, `features: "+strict-align"`,
  `max-atomic-width: 128`, `disable-redzone` n/a; drafted in P1 step 3 from this template.

## 3. Entry/link chain (C4 seam, recorded here so the table is complete)

```
ENTRY __vicell_std_start (shim, os::cellos runtime crate)
  ├─ emits __ViCell_manifest + __ViCell_syscalls sections   (manifest gate + signing)
  ├─ .ld template: KEEP both sections; PROVIDE __tdata_start/__tdata_end/__tbss_end (N6)
  ├─ init PAL heap region → build main TLS → SetTls(242) → run std lang_start(main)
  └─ destructors::run() → Exit(60)
```
Manifest allowlist a std cell MUST declare: `Futex` (new bit) + `Spawn` + **`Wait` (bit 9 —
review-corrected: it is gated, NOT always-permitted)** + `GetRandom` + `GetTime` + `Log` +
`StateRestore` (+ P2 adds Vfs*/Net*). `ThreadExit`/`SetTls`/`Yield`/`Exit` are
always-permitted.

## 4. Consequences fed forward

- **P0 must land first** — 5 of the table's rows dial NEW syscalls (240-243 + Spawn a2).
- The Law-1 batch (one 2×-confirm event at implementation): FutexWait/FutexWake/SetTls/
  ThreadExit numbers + one `Futex` allowlist bit + Spawn additive a2. Everything else in the
  table uses shipped syscalls (5/8/60/104/120/11/214 + stash) — verified this session +
  review-corrected (Wait = 8, bit 9; raw 3 is Reply).
- M1 oracle unchanged: `STD-COMPUTE: PASS` = serde_json + regex + clap **signed + booted**
  on x86_64 via this target JSON + `-Zbuild-std=std,panic_abort`.
