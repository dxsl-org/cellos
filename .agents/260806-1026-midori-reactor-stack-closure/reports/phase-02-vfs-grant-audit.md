# Phase 02 VFS grant-copy audit

## Result

Two VFS service sites perform unsafe copies through caller-owned grants. Both
complete the copy before returning the IPC reply, and therefore still depend on
the caller remaining blocked for the whole `ipc_call`.

## Sites

- `cells/services/vfs/src/dispatch.rs:249` — `ReadGrant`: copies from the open
  file image into a kernel-validated caller grant, bounded by file availability,
  request size, grant length, and 4096 bytes. The response is built only after
  the copy.
- `cells/services/vfs/src/dispatch.rs:310` — `ReadFileGrant`: copies an owned
  file buffer into a kernel-validated caller grant, bounded by the requested
  maximum and registered grant length. Its safety comment explicitly requires
  the caller's `ipc_call` to remain blocked until reply.

`WriteGrant` is excluded: it fails closed before resolving or reading the grant.
The caller-side copy in `libs/ostd/src/fs.rs:403` occurs before sharing the grant
and is not a VFS write into foreign memory.

## Boundary for Phase 04+

Any cancellable or completion-driven VFS operation must pin or own these buffers,
or add a descriptor drain/cancel contract, before removing the blocking invariant.
