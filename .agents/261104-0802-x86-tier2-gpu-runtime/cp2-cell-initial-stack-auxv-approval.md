# Cell initial stack + auxv — CP2 approved; slice RE closed

Recorded 2026-11-04. The owner (the user) answered **"duyệt"** ("approve") to [cp2-cell-initial-stack-auxv.md](cp2-cell-initial-stack-auxv.md) §5: the delta in §1 with deviations §2.1 (mlibc pin patch 0001), §2.2 (AX-6 weak default + anchor) and §2.3 (16 MiB arena, interim until R3b).

**Effect:**
- "Cell initial stack v1" is frozen as implemented: layout pinned by `libs/cell-initial-stack` and its host tests; a change needs a new CP. `docs/abi-boundary-records.md` §8 records it.
- Slice RE is closed and **releases the Intel WIP lane**. The next lane holder is the owner's call.
- Open findings carried out of the slice (not approved as done): threaded runs leak ≈ 56 KiB each (R3 100-run criterion), RV64/aarch64 mlibc runtime, KVM `-cpu host` boot exit.

| Path | SHA-256 at approval (HEAD `28dee8b3`, uncommitted tree) |
|---|---|
| `.agents/261104-0802-x86-tier2-gpu-runtime/cp2-cell-initial-stack-auxv.md` | `edb60bcdcbd153af10597cb81d6f14769bf214d370a569f1a15e369d8f3f4576` |
| `libs/cell-initial-stack/src/lib.rs` | `bc3204674c59f94deb1815296b52a58cba8e2a515e33a83ee7981450e5680e71` |
| `kernel/src/task/elf_prepare.rs` | `5c58f0afefb5a3e052e35fe72d5542879671fc7ced56c00aae68d13de27ff790` |
| `libs/ostd/src/startup.rs` | `521fe99d1e448be1e1ad281610fa61a1640422e9def53fd9d37d53fd5051a09a` |
| `libs/ostd/src/startup/libc_hook.rs` | `ee286c71b350e510e53856a7db265c1dbeef943e4d23f48bdce20c855b111f97` |
