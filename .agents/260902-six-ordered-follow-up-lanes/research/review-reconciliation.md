# Review Reconciliation

> Historical planning input. ADR-0013 supersedes the strict-serial and
> actor-separation conclusions below; ABI, backend, and evidence-binding
> conclusions remain usable.

## Accepted Corrections

- Replaced unsafe proposed bits 55–57 with provisional bit 60 for the CWD family, 61 for fstat, and 62 for rename; left 63 unused. Source confirms 55–59 are occupied and 60–63 are currently unassigned.
- Rejected ordinary ARM blocker redirection. Phase 01 now separates independent immutable evidence from a separately ratified append-only correction/resolution mechanism and leaves acceptance-ledger production Phase 3 untouched.
- Made writable backend, complete all-mutator authority, and canonical-path lease/reservation accounting preconditions to every rename/public-mount change. VIFS1 denial is a blocker witness, not completion.
- Made QEMU version checks literal equality in all three existing qualifying runners; no suffix, range, legacy package, or oracle relaxation.
- Expanded Phase 02 from path substitution to removal of live stale monolith/future-work wording where contradicted by implemented bounded entropy/network symbols, while retaining unsupported limits.

## Fstat Record Decision

The reviews proposed either a minimal 16-byte kind/size record or a 32-byte record with access and reserved fields. This plan selects one explicit 32-byte V1 contract: `kind:u32`, `access:u32`, `size:u64`, `reserved:[u64;2]`, `#[repr(C, align(8))]`. Access is not a duplicate POSIX mode: it truthfully distinguishes stdin read-only, stdout/stderr write-only, and current VIFS read-only objects. Reserved bytes are always zero, offsets and discriminants are frozen by tests, and C-layout translation stays outside the kernel wire.

## Rename Contract Decision

If gates qualify, v1 unequal rename uses one backend `rename_once`; equal existing regular source succeeds without it even if open. Ordinary `OpenCap` becomes existing-only `CapPerms::FILE_READ`, preserving shell/BootFs/hypervisor reads while removing ambient create/write; WriteCap/Truncate also require `CapPerms::WRITE`. A short-held canonical-path ledger issues shared read/transient leases and exclusive namespace reservations; only a separately confirmed create-open may atomically downgrade exclusive→shared before publication. `ParkedCapFile` keeps its operation lease across revoke; extract-unlock-drop cleanup and paused races prove no gap/nesting.

## Strict Serial and Evidence Binding

There is no blocker bypass. Any failed gate/test leaves that phase and every successor pending until the same phase succeeds. Each code/doc candidate is committed first and verified from a clean checkout of that exact commit/tree; only afterward may its normal verification report/current changelog bind revision/tree, literal commands/results, and evidence paths/SHA-256/sizes.
