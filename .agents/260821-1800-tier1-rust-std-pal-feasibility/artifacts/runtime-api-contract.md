# Cellos Rust `std` Runtime and API Contract

Contract ID: `CELLOS-RUST-STD-RUNTIME-v1`
Scope source: [`pal-hook-support-map.json`](pal-hook-support-map.json)
State: frozen feasibility contract; no implementation authorization.

## Universal Invariants

- `std` creates no authority. The existing admission manifest, syscall allowlist, resource capability, and service policy remain decisive.
- Denial, absence, unsupported behavior, exhaustion, and transport failure remain distinguishable. No denied or unsupported operation returns success, zero bytes, EOF, an empty environment, or synthetic data unless that exact value is the documented predicate result.
- Unsafe inputs must be checked before dereference or write: pointer provenance, checked bounds, alignment, complete caller-owned writable mapping for outputs, initialized output, handle ownership, and reply bounds. Current `GetRandom` does not meet this invariant because it constructs a mutable slice without `validate_user_buf`; therefore `PAL-019` and `PAL-031` remain Deferred.
- Any frozen `libs/api/src/abi.rs` change is a blocker requiring repository-mandated 2× explicit confirmation; this contract requires none.

## Lifecycle

| Family | Status / hooks | Frozen behavior |
|---|---|---|
| Startup | Supported backing; PAL glue Deferred (`PAL-002`, `PAL-005`, `PAL-032`) | Loader enters one of the existing x86_64/aarch64/riscv64 `_start` paths on a fresh aligned stack. PAL init is once-only before Rust main. Init failure aborts. Argument pointers remain loader-owned for the declared handoff interval. On aarch64, any required `.init_array.90` compiler-builtins initializer executes once in link order before user initialization; missing ordering or feature detection blocks the image. |
| Normal exit | Supported (`PAL-003`, `PAL-006`) | Main return becomes the exact Cellos exit code. Normal cleanup and language-TLS destructors run at most once before `ViSyscall::Exit`. Abort, OOM, or double panic does not promise cleanup. |
| Panic/unwind | Abort-only contract; personality Deferred (`PAL-004`, `PAL-029`, `PAL-030`, `PAL-033`) | `panic=abort`; unwinding, successful `catch_unwind`, and backtraces are Unsupported. The target still supplies the compiler-required aborting/C-unwind personality behavior without importing a foreign unwinder. Panic logging uses only admitted Log and must not add addresses, secrets, or foreign cell data. |
| Allocation | Supported backing (`PAL-007`) | The current freeing allocator is per-cell. Layout alignment is honored; allocation/deallocation ownership never crosses cells. Zero-sized allocation follows Rust's allocator contract. Null reaches the allocation error handler and terminates; it is never dereferenced or presented as successful allocation. Current single-task/single-hart non-overlap is mandatory. |
| Language TLS | Deferred (`PAL-003`, `PAL-026`) | Candidate is pinned `no_threads` static TLS. Normal-exit destructors run in reverse registration order; abort has no destructor guarantee. Access after destruction follows Rust TLS failure. Network TLS in `ostd::clients::tls_stream` is a network protocol and is unrelated. |

## Public API Families

| API family | Classification | Observable behavior and authority |
|---|---|---|
| Arguments | Deferred (`PAL-008`, `PAL-010`) | Per-task private spawn arguments; preserve bytes/encoding. Resolve current one-shot storage versus repeatable `std::env::args_os` before implementation. |
| Environment and target constants | Unsupported mutable environment; target constants Deferred (`PAL-009`, `PAL-035`) | No ambient environment. Enumeration and mutation fail Unsupported; lookup must not collapse denial into ordinary absence. `std::env::consts` must use an explicit Cellos target row; the pinned empty unknown-target fallback and foreign OS identity are prohibited. |
| Files / directories | Deferred (`PAL-012`, `PAL-013`, `PAL-016`) | Candidate open/read/write/seek/stat/close/read_dir routes only through held capabilities and admitted VFS/syscalls. Denial maps PermissionDenied, missing resource NotFound, unavailable methods Unsupported. No cwd/home/temp/current-exe globals. |
| Standard I/O | Deferred (`PAL-020`, `PAL-021`) | stdout/stderr may use admitted logging; stdin may use admitted input service. Unavailable/denied operations fail observably. `is_terminal` is false and grants nothing. |
| Network | Deferred (`PAL-014`, `PAL-015`) | Candidate TCP/UDP uses the admitted net service. No host sockets, ambient DNS/hostname, or implicit listen authority. Unsupported options return Unsupported. |
| Processes / pipes | Unsupported (`PAL-017`, `PAL-018`) | `std::process` and anonymous pipes are unavailable. Explicit privileged Cell spawning is not re-exported as a process API. No shell, PATH search, inherited descriptors, or environment. |
| OS threads / parking | Unsupported (`PAL-024`) | Cells remain one task. No spawn, join, OS ID, name, second stack, or parking primitive. A cell is never represented as another OS thread. |
| Thread query/yield/sleep | Deferred (`PAL-025`) | A later PAL must return `NonZero(1)` from `available_parallelism` and issue exactly one admitted `ViSyscall::Yield` from `yield_now`; the pinned unsupported PAL's `UNKNOWN_THREAD_COUNT` and no-op yield are prohibited. Thread creation remains Unsupported, and sleep follows the separately approved fail-closed behavior. |
| Monotonic time | Deferred (`PAL-027`) | Uses only admitted `GetTime` with declared frequency; values are nondecreasing and checked. Untrusted/unavailable timer fails, not zero. |
| Wall time | Unsupported (`PAL-028`) | No trusted clock; never substitute monotonic ticks or UNIX epoch as current time. |
| Entropy | Deferred; current backend non-qualifying (`PAL-019`, dependent on `PAL-031`) | Current default kernel features include `dev-weak-rng`, the VirtIO RNG stub returns zero, and `GetRandom` then reports xorshift bytes as success. Future qualification requires a production tuple without `dev-weak-rng`: real admitted entropy must fill the requested bytes, or unavailable/denied/short entropy must return zero/error without synthetic bytes, timer/address seeds, host RNG, or partial success. Evidence must bind the production feature tuple, source identity, device behavior, and direct-syscall results. |
| Raw I/O buffers / IPC | Deferred primitive boundary (`PAL-011`, `PAL-031`) | Typed frozen opcodes and allowlist gates exist, but `GetRandom` currently dereferences an unvalidated raw cell pointer. Future qualification requires checked length/overflow and complete caller-owned writable provenance before access, with hostile direct-syscall evidence for null, overflowed, oversized, unmapped, kernel, and peer-cell pointers; every rejection must occur without a read or write. Denial, invalid input, OOM, and service-down remain distinct; replies remain bounded with no cross-cell aliasing. |
| Math symbols / platform version | Math Deferred; empty platform-version surface Supported (`PAL-034`, `PAL-036`) | All pinned external math symbols come from exact compiler-builtins/private-sysroot linker inputs, never mlibc, POSIX libc, host libraries, or instrumentation. The pinned non-Apple `platform_version` module has no callable Cellos surface; any future API invalidates this status. |
| Backtrace | Unsupported (`PAL-029`) | Disabled/Unsupported with no frame or address disclosure. |

## Error Mapping

`Unsupported` is `std::io::ErrorKind::Unsupported` or the pinned equivalent result. `PermissionDenied` means an admission/capability gate denied the operation. `NotFound` means an authorized lookup found no resource, never a denial. `OutOfMemory` follows the allocator divergence rule. Infallible upstream signatures may abort only where this contract explicitly says fail closed; they cannot return fabricated entropy, time, handles, or bytes.

## Invalidation

Frozen ABI drift, loader/start/init-array ABI drift, allocator concurrency changes, any second task/thread, changed Yield or `available_parallelism` semantics, panic/personality strategy changes, compiler-builtins/cmath/env-constant changes, toolchain/source digest changes, changed service admission, any drift in the closed kernel security-backing inventory, reintroduction of `dev-weak-rng` into a production tuple, or a new success-shaped fallback invalidates this contract and blocks implementation. `PAL-019` and `PAL-031` cannot be promoted without the specified production entropy and hostile pointer-provenance evidence. Approvals are recorded only in [`../approvals/runtime-contract.md`](../approvals/runtime-contract.md).
