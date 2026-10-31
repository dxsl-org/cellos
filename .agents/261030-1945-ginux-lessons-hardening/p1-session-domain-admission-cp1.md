# CP1 — P1: session publication admits Tier‑2 domain children

Status: PROPOSED, awaiting user decision. No source edited.
Origin: RV64 shell launch fix (`110f5bf55`). The user chose "P2 now, P1 later" on 2026-10-31.

## Problem

The cellbox shell launches every external program through the Cell-session
path: `PrepareChild` (`kernel/src/task/cell_session.rs:1681`), which calls
`SessionPublication::reserve` (`kernel/src/task/launch.rs:378`), then
`CommitChild` (`cell_session.rs:1216`), which calls `SessionPublication::publish`
(`launch.rs:444`). `reserve` refuses `state.is_domain`
(`launch.rs:389-391`, "a session child is … never … domain").

Every signed FFI/UNTRUSTED cell is therefore unlaunchable from the shell
(`Denied`): doom, tetris-c, tetris-lua, tier2-smoke, tier2-exploit,
mlibc-smoke, pipe-test, and posix-shim-test before P2 removed its FFI tier.
It also blocks `tier2_fault_isolation.rs` and
`tier2_posix_ffi_execution_runs_in_paged_domain`.

The only domain admission today is in `publish_prepared` (`launch.rs:111-257`).
`domain_admission.rs:234-235` states it is the only caller: "a second
admission path would defeat ADR-0019 §2.1".

## Proposal

Do not create a second, independent admission policy. Make the session path run
**the same ADR-0019 sequence** as `publish_prepared`, split across prepare and
commit:

| Step | `publish_prepared` today | Session path (new) |
|---|---|---|
| a. Live-grant preflight (`refuse_while_domain_grant_live`, grant tables before SCHEDULER) | before `SCHEDULER.lock()` | in `prepare_child`, before the lock at `cell_session.rs:1710`; **again** at commit, before the SCHEDULER lock that `CommitChild` (op 13) is dispatched under |
| b. `admit_for_launch(path, granted)` → lease | under SCHEDULER, after the task exists | in `reserve`, after `into_task`; the lease is stored in `SessionPublication` |
| c. `create_cell_domain(ks, us, seg)`, `bind_address_space` | same place | in `reserve` (allocating, and fallible, as everything in `reserve` is); a failure drops every reservation as today |
| d. `lease.remains_enabled()` immediately before runnable | before `bind_address_space` | in `SessionPublication::current()`, which `commit_child` checks at `:1248` under SCHEDULER, in the same critical section as `publish` |
| e. Audit / log `[domain] admitted cell … to Tier 2 Paged Domain` | yes | yes, the same line, so existing witnesses match |

The refusal at `launch.rs:389` keeps every other term: `requested_cell_id`,
`platform`, `replacement`, `block_io`, `input`, `development_silo`. Only
`is_domain` changes from "refuse" to "admit through the steps above".

Builds without `native-domains`, and unsupported architectures, keep refusing
with `NotSupported`, as `publish_prepared` does.

**Drain window.** A drain (`begin_domain_drain`) between prepare and commit bumps
the generation. Step d sees it and the commit fails with `Revoked`; the
staged child is aborted through the existing `abort_prepared`, and its domain
is dropped with the reservation. No SAS fallback.

**Live grant created between prepare and commit.** The commit-time repeat of
step a refuses it. Because op 13 runs under SCHEDULER, the preflight must move
to the syscall entry for op 13 (before the lock), and its result is carried into
`commit_child`.

**Session stdio from a private root.** The child's stdio/cwd endpoints are
kernel objects (`pipe::*`, VFS receipts). They are not user memory mapped into
the domain, so no new mapping is needed. To be proven by the runtime witness,
not assumed.

## Decision needed (Law‑1 / ADR)

ADR-0019 §2.1 says domain admission has a single on-path control. Two options:

- **A (recommended): amend §2.1.** It becomes "one admission *policy*
  (`domain_admission`), applied at every publication point; today
  `publish_prepared` and `SessionPublication`". The `domain_admission.rs:234`
  doc and the lock register's ADR row are updated.
- **B: route session launches through `publish_prepared`.** This loses the
  session's reserve/commit atomicity (stdio receipts and context are committed
  separately), so it is a larger cell_session rework. Not recommended.

## Files (option A)

- `kernel/src/task/launch.rs`: `SessionPublication` gains
  `domain: Option<(DomainAdmissionLease, AddressSpace)>`; `reserve`, `current`
  and `publish` change.
- `kernel/src/task/cell_session.rs`: preflight in `prepare_child` and at op 13
  before the lock.
- `kernel/src/loader/domain_admission.rs`: doc of the caller set only.
- `docs/decisions/0019-*.md`: §2.1 amendment.
- `cells/tests/posix-shim-test/src/main.rs`: restore
  `tier = PROTECTION_CLASS_FFI`.
- `docs/posix-ledger.md` preamble.
- `CHANGELOG.md`.

## Evidence plan

- **Kernel host tests (new):**
  - a domain session child is admitted with a lease;
  - a drain between reserve and commit refuses the commit (`Revoked`) and leaves
    no task;
  - a live domain grant refuses at prepare and at commit;
  - the non-domain refusals stay as they are.
- **RV64 runtime:**
  - `posix_shim_fstat`, `posix_shim_getentropy` and `posix_shim_net` pass with
    the FFI tier restored, and the log shows
    `[domain] admitted cell 'posix-shim-test' to Tier 2 Paged Domain`;
  - `tier2_fault_isolation` and `tier2_posix_ffi_execution_runs_in_paged_domain`
    are re-run, with their pre-change result recorded first;
  - the full RV64 boot suite shows no regression against 51/65.
- **Regression:** the x86 candidate `ahci-x86`, `nvme-x86` and `x86_64-boot`;
  RV64 `atomic-publication` and `authority-provenance`.

## Risk

Medium to high:
- lock order: grant tables before SCHEDULER, with two preflights;
- domain memory charged at prepare and held until commit or abort (bounded by
  the existing 16-per-operation and per-principal staging limits);
- the drain-lease window.

Rollback: revert the source and restore the `is_domain` refusal. No persistent
state is involved.
