# CP2 — shared service runtime and wire modules (Phase 12)

**Date:** 2026-11-13 · **CP1:** [cp1-service-wire.md](cp1-service-wire.md) (owner confirmation 1, 2026-11-13) · **Law 1:** confirmation 2 given 2026-11-13 · **Branch:** `p12-service-runtime` (9 commits, rebased on `17bd2db19`)

## 1. Delta as implemented

| # | CP1 item | As implemented | Deviation |
|---|---|---|---|
| W1 | Proto modules | `libs/api/src/services/proto/{app_envelope,supervisor,net,tls,nic,vfs,config,input,compositor}.rs`; `proto_tests.rs` pins every value to its old byte, and pins `NetRequest` below `tls::CLOSE`. Servers, ostd clients, sys-tools, the 11 NIC drivers, the kernel (`acpi/sci.rs`, `authority.rs`, `gpu-scanout.rs`, `ps2_keyboard.rs`, `console_drv.rs`, `thread_cap_selftest.rs`), Lua, httpd and the bench import them. `scripts/check-service-wire-literals.py` (CI, with `--self-test`) refuses the proto-owned names and three literal shapes. | `kms`, `ai`, `block`, `silo`, `vmm_session` keep their existing owners and are not re-exported through `proto` (no new dependency edges). `DESTROY_OK/DESTROY_REFUSED` added: `DESTROY_SURFACE` answers a status, not the boolean. `OP_SNAPSHOT`/`OP_STATUS` are not gated names (other wires own the same names). |
| W2 | `Recv` a3 timeout | Bits 8–31 = ticks (`recv_flags`, `recv_timeout_ticks`); a non-zero timeout decodes to `RecvTimeout { attest_caller, timeout_is_error: true }`. The trailer is written at every delivery, never on expiry; expiry returns the generic error sentinel. `ostd::syscall::sys_recv_attested_timeout`. Kernel test `recv_timeout_bits_select_a_timed_receive_that_keeps_attestation`. | Expiry is an error, not `Ok(0)` (CP1 wrote "as `RecvTimeout`"): `Ok(0)` is also the kernel's sender id. |
| W3 | VFS trust wire | `VfsRequest::{TrustLookup, TrustChallenge, TrustCommit}` are documented tombstones answered `Err(VfsError::Unsupported)`; fields kept so old frames decode. `VfsResponse::{Trust, TrustChallenged}` deleted. Test pins 0x28/0x29/0x2a, `MkdirAt` 0x2b, `SaveOutcome` last. | — |
| W4 | kms trust ops | `KmsOpcode` 25–29, `KmsErrorCode` 39–40, `KmsTrustQueryKind`, payloads in `libs/types/src/kms/payload/trust.rs`, byte pins. Staged like the SSH import (64-byte Begin, 96-byte chunks, Commit → 40-byte outcome). | — |
| W5 | Input trust selector | Selector 29 is admitted from the live kms provider; docs in `cell_session.rs`, `session.rs`. | — |

Not Law-1, in CP1 §3:
- **`ostd::service::serve`** (`service/serve.rs`, `service/owners.rs`): attested receive (trailer must name the sender), `WatchPolicy::{Off, OnRequest, Every}`, `on_request`, `on_unwatched`, `on_owner_death`, `on_unattributed`, `on_turn`, `on_idle → Wait::{Block, Ticks, Poll}`, `on_empty_poll`, `keepalive`. `service_entry!` and `service_syscall_set` deleted.
- **Services on serve:** config, kms, ai (now attested; sessions keyed to `(cell_id, generation)`), supervisor, compositor (creator tids per owner; idle 5 s resolution check), VFS (`Every`, fail-closed `Err(Denied)`), net (`NetState`, `Probe`, own owner table so admission precedes the watch).
- **VFS block path:** exact-operation calls bounded at `BLOCK_REPLY_TICKS = 3_500`; `Expired { dispatched }` → `Indeterminate`/`NotIssued`; stall latch until the route names another provider; heartbeat 6 000 ticks; `FsBackend` → `Result<_, FsError>` (wire bytes unchanged; a mid-file device error is `Io`).
- **Trust store in kms** at `/srv/cellos/kms/ssh_known_hosts`; kms ships in every non-board image (owner decision).

## 2. Proposed for confirmation 2

Confirm W1–W5 as implemented, with the W2 deviation (expiry is an error). Re-pins: `check-multi-source-wait-law1-digests.sh` (allowlist-export count in `runtime.rs` 4 → 3, `service_entry!` deleted); `scripts/cellos_sign/signing.py` (`X86_CANDIDATE_RECIPE_SHA256` and `service-kms` in the headless signed set). The syscall golden dump and generated headers did not change (doc-only change to the `Recv` row).

## 3. Proof obligations (CP1 §4)

| # | Obligation | Result |
|---|---|---|
| 1 | Byte tests | `api` `proto_tests` (10), `types` kms trust pins, VFS tombstone pin: pass. |
| 2 | Frozen block driver fixture | `a_frozen_block_driver_does_not_hang_vfs_x86`: **fails on `e676d0ca9`** with only the hook and test added (VFS stops answering; `vcat /tmp/alive.txt` → `cannot read`), **passes** on the branch (after step 4 and again after VFS moved to serve). |
| 3 | W2 | Kernel decode test; every service on serve receives attested with a timeout (config/kms/ai/supervisor/compositor/VFS lanes below). |
| 4 | ai attestation | ai on serve serves only `on_request` (attested); unattributed messages are dropped. |
| 5 | Trust | kms host tests (moved VFS suite + framing/admission/limits); phase 09 ssh lane: see §4. |
| 6 | Net bench | RV64 guest `wget` 96 KiB ×7 ×2 boots: median 1853 ms before, 1853 ms after. |
| 7 | Gates and lanes | see §4. |
| 8 | ABI artifacts | headers OK, golden unchanged, digests pass after the one listed re-pin. |

## 4. Evidence

All on `75d015d22` (rebased on `17bd2db19`), 2026-11-13, one host, TCG unless noted:

| Check | Result |
|---|---|
| Host tests | kernel, api, types, ostd, service-vfs/net/hypervisor/config/compositor/kms/input, supervisor, cellbox, init; net-tools, httpd, silo, dwc2-usb, net-broker, lua, kms `ssh-dev-custody`: all pass (41 suites) |
| Clippy | x86_64 workspace (compile-only + test-hooks) and riscv64 workspace: no errors |
| Gates | 3 Law-1 digest scripts, service-wire literal gate (+ self-test), syscall headers, catalog, kernel residents, host-test crates, assembler/signing tests (108), `syscall_golden` |
| x86 headless ISO boot (`qemu-x86_64-test.sh`) | PASS |
| `power-x86` (shutdown, reboot, power button) | 3/3 |
| `nic-x86` (Intel NICs, DHCP) | 6/6 |
| Tier 3 display (KVM) | 41 checks PASS |
| `local-service-lifecycle-x86` (incl. non-receiving client) | 4/4 |
| `nvme-x86` on the fault ISO: frozen driver, restart, FAT mount, boot | 4/4 |
| Phase 09 ssh lane `--no-faults` (trust store from kms, first trust, changed key) | 34 checks PASS |
| RV64 boot subset (shell, FAT, VFS, block access, network, compositor input, hot migration) | 16/16 |
| RV64 `vfs_lifetime_selftest_passes` (test-hooks) | PASS |
| Frozen-driver witness before the phase (`e676d0ca9` + hook + test) | FAILS as required (VFS stops answering) |
| Net bench (RV64 `wget` 96 KiB, 7 runs × 2 boots) | median 1853 ms before and after |


## 5. Review

Independent review (`haily-reviewer`, read-only, on the 7 step commits): PASS_WITH_RISK, 1 high, 3 medium, 7 low. Fixed in `fix(services): review findings…`:

| Severity | Finding | Disposition |
|---|---|---|
| High | Upgraded disks lose the old `/srv/.cellos-trust` store (no migration), so a changed host key reads `UNKNOWN` and can be pinned through first trust; the old directory also lost its deny rule. | Deny rules for the old directory restored. Migration not added (owner decision CP1 §8.4: provisioner reseeds); **raised again in §7**. |
| Medium | A full owner table refused a new generation of a watched Cell; VFS was newly capped at 256 owners. | Fixed: same-CellId entries count as reclaimable; VFS unbounded again. |
| Medium | The stall latch never cleared for a driver that was only slow (NVMe admin budget 60 s, host stalls). | Fixed: one probe per `STALL_RETRY_TICKS = 6_000`; a replacement still clears it at once. Host test extended. |
| Medium | kms (newly heartbeating) could exceed 60 s in one trust commit behind a slow VFS. | Fixed: kms re-arms before each store round trip. |
| Low | Challenge id spent only after input registration succeeded (carried over from VFS). | Fixed. |
| Low | VFS counted every turn, not empty polls, toward reconcile. | Fixed (`on_empty_poll`). |
| Low | The supervisor ignored the kernel's shutdown envelope (sender 0). | Fixed. Any attested sender's shutdown is still obeyed, as under `AppContext`. |
| Low | Literal gate misses byte-by-byte envelopes. | Indexed-assignment pattern added; decimal forms, `static` copies and non-Rust files remain uncovered. |
| Low | `recv`/`recv_timeout` fast path: `ipc_recv` returning 0 is both "parked" and "kernel message delivered"; `TryRecv` `Ok(0)` in poll mode. | Pre-existing in `recv`; not changed. |
| Low | kms staged-query table: 4 slots, 10 s, per-cell replacement; four busy network Cells can keep `cellbox ssh` at `Busy`. | Residual. |

## 6. Not done / residual

- Input, net-broker and silo keep hand-rolled loops (owner decision).
- CellosFS reports an indeterminate write as `Io` (needs `libs/cellos-fs`).
- The compositor lost its one-time "owner death watch unavailable" log (serve does not report a refused fail-open watch; it records creators only for watched owners).
- `supervisor --features hostile-backend-recovery` and `service-net --features qemu-dev-https` alone do not build on `main` either (pre-existing).
- `hypha-local-ai` fails on a launch-edge denial (`hypha-core` → `/bin/llm-gateway`); loader and catalog are unchanged here (pre-existing).
- `nvme_09c_nic_owner_survives_replacement_x86` failed once in the 12-test sequence (fault marker not seen in 20 s) and passed alone.
- Desktop ISO lane not run (`build/cellos-x86_64.iso` not built); Tier 3 display covers the compositor.

## 7. Owner decision — 2026-11-13

- **Confirmation 2 given:** W1–W5 as implemented, including the W2 deviation (expiry is an error, never `Ok(0)`).
- **Upgraded disks:** no migration (CP1 §8 stands). The provisioner seeds `/srv/cellos/kms/ssh_known_hosts`; the old `/srv/.cellos-trust` stays denied to every caller. Accepted residual risk: on a disk provisioned before Phase 12, every host reads `UNKNOWN` until it is trusted again.
- **Integration:** fast-forward `main` to the branch.
