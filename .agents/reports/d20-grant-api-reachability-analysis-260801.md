# D20 — Grant API reachability and the obsolete `sys_grant` stub

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

Grants are reachable from cells today. The docket looked at the wrong wrapper:

- Legacy `ostd::sys_grant` is an uncalled stub (`libs/ostd/src/syscall.rs:962-965`).
- The active ABI is `GrantAlloc/Share/Slice/Free` 208-211 and
  `GrantRegister/Unregister` 215-216 (`libs/api/src/abi/syscall.rs:218-257`).
- Kernel dispatch implements all six operations
  (`kernel/src/task/syscall.rs:3885-4080`).
- VirtIO block/GPU/net/input drivers allocate and free grant-backed DMA buffers; VFS and
  compositor use sharing/slicing paths.
- `git grep "sys_grant("` finds only the dead legacy definition.

Therefore Spec 12 §4.4's 2026-06-06 argument that grants cannot be created and their
tables are always empty is false. Current safety rests on the grant reaper, pin refusal,
quarantine, IOMMU teardown, and acknowledgement ordering — not impossibility.

## Recommended ruling [FINAL]

**Approve recommendation A: recognize the active Grant API and delete the legacy stub.**

1. Remove the unused `sys_grant` wrapper after a normal compile check; it is not the ABI.
2. Rewrite Spec 12 §4.4 around the implemented reaper/pin/quarantine invariants.
3. Keep `00-context` and system architecture's “Grant API implemented” statement, but link
   it to the 208-216 API and state current limits: one grantee, identity-mapped SAS, and
   no per-cell PTE enforcement in Tier 1.
4. Add runtime tests for owner death, grantee death, free-while-pinned refusal,
   quarantine acknowledgement, and repeated allocation/reap before claiming leak-free.

This ruling authorizes documentation and dead-wrapper cleanup only, not an ABI change.
