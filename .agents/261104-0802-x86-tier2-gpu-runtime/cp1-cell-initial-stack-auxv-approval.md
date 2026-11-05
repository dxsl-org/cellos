# Cell initial stack + auxv — CP1 design check approved

Recorded 2026-11-04. The owner (the user) answered **"duyệt"** ("approve") to [cp1-cell-initial-stack-auxv.md](cp1-cell-initial-stack-auxv.md) §6: AX-1…AX-8 as recommended.

| ID | Approved value |
|---|---|
| AX-1 | Every Cell, on riscv64, aarch64 and x86_64, gets the new initial stack |
| AX-2 | Linux/SysV layout at the 16-byte-aligned entry `sp`: `argc`, `argv[]`, `NULL`, `envp[]`, `NULL`, auxv, `AT_NULL`, then the pointed-to data; entry registers unchanged |
| AX-3 | v1: `argc = 0`, empty environment; `SpawnRequest::argv` keeps its own path |
| AX-4 | `AT_PHDR` (stack copy), `AT_PHENT`, `AT_PHNUM`, `AT_PAGESZ` = 4096, `AT_ENTRY`, `AT_BASE` = Cell load base (Cellos meaning), `AT_RANDOM` only with an entropy source, `AT_NULL` |
| AX-5 | ≤ 16 program headers, ≤ 9 auxv pairs, 16 entropy bytes (≈ 1.1 KiB); more headers → spawn refused |
| AX-6 | ostd `_start` calls a weak `__cellos_libc_start(entry_sp)` instead of walking `.init_array`, then `main`, then `Exit(0)`; `mlibc-shim` references the hook strongly in the same change |
| AX-7 | Written into the Cell's own user stack before first entry; no mapping, no authority |
| AX-8 | Governed like a Law-1 contract: CP2 after implementation, ABI-boundary record row "Cell initial stack v1" |

**Effect:** the design is fixed. Implementation is a kernel + `libs/ostd` change (CP1 §5 file set) and needs the Intel WIP lane. At approval time S1 had closed and released the lane (HEAD `b3b5d51e`, "close slice S1"); which slice takes the lane next is a separate owner decision.

| Path | SHA-256 at approval |
|---|---|
| `.agents/261104-0802-x86-tier2-gpu-runtime/cp1-cell-initial-stack-auxv.md` | `8a8feebf6f372521c0fab277028541205b17eafc55fb23535868b08bc392062e` |
