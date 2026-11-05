# x86 Tier-2 GPU runtime — owner decisions D-R1…D-R10

Recorded 2026-11-04. The owner (the user) answered **"duyệt"** ("approve") to the D-R1…D-R10 recommendations in [plan.md](plan.md).

| ID | Approved value |
|---|---|
| D-R1 | Plan registered separately; slices admitted one by one |
| D-R2 | R1–R4 and R6 run beside the Intel WIP slice, each after a lock-inventory check and its handoff request; R5 needs the lane |
| D-R3 | H1: Tier-2 C/C++ compiled hard-float SSE2; Rust stays soft-float; no float or vector types across Rust↔C |
| D-R4 | Hosted C++ profile `cpp-hosted` (LLVM libc++/libc++abi/libunwind) by a new ADR amending ADR-0018 §2.3; libstdc++ rejected |
| D-R5 | XSAVE v1: XCR0 = x87, SSE, AVX; AVX-512, AMX and PKRU stay out |
| D-R6 | XSAVE goes through a CP1/CP2-style owner check, although it is outside Law 1 |
| D-R7 | XSAVE is off the G3 critical path and required before the G4 Linux comparison |
| D-R8 | Pin mlibc, port `sysdeps/vicell` to upstream's `Sysdeps<Tag>` API, enable `posix` only |
| D-R9 | libdrm-lite: pinned upstream libdrm subset (MIT), only device enumeration reimplemented; identity from the `xe` `CONFIG` query plus a launch-config BDF |
| D-R10 | Production admission of the inference Cell artifact stays a separate decision; QEMU witnesses use the development profile |

**Authorizes:** starting R0's documents and the user-space slices R1–R4 and R6 under D-R2.

**Does not authorize:**
- any kernel, HAL or `libs/api` edit (R5 and the large-memory implementation wait for the lane);
- admission of the inference Cell;
- the ADR text itself, which is written under D-R4 and recorded as accepted on that basis;
- the XSAVE CP1, which is still drafted for its own owner check (D-R6).

| Path | SHA-256 at binding (HEAD `fb79d0fa`) |
|---|---|
| `.agents/261104-0802-x86-tier2-gpu-runtime/plan.md` | `3089cec8c24070cd38f2b1d938bdc47fcf07f0990808970126523b0becca14ca` |
