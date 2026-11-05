# Intel iGPU compute — CP1 confirmation 1, lock handoff and G3 decisions approval

Recorded 2026-11-04. The accountable maintainer/owner (the user) answered **"duyệt theo khuyến nghị"** ("approve per the recommendations") to this question:

> - CP1 D-G1: buffer ownership B (client-owned exports);
> - D-G0: Tier-2 large memory as a separate CP1;
> - D-G2 … D-G13 per §10 of `cp1-gpu-kernel-abi.md`;
> - Q1: Mesa carries the small OS-identity patch (Managarm precedent);
> - Q3/Q4/Q5: inference Cell linking `anv`, a C++ runtime for Cellos, hard-float C, each in its own plan.

## Recorded values

| ID | Decision |
|---|---|
| D-G0 | Large anonymous memory for x86 Tier-2 is a separate CP1. Draft: [cp1-tier2-large-memory.md](cp1-tier2-large-memory.md), not approved |
| D-G1 | **B**: the client owns buffer memory and exports it to the driver's VT-d domain |
| D-G2 | One shared holder VT-d domain per iGPU; per-client PPGTT isolates clients (no PASID) |
| D-G3 | Kernel-allocated IOVA window per export, region `[2^38, 2^39)` |
| D-G4 | Holder named by service id in the kernel table `DMA_EXPORT_SERVICES`; the provider needs `PcieDriverCap` |
| D-G5 | Revocation is kernel-only, with drain; unacknowledged invalidation quarantines frames; the kernel never waits on the driver |
| D-G6 | Completions reuse `IpcSubmit`/`IpcReply` + `WaitCompletion(IPC)`; no new completion source |
| D-G7 | `XE_GPU` is added to `OWNER_WATCH_SERVICES` |
| D-G8 | `service::XE_GPU = 21`, init-registered |
| D-G9 | 437 and 439 have no allowlist bit; 438 shares bit 48 with `GrantDma` |
| D-G10 | Device-read-only exports (`R`-only second-level leaves) |
| D-G11 | Breadth phase 12: the graphics DRHD translates when the GPU driver Cell is present (Key Insights amended 2026-11-04) |
| D-G12 | `MAP_USERPTR` uses the same export; no extra kernel delta |
| D-G13 | Wire per §5: `ExportDmaRange = 437`, `MapDmaExport = 438` → `ViDmaExportV1` (48 bytes), `RevokeDmaExport = 439`, sentinels per E21 |
| Q1 | Mesa may carry the OS-identity patch (`DETECT_OS_CELLOS`, two meson system lists, static ICD target) |
| Q3 | The G3 deliverable is a Tier-2 inference Cell that links `anv` |
| Q4/Q5 | C++ runtime, hard-float C ABI, XSAVE, mlibc `posix` and libdrm-lite are owned by the separate plan [`261104-0802-x86-tier2-gpu-runtime`](../261104-0802-x86-tier2-gpu-runtime/plan.md) (registered, not admitted; its own decisions D-R2…D-R10 are pending) |

## What this authorizes

- Law-1 confirmation 1 for the 437/438/439 designs, `ViDmaExportV1` and `service::XE_GPU = 21`.
- The file-bounded exception for exactly CP1 §8. It is **effective only** when all of these hold:
  1. the owner gives this slice the Intel WIP lane;
  2. x86 breadth S1 no longer holds the shared kernel and ABI files;
  3. breadth phases 04a, 04b and 12 have landed.

## What this does not authorize

- Law-1 confirmation 2 (required after implementation).
- The D-G0 large-memory ABI, or any decision in the runtime plan.
- Admission of an x86 Tier-2 inference Cell on any image.
- Hardware purchase, or physical, remote or production claims.
- Release of any CELL-CONTRACT-0024 whole-plan relation.

Per ADR-0013, the integrating session ratifies nothing itself. This record carries the owner's answer only.

**Source HEAD at binding:** `fb79d0faec23a3ec31bfe373e341e57d2887393c`. Other sessions have uncommitted edits in the working tree; the digests below are of the files as bound.

**Digest form:** SHA-256 of whole files including the final newline, with no normalization. No digest is stored inside the file it hashes.

## Bound documents

| Path | SHA-256 |
|---|---|
| `.agents/261104-0801-x86-igpu-compute/cp1-gpu-kernel-abi.md` | `b4e33b988494fff2d50b86263a153af9eb41b0593a000d0b62128eaf475bc31d` |
| `.agents/261104-0801-x86-igpu-compute/xe-uapi-subset.md` | `233c13646696d13334cbc1e91a4744bda8c0f3a414aa5affb1a500da13cdda81` |
| `docs/decisions/0033-x86-intel-igpu-native-compute.md` | `db215d28277f6bd99da272ba2e77a3a7272339c57c18b7141e93c167df408eaa` |
| `docs/decisions/0022-intel-x86-64-c2c-only-direction.md` | `9ecda873f9347de6b8e97df83cb7c6f93d5924fa9ac1e2997b399dd62a0e7fe0` |
| `docs/abi-boundary-records.md` (with §7) | `d771ce856d3db03f975c5769c196d0d639bacacb8dc6fa0fe86dd09aa484783c` |
| `.agents/cell-contract-0024-locks.md` (with the pending-handoff entry) | `2e09d7ea10b859b94fabaf644a1116962d26413316aa23fff97ed2292971a591` |
| `docs/code-standards.md` | `3be252a9ed9e9efb3075c48a0f1a2bf19bf426431306dd5b075bdfd302f9106b` |
