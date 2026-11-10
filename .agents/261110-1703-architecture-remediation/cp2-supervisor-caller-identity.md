# CP2 — supervisor authorises callers by measured image (Phase 06, CP1 option A)

**Date:** 2026-11-11 · **CP1:** [cp1-supervisor-caller-identity.md](cp1-supervisor-caller-identity.md) (option A approved 2026-11-11) · **Law 1:** confirmation 2 requested

## 1. Delta as implemented

No change to `CellImage` bytes, the version, the decoder, syscall number 441 or any allowlist bit, and no `libs/api` / `libs/types` edit.

- **Kernel, `kernel/src/task/cell-image.rs::resolve_for_caller`.**
  - The registered VFS Cell is answered as before, unfiltered.
  - A `SupervisorCap` holder is now answered too, but only when the subject's root has `Task::launch_named`, meaning it was launched from a kernel-resolved `/bin` install.
  - Every other caller gets `PermissionDenied`.
- **Narrower than the CP1 text.** The supervisor gets no answer for a sandbox copy or for caller-supplied ELF bytes, even when those bytes are the signed program. This is the same rule as the kernel's own 502 gate.
- **`docs/abi-boundary-records.md` §9, 441 row.** The "who may call" text is updated.
- **`libs/ostd/src/app.rs`.** The owner approved this extension to the file set on 2026-11-11.
  - Adds the opt-in `AppContext::attest_callers()` and `AppContext::caller()`.
  - Untimed receives then use `sys_recv_attested`. The trailer bytes are zeroed before each receive and cut from the payload of every event.
  - Cells that do not opt in behave exactly as before.
- **`libs/ostd/src/syscall.rs`.** Doc comment for `sys_resolve_cell_image` only.
- **`cells/services/supervisor`.**
  - It opts in at `Init`.
  - It accepts `OP_POWER` (operator), `OP_HOTSWAP` and snapshot requests only when `sender_identity::caller_runs_program` holds. That requires an attested trailer for this exact sender, and 441 must report the image as signed `shutdown` or `hotswap`.
  - The `GetProcs` name path is deleted.
  - The supervisor has no `__ViCell_syscalls` section, so no allowlist edit was needed.

## 2. Evidence

| Planned in CP1 §4 | Result |
|---|---|
| Kernel host test: 441 answers the supervisor and VFS and refuses others | `cell_image::tests::supervisor_is_answered_only_for_named_launches` plus the existing `only_the_registered_vfs_cell_is_answered`. Kernel suite: 610 passed |
| Supervisor host test: unsigned or other-named senders rejected, signed `shutdown` accepted | `sender_identity::tests::only_the_attested_signed_program_is_accepted`, covering an other-signed image, unsigned bytes, no 441 record, no trailer, and a trailer for another sender. Supervisor suite: 12 passed |
| `power-x86`: operator reboot/poweroff and power button | 3/3 PASS on an image assembled from this tree (`/tmp/s1-lanes/cp2`) |
| Negative QEMU control (`/home/x/shutdown`) | Not run. The kernel and supervisor unit tests cover the refusal; a QEMU fixture would need a sandbox copy step in the image recipe |

Also run on the fresh headless image: x86 boot to shell, `x86_64-boot` (10/10) and two-node C2C (TCG) all pass. RV64 and AArch64 boot lanes (cells built from source) pass. Their logs never mention the supervisor, so they show only that the shared ostd change does not break other cells.

**Surprise.** The first run of `power-x86` refused the operator with "refused; the supervisor resumed the system". The cause was `parse_power_request`, which requires zero bytes after the request; the trailer at the tail of the buffer broke that. The fix was to have the ostd payload exclude the kernel-owned tail. After the fix, the lane passes.

## 3. Rollback

Revert the kernel, ostd and supervisor hunks. Nothing persists.

## 4. Question to the owner

Confirm CP2 (Law-1 confirmation 2) for the 441 "who may call" widening as implemented in §1.
