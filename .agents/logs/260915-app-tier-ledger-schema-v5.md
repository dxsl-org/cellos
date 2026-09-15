# 2026-09-15 — App-tier ledger schema v5: source evidence binds archived revisions

## Why

The ledger's validation job was red on `main` for two independent reasons:

1. **A lapsed TTL.** `B-AARCH64-SEMHOSTING` was resolved on 2026-09-03 with a 2-day TTL, so its
   evidence expired on 2026-09-05. The validator rejects expired resolutions, and it also validates
   the *trusted baseline* at its own tip time, so once a snapshot carries expired evidence no later
   push can satisfy the append-only delta rules. CI has been red on every push since.
2. **A contract amendment.** Spec 23 section 5.1 makes the contract amendable by design, but schema
   v4 pinned the *live* `docs/specs/23-native-sdk-contract.md` as `source` evidence in 18 places
   (1 binding, 4 blockers, 12 security negatives, 1 seed event). The C2-MID witness/gap prose
   amendment at `81dbb81c` therefore invalidated all of them at once, and no schema v4 event kind
   could repair the seed event's pin: a migration may not touch history (the v3→v4 migration kept all
   four prior events byte for byte).

## What landed

- **Schema v5** resolves `kind: "source"` evidence against the content-addressed revision named by
  its digest — `docs/evidence/spec23-native-sdk-contract-<sha12>.md` for the contract,
  `docs/evidence/source/<path>` for every other source file — never against the working tree.
  Archival helpers live in `source.py` (`snapshot_path`, `matrix_at`, `matrix_digest_at`) and
  `checks.preserve`.
- A **v4→v5 migration** re-based `source_binding` onto the amended revision and is the only event
  kind allowed to do so; it must bind a revision that is itself archived. Claims now bind the
  revision their evidence was produced against while their ratified *matrix* digest must still match
  the live contract, so an amendment cannot quietly change a ratified availability value. Cohorts
  anchor their source witness to the same archived revision.
- Both revisions are archived: `…-027e1a2bbfb1.md` (ratified, `798e8b04`) and `…-99146be984d6.md`
  (amended, `81dbb81c`; C2-MID prose only, matrix digest unchanged).
- 13 new adversarial tests (`test_schema_v5.py`) cover the migration, the archival requirement, and
  the CI baseline scenario (a v4 snapshot validated in a ratified-revision root). Full suite: 81 tests.
- The resolution was refreshed with a real clean-tree run (`build-aarch64-test-hooks-ci.sh` +
  `qemu-aarch64-test-hooks.sh`, RC 0, semihosting self-tests PASS, vfs-test 96 PASS / 0 FAIL) against
  the standing Issue #47 decision, with the TTL raised from 2 days to 30 days so the gate is not red
  by construction. Evidence: `aarch64-semihosting-20260915-04-{raw,runner}.txt`.
- Repairing that run required fixing `build-aarch64-test-hooks-ci.sh`: it counted `attr=20` entries
  from the `/bin` marker to the end of the inspection dump, so `/etc/hostname` (added by `7d27af0a`)
  counted as a tenth `/bin` cell and every aarch64 test-hooks build failed its own assertion.
- Phase 04 (`PLANNED → IMPLEMENTED`) is recorded with the artifact it owns
  (`python3 scripts/validate-admission-prequalification.py` against the pinned 18-row catalog).
  Nothing is promoted: `c9` stays `NOT_COMPLETE` and all three blockers stay `BLOCKED`.

## Commits

`994c0b01` v5 schema + migration · `094d1874` test-hooks count fix · `d7006212` refreshed embedded
init · `f2f7d2a6` fresh resolution · `a312ff27` Phase 04 transition.

## Lessons

- Evidence that points at a *mutable file* is not evidence: pin the revision, archive it, and let the
  TTL own freshness.
- A fail-closed validator plus a short TTL means an unattended lane goes **un-updatable**, not just
  red: the baseline check runs before the delta check, so a lapsed snapshot blocks every later push.
  Refresh before expiry or accept a red intermediate push.
- Schema migrations may append but never rewrite; verify that assumption against the previous
  migration before designing one.

## Next

- Watch the CI `Lint` job on `a312ff27`: its baseline (`f2f7d2a6`) is the first valid snapshot since
  2026-09-05, so the append-only chain should be green again. The other four red jobs (C2C broker
  oracle, RedoxFS `/srv`, riscv64 network data-path, x86_64 hypervisor boot) fail identically on
  `0dc8bd9a`, before this work.
- Re-verify the semihosting resolution before 2026-10-15 or extend the TTL deliberately.
