# ADR-0018 — Translate POSIX onto cell primitives; add runtime profiles instead of emulating Linux

> **Status**: Accepted 2026-09-22.
> **Supersedes**: None. Extends [ADR-0015](0015-dual-mode-hybrid-architecture.md) (tier model)
> and [ADR-0017](0017-dual-browser-strategy-ocel-and-tier3-chrome.md) (first Tier 2 application).

## 1. Context

Linux applications demand POSIX semantics that conflict with the Cellular SAS/LBI model:
`fork` needs a second address space, `mmap` needs a per-process virtual-memory object,
`dlopen` needs a dynamic linker, `pthread` needs a thread runtime, and signals need
asynchronous user-context delivery. Cellos has none of these by design
(`docs/FAQ.md:80-83`: full Linux ABI compatibility "is not a goal").

At the same time the tree already contains a **narrow semantic translation layer** and the
cell-native primitives it would need to grow:

| Layer | What exists today |
|---|---|
| POSIX shim | `libs/api/src/services/posix/` — malloc/stdio/file I/O/entropy/time/TCP client; `fork`/`exec`/`wait`/`kill` are explicit failures (`sysio.rs:141-180`) |
| Process lifecycle | `SpawnFromPath` (12), `SpawnFromMem` (10), `SpawnFromElf` (238), `SpawnReplacement` (421), `ForceExit` (61), `Wait` (8), `NotifyOnExit` (204) |
| Threads | `Spawn` (5) creates a same-cell thread with its own stacks and inherits CellId/CapSet/allowlist/PKU (`kernel/src/task/syscall.rs:3461-3509`); wrapper at `libs/ostd/src/task.rs:38` |
| Memory sharing | `GrantAlloc`/`GrantShare`/`GrantSlice`/`GrantRegister`; `ShmAlloc`/`ShmMap` (SAS identity map, global handle table); domain grants for Tier 2 |
| Services | `RegisterService`/`LookupService`, VFS and Net IPC (`TcpListen`/`TcpAccept` in the net service), `SpawnSetDirs`/`dir_inherit`, `StateStash`/`StateRestore` |
| Porting precedent | Lua 5.4 (vendored C via `cc`), Zig level A/B, DOOM and Tetris-C platform-hook ports, `c-math-smoke` (sin/cos/log/printf/sprintf/fopen/fwrite/setjmp) |

Three kernel facts bound any translation and cannot be engineered around:

1. **One address space per cell** (Tier 1 SAS) or one private domain per cell (Tier 2).
   There is no shared mutable state between two processes to clone or map.
2. **No dynamic linking.** The loader applies PIE with RELATIVE relocations only and has no
   `PT_INTERP`/`DT_NEEDED` path (`kernel/src/loader/reloc.rs:1-9,53-60,131-177`).
3. **No asynchronous signal delivery to user handlers.** A fault terminates the task
   (`hal/arch/riscv/src/rv64/trap.rs`); `SIGCHLD`-like notification is `NotifyOnExit`, not a
   signal frame.

Tier 2 (paged domain, [ADR-0015](0015-dual-mode-hybrid-architecture.md), Spec 22) changes the
*economics* of porting without changing those three facts: ported C/C++ no longer has to be
trusted (LBI), only contained (MMU). That is the first time "cheap to port" and "safe to run"
are the same answer.

## 2. Decision

### 2.1 POSIX is translated in userspace, never as a kernel Linux personality

The translation layer is the existing userspace shim (Tier A: `libs/api/src/services/posix`,
Tier B: mlibc sysdeps). The kernel gains **exactly three** primitives, each justified by a
whole class of applications and none of which is a Linux syscall:

| Primitive | Why the kernel and not the shim | Unlocks |
|---|---|---|
| Per-task TLS base (`tp` on RV64, `TPIDR_EL0` on AArch64, `FS_BASE` on x86_64) | Only the context switch can install it; `tp` is currently `0` for cells and is used by `HartLocal` on RV64 (`kernel/src/task/hart_local.rs:36-38,247-260`) | Every threaded C/C++ runtime, `errno`, thread-safe statics |
| `FutexWait`/`FutexWake` ABI | Wait-on-address needs a park queue; the kernel already has `TaskState::FutexWait` and `futex_wait`/`futex_wake` (`kernel/src/task.rs:2476-2523`) but no userspace opcode | pthread mutex/condvar/semaphore |
| Pipe/stream object (two endpoint caps, ring buffer, EOF, backpressure) | Cross-cell streaming needs a kernel-owned object with a capability boundary; today only a SAS-pointer ring exists (`libs/api/src/services/ring_channel.rs`) | `pipe`, `popen`, `subprocess`, streaming servers |

Everything else stays in userspace: `execve` → spawn + exit, `waitpid` → `NotifyOnExit` +
`Wait`, `kill` → `ForceExit`, `mmap(MAP_ANON)` → heap/arena, `mmap(file, MAP_PRIVATE)` →
eager `ReadFileGrant`, `socket`/`bind`/`listen`/`accept` → Net IPC, `shm_open` → grants,
`poll` → `WaitForEvent` + `RecvTimeout`, `getpid`/`uname`/`chdir`/`getcwd`/file CRUD → the
existing syscall and VFS surfaces.

### 2.2 Three porting lanes, selected by the application's POSIX profile

| Lane | Method | Cost | Precondition |
|---|---|---|---|
| **L1 — relink** | Rebuild C/C++/Zig against the shim; rewrite only the platform layer (init, main loop, I/O, display, input) | Days → weeks | Source available; single-process; no JIT/dynamic plugin |
| **L2 — embed the library** | Port the *library*, not the program: link it into a Rust cell under the `ffi-posix` profile | Hours → days | The value is in the library, not the process model |
| **L3 — guest** | Run the unmodified binary in a Tier 3 Linux VM | Zero code change | Accepts 2–10 s boot and the current volatile guest disk (`docs/guides/tier3b-linux-vm.md:18,91`) |

Application classification that selects the lane:

| Class | Uses | Lane |
|---|---|---|
| A | stdio, file I/O, heap, math, single-threaded | L1 (or L2) |
| B | + OS threads | L1 after §2.1 primitives land; otherwise `-DNO_THREADS` where the app supports it |
| C | + `fork`+`exec` for subprocesses | L1 with a small patch: spawn + IPC + `NotifyOnExit` |
| D | `fork` without `exec`, `dlopen` plugins, JIT, `mmap(MAP_SHARED)` file mappings, closed-source binaries | L3 only |

### 2.3 A language is a runtime profile, admitted by five conditions

Adding a language means adding a **runtime profile** to Spec 18 §2 and Spec 05 §3 plus an
acceptance-ledger witness — not just a build recipe. Admission conditions:

1. Builds a static PIC PIE for a Cellos target (RELATIVE relocations only).
2. Declares its libc tier (POSIX shim or mlibc) with a published symbol contract.
3. Declares the runtime facilities it needs; a language requiring OS threads cannot be
   admitted before the TLS and futex primitives exist.
4. Fits the per-cell budget: 16 MiB heap quota, 32 MiB VA stride, 256 KiB default stack.
5. Does not depend on dynamic loading, JIT, file-backed shared mappings, or async signals.

**Next profile: `cpp-freestanding`** — C++ built with `-fno-exceptions -fno-rtti
-fno-threadsafe-statics`, which is the language of the portable Linux application corpus and
today has zero support in the tree (no `.cpp`, no `cxx` crate, no unwinder configuration).
Escalation to hosted C++ (exceptions, RTTI, thread-safe statics) is gated on the TLS and
unwinder work, not on this ADR.

Managed runtimes stay at Lua 5.4. `docs/research/ecosystem.md` (Rank 1) already ruled that a
second managed runtime before G2 violates YAGNI, and RustPython is a hard no
(`docs/research/ecosystem.md:424-430`). MicroPython-class or QuickJS-class runtimes are
admissible only when a named application demands them, and they enter as `ffi-posix` cells.

### 2.4 The porting kit is a deliverable, not documentation debt

Porting cost is currently unpredictable because the shim surface is ad hoc. The kit is:

1. **Published shim contract** — every exported symbol, its semantics, and the exact
   `ENOSYS`/`-1` behaviour of everything else, enforced by a test that fails when the table
   and the implementation diverge.
2. **Platform layer** — one C header plus a Rust host cell providing `main()`, VFS I/O, net,
   compositor surface, input, and time, generalising the DOOM/Tetris platform-hook pattern so
   a port is "implement N hooks", not "learn the ABI".
3. **Build recipes** — CMake toolchain file, Meson cross file, and a `cellos-cc` wrapper for
   the Cellos targets (mlibc's `sysdeps/vicell` meson build is the precedent).
4. **Substitution policy** — `mmap(MAP_ANON)` → heap arena; `mmap(file, MAP_PRIVATE)` → eager
   read into private pages; `fork`/`dlopen`/`mprotect` → fail loudly.

### 2.5 Divergence must fail loudly

A translated call that cannot preserve POSIX semantics returns an error; it never
approximates silently. `fork()` is `ENOSYS`, `dlopen()` is `ENOSYS`, `mmap(MAP_SHARED)` on a
file is `ENOSYS`. Silent divergence produces applications that appear to work and corrupt
data under load, which is strictly worse than a clean refusal.

## 3. Rejected alternatives

- **In-kernel Linux syscall ABI (WSL1-style personality).** Requires raw-pointer semantics,
  `/proc`, and an ever-growing syscall surface inside the kernel, breaks the LBI/SAS invariant
  that user pointers are validated at the ABI boundary (Spec 22 §2.4), and is a compatibility
  treadmill: WSL1 was replaced by a VM precisely because coverage and performance never
  converged. Rejected: unbounded scope, direct conflict with the security model.
- **`fork()` by copy-on-write clone of a Tier 2 domain.** The page table could be cloned
  (domains own a mapping ledger, Spec 22 §2.1) but the kernel-side cell state cannot: open
  handles, capability sets, service-registry bindings (one provider per `service_id`,
  `kernel/src/cell/service_registry.rs`), IPC queues, and cell generation have no clone
  protocol. Cost is large, value is narrow (plain `fork` without `exec` is rare), and it would
  break a shipped invariant. Rejected.
- **Dynamic linking / shared objects.** The loader supports RELATIVE relocations only; adding
  a userspace ELF loader, symbol resolution, and per-library W^X windows is a project of its
  own and contradicts W^X discipline. The plugin model in Cellos is a spawned cell with a
  stable ABI, not a `.so`. Rejected.
- **Asynchronous POSIX signal delivery.** A signal frame, dispositions, masks, `sigaltstack`,
  and `EINTR` semantics are a new kernel ABI with a wide blast radius, and the applications
  that truly need handler-based signals also need `fork`/JIT, i.e. they belong in Tier 3.
  Rejected for now; `NotifyOnExit`/`ForceExit` cover the process-control subset.
- **A second managed runtime (wasmi/Boa/QuickJS/MicroPython) before demand.** `ecosystem.md`
  Rank 1 already rejected this as YAGNI; QuickJS is admitted only through ADR-0017's Ocel
  application, as an `ffi-posix` library. Rejected as a general platform move.
- **Send everything to Tier 3.** Zero porting cost per app, but 2–10 s boot, a volatile guest
  disk, and no path to native Cellos applications. It also forfeits the whole point of the
  tier model. Rejected as a strategy (it remains the correct answer for class D).

## 4. Consequences

- Tier 2 becomes the landing zone for ported untrusted native code; Tier 1 remains for trusted
  first-party cells. This is the model ADR-0017 already assumed for Ocel.
- The shim becomes a maintained product surface with an owner, a published contract, and a
  test that keeps the contract honest.
- `fork`/`dlopen`/`mprotect` remain permanent `ENOSYS`; a porting candidate that needs them is
  classified class D and routed to Tier 3 rather than patched around.
- Full CPython, Node.js, the JVM, nginx, and PostgreSQL stay Tier 3. A CPython-lite build
  (static, frozen stdlib, no extension modules, no `multiprocessing`) becomes *possible* only
  after the TLS and futex primitives land, and is not promised by this ADR.
- Each admitted language costs an acceptance-ledger row; "supported language" is a claim the
  ledger must be able to witness, not a README sentence.
- The three kernel primitives are ABI additions and must follow the existing ABI process
  (Law 1 confirmation, allowlist bits, negative tests), not be smuggled in as loader changes.

## 5. Cross-references

| Topic | Document |
|---|---|
| Execution tiers, runtime profiles, admission | `docs/specs/18-cell-trust-tiers.md` |
| Tier 2 implementation gate and admission control | `docs/specs/22-native-domain-cell-implementation-gate.md` |
| Tier 2 admission control ruling | `docs/decisions/0019-tier2-admission-control-on-path.md` |
| Application tiers, profiles, SDK modules | `docs/specs/05-application.md`, `docs/decisions/0003-application-tier-taxonomy.md` |
| FFI/POSIX profile guide | `docs/guides/tier1b-c-zig.md` |
| Dual-mode hybrid architecture | `docs/decisions/0015-dual-mode-hybrid-architecture.md` |
| First Tier 2 application (Ocel) | `docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md` |
| Runtime/language research and verdicts | `docs/research/ecosystem.md` |
| IPC wire contract (what cannot cross a domain boundary) | `docs/specs/17-ipc-wire-contract.md` |
| Implementation plan | `.agents/260922-1549-cell-native-portability-program/plan.md` |
