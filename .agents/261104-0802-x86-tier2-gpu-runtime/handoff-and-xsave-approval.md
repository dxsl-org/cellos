# x86 Tier-2 runtime — slice handoffs (H-1…H-6) and XSAVE design check (X-1…X-7)

Recorded 2026-11-04. The owner (the user) answered **"duyệt"** ("approve") to this table:

| ID | Approved value |
|---|---|
| H-1 | Witness launch rows in `kernel/src/loader/launch_profile/{profiles,tests}.rs` (S1-held): the S1 integrating session adds append-only rows with an empty capability ceiling. Requested in the lock inventory. Witnesses stay blocked until those rows exist |
| H-2 | `Cargo.toml`/`Cargo.lock`: append-only member lines, landed by the integrating owner (Main) |
| H-3 | Changelog text is handed to Main; slices do not edit `CHANGELOG.md` (S1-held) |
| H-4 | Witness runners are new standalone scripts only; S1's harness, x86 QEMU scripts and CI job are not edited; no CI job in these slices |
| H-5 | Shared files across slices (`third_party/mlibc/sysdeps/vicell/**`, `docs/posix-ledger.md`, `docs/guides/tier1b-c-zig.md`) in strict order R2 → R3 → {R4, R6}, one slice open on them at a time |
| H-6 | `-Wl,--build-id=sha1` from each mlibc Cell's own `build.rs`; `libs/cell-build` leaves R3's file set |
| X-1 | Plain XSAVE/XRSTOR in v1; XSAVEOPT later as a measured kernel-internal change |
| X-2 | One machine-wide XCR0; an AP with different CPUID stays offline |
| X-3 | An XRSTOR fault is a fatal kernel fault with diagnostics; no fixup |
| X-4 | Intercept and emulate guest XSETBV on SVM, and swap XCR0 around VMRUN; VMX inherits the rule at P09 |
| X-5 | Keep the FXSAVE path behind the permanent boot flag `fpu=fxsave` |
| X-6 | R5 handoff = the package's §7 file set, effective with the Intel lane and S1's release of `hal/arch/x86/**` |
| X-7 | CP2 after implementation, with the exact delta, evidence and an `abi-boundary-records` row |
| T-1 | Open separately: `cmake/cellos-aarch64.cmake` falls back to riscv64 at build time when `CELLOS_C_TARGET` is unset |
| T-2 | Open separately: `ocel-quickjs` on x86 passes `double` from SSE2 C to soft-float Rust `libs/api` math |

**Effective now:** the R1, R2, R3, R4 and R6 handoffs, for exactly the file sets in [handoff-requests.md](handoff-requests.md) §3, beside the Intel WIP slice (D-R2).

**Not effective yet:**
- R5 (needs the lane);
- any witness launch row (H-1, S1's action);
- kernel ABI 437–440 (needs the lane).

| Path | SHA-256 at binding (HEAD `fb79d0fa`) |
|---|---|
| `.agents/261104-0802-x86-tier2-gpu-runtime/handoff-requests.md` | `213aa974f33b4a43745c957412b6232532c62cb1cf77dbbb5a054d8553c80c8e` |
| `.agents/261104-0802-x86-tier2-gpu-runtime/cp1-x86-xsave-user-state.md` | `665de0047730af724cea25298b14873a5a80afb22a85ef17b7f840ee97f80d91` |
