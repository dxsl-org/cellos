# AArch64 Semihosting and Ledger Summary

> Historical planning input. ADR-0013 supersedes the independent-runner,
> separate-steward, and ordered-sequence recommendations below; factual ledger
> and runner findings remain usable.

## Facts

- Test-hook-only ARM exit uses semihosting, and the dedicated runner boots `virt` with `-semihosting`, rejects nonzero/timeout/fault/missing-marker outcomes, and prints a final PASS only after seven markers (`kernel/src/main.rs:77-110`; `scripts/qemu-aarch64-test-hooks.sh:43-72,88-124`).
- The governed blocker is still `BLOCKED`, bound to `qemu-rv64`, and states the stale compile failure (`docs/app-tier-acceptance-ledger.json:6-15`).
- Ordinary baseline validation requires one adjacent lifecycle transition and preserves blocker `id`, `subject`, `scope`, and `evidence` (`scripts/app_tier_acceptance/ledger.py:159-211`). Fresh ARM evidence therefore cannot lawfully repair this record through the current path.
- Ledger production Phase 3 is unrelated production-admission work and must remain `PLANNED`; semihosting evidence cannot advance it.

## Reconciled Decision

First capture exactly two unique, tracked `.txt` artifacts: untouched QEMU stream and complete build/runner transcript, with command, revision, ELF/QEMU identity, UTC time, exit status, byte counts, and SHA-256. Require an actually independent runner and final PASS. Then stop until a separate steward/reviewer ratifies a general append-only, non-lifecycle correction/resolution event mechanism. That mechanism must bind trusted-parent and before/after section digests, preserve the historical event chain, keep correction separate from evidence resolution, and fail closed on replay or scope drift.

Never hand-edit the blocker, redirect it to ARM in an ordinary lifecycle event, resolve it against RV64, force-add ignored `.log` files, synthesize PASS from markers, or weaken immutability. Passing QEMU evidence remains test-hook-only and proves no production, admission, physical, Tier-2, or C9 qualification.

## Evidence Gate

`bash scripts/build-aarch64-test-hooks-ci.sh` followed by `bash scripts/qemu-aarch64-test-hooks.sh` must both exit zero, with final PASS and raw stream preserved. Missing independence/raw/TTL/digest/marker/zero exit or governance ratification halts the entire ordered sequence. After lawful resolution, current verification/changelog binds the exact tested revision/tree, commands, and artifact hashes; only then may Phase 02 start.
