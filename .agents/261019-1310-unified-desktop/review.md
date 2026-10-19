# Desktop plan review and validation

2026-10-19. Four internal read-only reviewers: security, assumption refutation, failure modes, dependency/UX coverage. These are automated review roles, not independent human approvals or ABI ratification. All read the draft overview, contracts, acceptance ledger, recon and phase files. No external model egress, implementation edits, builds or tests performed by reviewers.

## Adjudication

20 unique findings accepted: 17 Major, 3 Minor, no Critical. One duplicate Tier3->Files dependency finding merged. Corrections were applied by the integrating planner; this is a record of mitigation, not an independently re-reviewed implementation pass.

| ID | Finding | Severity | Resolution in revised plan |
|---|---|---|---|
| D1 | Files Grid consumes thumbnail code from its own dependent phase | Major | Minimal shared image/thumbnail extraction moved into P06; P07 depends on P06, P08 extends existing primitives. |
| D2 | Tier3 scenarios need Files/Open With before P10 | Major | P10 now depends on P07; index and phase DAG updated. |
| D3 | Files absent from native `+` consumer acceptance | Major | AC-05 owns P07; independent new browsing session and failure/cancel scenarios added. |
| D4 | Alt+Space intercepted without a consuming Desktop endpoint | Major | contracts.md defines authenticated OpenSpotlight activation IDs, idempotent open, consume-once and unavailable/restart/backpressure behavior; P04 consumes it. |
| D5 | Six-cell selector geometry unspecified, center cells inaccessible | Major | P05 specifies 264x176-DIP top-center overlay, 88-DIP cells, 8-DIP drag activation, scale/clamp/minimum-fit behavior and menu alternative. Prototype defaults remain subject to user-path validation. |
| D6 | User-pinned Places missing implementation/acceptance | Minor | P07 pin/unpin and explicit preference persistence semantics/scenario added. |
| S1 | Close/power consent not bound to exact inventory | Major | Prepare/commit bind immutable incarnation-qualified view set, inventory and content revisions; change invalidates consent at dispatch boundary. |
| S2 | Force-stop hides other same-process dirty views | Major | Confirmation enumerates full owner-incarnation blast radius across frames and expires on owner/set change. |
| S3 | Selected-file grant not atomic with launch recipient | Major | Single-use open intent binds resource identity/rights/AppId/recipient/LaunchRef; atomic consumption/revocation on cancellation, failure and death. |
| S4 | Read-only generic settings access leaks unrelated keys | Minor | P02 restricts backing config Get/List/subscription by principal/key; apps get narrow token snapshots. |
| S5 | Cancelled non-preemptible PDF job can hold sole slot forever | Major | P08 requires lifecycle watchdog, killable provider generation, confirmed death before grant retirement and queue progress; hung-worker scenario required. |
| S6 | Guest clipboard introduced without authority contract | Major | Remove host/guest clipboard bridge from scope; no ambient guest access. Local Terminal copy/paste remains in scope. |
| A1 | VMM run loop has no app-session request dispatcher | Major | P10 adds actual authenticated receiver/dispatch/replies for launch/resize/input/close/cancel/death plus bounded guest execution/control service interval. |
| A2 | App volume `/dev/vdb` expected but only one disk path assumed | Major | P10 specifies separate vda base/vdb read-only payload devices, MMIO/IRQ/queue/teardown mapping, no shell fallback, separate selected-file transfer channel. |
| A3 | External CLI children bypass shell stream facade | Major | P09 migrates process-default ostd::io and direct I/O consumers to inherited endpoints; actual external-child concurrency proof. |
| F1 | Individual resize ACKs can half-commit mixed-owner group | Major | Frame configure stages all required views under one membership/work-area revision; old frame stays active until complete commit, cancellation retires staging safely. |
| F2 | Copy/move can mix source revisions or destroy previous destination | Major | Stable source lease/revision, distinct staging, supported safe replace and source revalidation; unsupported primitives preserve originals and report explicit outcome. No new crash-atomicity promise. |
| F3 | Background/foreground shell children compete for stdin | Major | One stdin reader lease; prompt shell -> foreground child -> shell; background receives EOF unless redirected. |
| F4 | Guest close bypasses native prepare/cancel/commit | Major | P10 guest adapter uses same prepared inventory/revision contract; unsupported safe consent is explicit NeedsUserDecision/Unknown, not fabricated readiness. |
| F5 | No actual compositor restart proof | Minor | verification.md requires killing/restarting compositor, rejecting old references/ACKs/grants, recovering live content; repeat with actual Ocel/Terminal after consumer migration. |

## Source verification

Parent directly read/grepped: VFS manager 480-byte listing cap and limited Stat; config in-memory Get/Set/List; ViTheme and Ocel private palettes; vm-runner logs then exits; VMM Background scanout; compositor bare TID ownership; guest init vdb mount and shell fallback; ostd Stdin input/UART and Stdout sys_log. Those corroborate the major prerequisites in recon. Source existence is not runtime or admission proof.

## Remaining exact implementation gates

- P01 concrete public-wire/SDK deltas, owner handoff and Law-1 CP1/CP2, including close/preparation, source lease/safe replace, stdin transfer and VMM control support. General product approval does not waive these.
- Existing freeze/complete projections remain inconsistent; resolve exact applicable provenance, not rewrite historical receipts or reopen unrelated work.
- Intel VMX/guest artifacts and KVM/VMware host availability limit only named evidence. Whole requested desktop cannot be declared complete until every applicable AC has actual evidence.
- Native/guest app cannot provide reliable prepared-close => explicit user-controlled save/close or force confirmation; no generic third-party dirty-state oracle is assumed.

## Validation procedure

Run a documentation validator for local links, unique AC01–20, phase front-matter/index dependency equality, acyclic references, required sections and phase length. Review names in phase files are proposed interfaces until P01. Evidence: final validator output is captured in this session. No implementation smoke/test outcome is claimed by planning.
