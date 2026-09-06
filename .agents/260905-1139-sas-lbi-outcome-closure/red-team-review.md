# Red-Team Review and Adjudication

Date: 2026-09-05. Target: [approved Approach A plan](./plan.md), five phase documents and scout report. Reviewers inspected the draft; Main integrated corrections. No second independent pass is claimed.

## Review Coverage
- Trust/Security Adversary: 50 security-relevant claims/path facts inspected; 2 Major, 1 Minor reported.
- Assumption Destroyer: 41 claims inspected; 3 Major, 1 Minor reported.
- Failure Mode Analyst: 22 claims inspected; 4 Major, 1 Minor reported.
- Counts overlap; they are not 113 distinct verified properties. Twelve reported findings reduce to ten distinct findings: eight Major, two Minor. All accepted; two duplicate reports consolidated below.
- Inspection only: no new build, tests, QEMU, physical execution or successful exploit proof. Main checked cited dispatch/loader/stash/error paths and plan structure. Earlier comparator experiments remain the scout report's prior observed evidence.

## Accepted Findings
| ID | Severity | Evidence and failure | Plan disposition |
|---|---|---|---|
| F1 | Major | Supervisor polls then clears; delayed Snapshot can publish after rollback (`cells/services/supervisor/src/hotswap.rs:44-52,172-176`). Global stash has a 64-entry cap (`kernel/src/cell/state_stash.rs:19-26`). | Phase03 requires bounded completion/cancellation fencing and a delayed-publication oracle before claiming reclaimed transaction state. Current pause/clear alone is insufficient. |
| F2 / T3 | Major | Restore is one-way; errors have no schema/rollback status; rollback discards failures (`cells/services/supervisor/src/transfer.rs:21-31`, `error.rs:7-23`, `hotswap.rs:172-176`). | Preserve numeric wire meaning. ReadyTimeout reports failure to become ready; correlate replacement diagnostics and checked supervisor rollback/provider evidence separately. CLI cannot infer rollback success. T3's Minor report duplicates this Major contract gap. |
| F3 | Major | Comparator silently ignores corrupt streak state (`scripts/compare-bench-results.sh:65-71`). | Phase01 reconstructs ordered distinct valid history or fails the regression row closed; it cannot reset a third bad run into green. |
| F4 / A2 | Major | Recovery handler accepts hypervisor only; init's recovery feature requires hypervisor-min (`cells/services/supervisor/src/hostile_backend_recovery.rs:11-18`, `cells/tools/init/src/main.rs:5-6`). | Phase05 owns a distinct private native-test bridge using unchanged wire, exact trusted launch-instance authority and VFS-only targets. Existing hypervisor/normal-build policy is not relaxed. Actual combined-image recovery still needs execution. |
| F5 | Minor | Initial whole-phase 01/02/03 -> 04 dependency blocked independent measurements. | Phase04 requires Phase01 globally, with quota/recovery dependencies per row. Phase05 whole-phase prerequisites are 02/03 plus explicit consumed Phase04 budget rows, not unrelated unfinished rows. Rebind/rerun affected rows after changes. |
| A1 | Major | Generic bench spawns bench-probe; actual echo is one zero byte (`cells/tests/bench/src/scenarios/ipc_send_recv.rs:13,25-26`, `cells/tests/bench/src/bench-probe.rs:44-51`). | Correct Phase01/04/scout to 64-byte request / one-byte-zero reply. Different reply semantics require a different comparison profile. |
| A3 | Major | Existing cached-TID helper adds one inc and expects 5->6 (`cells/tests/bench/src/scenarios/hotswap_supervisor.rs:103-107,162-169`). | Phase05 specifies 999 primary increments plus one witness increment: operation301 at the 300 boundary. Parameterize helper; total remains exactly1,000. |
| A4 | Minor | Date-only result filename overwrites same-day captures; comparator advances streak per invocation (`.github/workflows/perf.yml:143-149`, `scripts/compare-bench-results.sh:104-108`). | Phase01 requires immutable run/profile/repetition IDs, collision-free retention and atomic processed-identity/streak state. Content provenance and cross-revision compatibility are distinct. |
| T1 | Major | Supervisor authorizes display name (`cells/services/supervisor/src/main.rs:54-58,132-143`); memory-spawn labels are caller-controlled and basename becomes task name (`kernel/src/loader/mem_spawn_gate.rs:4-13`, `kernel/src/loader/governed_spawn.rs:157-158`). | Phase03 has an exact kernel-authenticated launch-principal gate and forged-name denial oracle. Display name or first-seen TID is not an authority bootstrap. A missing usable interface requires an exact design checkpoint. |
| T2 | Major | Stash is global caller-chosen key->bytes; non-argv read/clear lacks transaction/source binding (`kernel/src/cell/state_stash.rs:24-26,45-80`, `kernel/src/task/syscall.rs:5060-5129`). | Phase03 owns source-generation/swap/replacement binding and hostile read/overwrite/clear cases, preserving legitimate argv/non-hotswap users. No generic stash rewrite or new ABI is silently authorized. |

## Adjudication Boundaries
- Trusted SAS does not imply malicious-native-code isolation. T1/T2 address declared capability/delegation and state-preservation contracts, not sandboxing arbitrary unsafe native code.
- Reporting ReadyTimeout plus independently checked rollback is the selected existing-wire contract; richer public outcomes are not smuggled into old codes. A failed rollback stays a failed outcome.
- Authority, transaction binding and snapshot fencing are mandatory Phase03 design prerequisites, not completed fixes. If existing interfaces cannot express them, stop that slice for the exact design approval while other lanes continue.
- The native restart bridge is a bounded, private test-fixture adaptation to reach the approved workload; it does not enable a production kill endpoint or weaken the existing hypervisor-only route.
- Counter/quota and combined-image RedoxFS recovery remain runtime-unverified. Memory target misses remain open; valid evidence does not imply performance success.
- Prior signed/hash-bound approvals and ledgers remain immutable historical evidence. Shared-source edits require the appropriate fresh evidence; no automatic PAL or production promotion.

## Planning Validation
- Initial draft check passed: seven documents, index68 lines, phase files64–67 lines, required sections present and dependency graph acyclic.
- Exact-path inventory found one mistaken API path; corrected to `libs/api/src/services/benchmark.rs`. The comparator test and native workload source are explicitly proposed new files, not missing prerequisites.
- Final revised-plan structural check PASS, persisted in `validation.json`: 8 documents, 9 relative links, 47 exact file references with 2 explicitly proposed new sources; required sections and dependency DAG valid. Index69 lines, phase files64–71 lines. This is not a build or runtime test.

## Next Action
Start [Phase01](./phase-01-evidence-validity.md) through hc-cook. Retain the phase-local design/approval gates; do not mark M1/M3 or production complete from this plan.
