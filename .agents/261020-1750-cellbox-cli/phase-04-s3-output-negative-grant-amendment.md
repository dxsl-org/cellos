# S3-output CP1 amendment — private stock negative-grant launch

Status: **APPROVED by the user on 2026-10-22 (“Duyệt row fixture echo”); implemented and runtime-proven on the final private S3 image.** Parent: [S3-output](phase-04-s3-output-proposal.md). [Saved evidence](reports/s3-output-runtime-evidence.md); no CP2, production edge or rollout approval.

## Missing prerequisite at amendment approval (historical)

AC-5 requires a real stock command with missing/invalid committed output grants to fail without UART payload leakage. Current driver can launch only cellbox. Cellbox validates its complete session binding before launching tools, and its PipeChild path always supplies stdout/stderr. Thus ordinary GUI-shell checks cannot deliver that negative context to the stock helper. Pure helper unit tests cover missing grants but are not the required runtime proof.

## Proposed narrow admission

Under existing non-default `phase04-driver-profile` only, permit `phase04-terminal-driver -> /bin/echo` on Path route with an empty child capability ceiling. No Elf route, ambient launch privilege, ordinary-image row, new syscall/selector/capability or production change.

Extend source closure only to `kernel/src/loader/launch_profile/profiles.rs` and its existing `tests.rs`. Driver routing already reaches its feature-specific profile in mod.rs; modify that dispatch only if inspection proves required, and record it before editing. The target `/bin/echo` already has reviewed empty-ceiling admission. Existing driver SpawnFromPath declaration suffices.

Use existing `Pipeline`/prepared-child context APIs in `cells/tests/phase04-terminal-driver/src/main.rs` to launch the actual signed stock echo without one/both output grants and with invalid output references. Missing grants must reach the command and fail nonzero; invalid references may be rejected before publication by canonical admission, in which case record that distinct outcome rather than claim helper execution. Where owner-valid delegated endpoints can be revoked before write without killing the child, exercise that separately. Never add a public helper constructor or bypass context validation solely for tests.

Add a direct ordinary no-context stock launch only if the existing driver route supports it; otherwise keep the already planned legacy init/free observation. Runner must require no distinctive negative-case payload on UART or unrelated session streams, independently query child termination via existing child-status APIs, and close/drain only owned operations.

## Unchanged limits

All parent ACs remain binding. The GetTime amendment remains free-only. This amendment is fixture admission, not Phase04 CP2, release authority, GUI pixel acceptance, canonical signing/image writes or quarantine removal. Roll back this feature-gated row and its fixture changes together; no persisted state is involved.

## Observed final outcomes (2026-10-22)

The non-default fixture Path edge has an empty child capability ceiling. `profiles.rs` and its adjacent row tests changed; dispatch in `mod.rs`, target ceilings and the CellSessionV1 ABI did not. The [authority baseline](reports/s3-output-authority-baseline.json) explicitly distinguishes the preexisting authorized source from the approved private echo row; it is not a clean-HEAD comparison.

| Case | Publication/helper execution | Independent outcome |
|---|---|---|
| Missing stdout | Published; real signed stock echo executes | `QueryChild` exit 1; stdout/stderr captures empty |
| Missing stderr | Published; real signed stock echo executes | `QueryChild` exit 1; stdout/stderr captures empty |
| Missing both | Published; real signed stock echo executes | `QueryChild` exit 1; stdout/stderr captures empty |
| Invalid stdout reference | Canonical admission returns `Stale` **before publication/helper execution** | Both captures empty; admission rejection is **not** helper execution proof |

All four cases also pass unrelated-host isolation and independent no-negative-payload UART checks. Actual markers and empty byte captures are preserved in [the sanitized archive](reports/s3-output-evidence/serial.log) alongside the full [308-check verdict](reports/s3-output-evidence/verdict.json). No extra public constructor, validation bypass, ordinary-image edge or capability was introduced. Valid-endpoint revocation is covered by the stock full-ring close/cancel scenarios; no separate pre-write revoked-negative echo execution is claimed.

Legacy/no-context behavior is observed through free on the existing init edge, not an extra no-context echo route. Final signed echo SHA-256: `ccbb3f9589d8d2bf79832ace7e465eca240b2d7bddc23b0b4bcbd3f3af54f72c`.
