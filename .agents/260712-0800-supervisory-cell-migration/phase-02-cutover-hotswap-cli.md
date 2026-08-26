# Phase 02 — Cut Over the Hotswap CLI/Shell to the Supervisor Cell

## Context Links
- Plan: [plan.md](plan.md)
- CLI still on kernel path: `cells/tools/sys-tools/src/bin/hotswap.rs:73` (`sys_hotswap`)
- Supervisor IPC entry: `cells/services/supervisor/src/main.rs:34-63` (OP_HOTSWAP)
- Supervisor protocol: `cells/services/supervisor/src/protocol.rs`
- Service lookup: `service::SUPERVISOR=11`, `sys_lookup_service`
- Test asset: `tests/integration/tests/hotswap-smoke.rs`

## Overview
- **Priority**: P1 (this is the actual "migration" the law asks for — stop using the kernel orchestrator). **Status**: complete. **Risk**: MED.
- Redirect the user-facing hotswap trigger from `sys_hotswap` (kernel orchestrator, `HotSwap=400`)
  to an IPC request to `service::SUPERVISOR`. After this phase the kernel `hotswap()` path is dead
  code (deleted in Phase 04) but retained as fallback.

## Key Insights
- The supervisor's IPC entry keys hotswap by **service name → service_id** (`main.rs:73-81`), not by
  arbitrary cell tid. The current CLI resolves a cell by name via `ps` and passes a `CellId`. The
  cutover must reconcile these: the supervisor only hotswaps registered services (vfs/net/compositor/input).
  Swapping an arbitrary non-service cell (e.g. a demo) needs the protocol to also accept a raw tid, OR
  the demo must register a service. **Decide before coding** (see Steps).
- `sys_lookup_service` + `sys_send` + wait-for-reply is the standard client pattern (mirror net/vfs clients).
- The supervisor replies with a status envelope (`encode_status`, `main.rs:41/47/53/57`) — the CLI must parse it and report success/failure instead of "see kernel log".

## Requirements
- Functional: `hotswap <name> <elf>` sends an `OP_HOTSWAP` request to the supervisor and reports the returned status; no `sys_hotswap` call remains in `cells/`.
- Functional: the automated e2e (demo-v1 inc×5 → swap → get==5) passes through the supervisor path.
- Non-functional: latency within the existing ~5s swap budget; clear error messages from status codes.

## Architecture / Data Flow
```
shell: hotswap net /bin/net-v2
  CLI: sup_tid = sys_lookup_service(SUPERVISOR)
       sys_send(sup_tid, HotswapRequest{ service_name="net", elf_path="/bin/net-v2" })
       reply = sys_recv() → status envelope → print
  supervisor: hotswap::hotswap(service_id, elf_path)   [Phases 00+01 make this correct+loss-free]
```

## Related Code Files
- Modify `cells/tools/sys-tools/src/bin/hotswap.rs`: replace the `sys_hotswap` call with the lookup+send+recv client flow; parse status envelope.
- Modify `cells/services/supervisor/src/main.rs` + `protocol.rs`: (if supporting arbitrary cells) extend `HotswapRequest` / `service_id_for_name` to accept a tid or a wider name set; otherwise document service-only scope.
- Modify `tests/integration/tests/hotswap-smoke.rs`: add an end-to-end swap scenario driving the CLI (or a scripted request) and asserting counter preservation; add a privileged-service swap to exercise Phase 00.

## Implementation Steps
1. Decide swap-target model: **service-only** (simplest, matches current supervisor) vs **name-or-tid**. Recommend service-only for G1 (YAGNI) — register the hotswap demo as a throwaway service, or add a demo-name arm. Document the decision inline.
2. Rewrite the CLI to the IPC client flow; map status codes → human messages.
3. If service-only: make the demo swap scenario register a service so the e2e can target it.
4. Add the automated e2e swap to hotswap-smoke (currently only spawns + unit-tests key derivation — line 5 says full swap is not automated).
5. Add a privileged-service swap case (exercises Phase 00 cap inheritance).
6. Boot 3 arches; run reliability + hotswap-smoke green.

## Todo List
- [x] Decide + document swap-target model
- [x] CLI → supervisor IPC client flow + status parsing
- [x] Demo/service registration for e2e target
- [x] hotswap-smoke: automated e2e swap (state preserved)
- [x] hotswap-smoke: privileged-service swap (cap retained)
- [x] Affected RV64 cells plus RV64/AArch64/x86_64 kernel release builds green; RV64 QEMU suite green
- [ ] AArch64/x86_64 runtime boot smoke (host-gated follow-up; not executed or claimed by this phase)

## Success Criteria
- [x] `hotswap` CLI has no direct `sys_hotswap(` call in `cells/`; lifecycle `sys_hotswap_ready` calls remain intentionally.
- [x] e2e: inc×5 → swap → get==5 automated and green.
- [x] Privileged swap: replacement retains its cap (asserts Phase 00).

## Evidence
- `cargo fmt --all -- --check && git diff --check` — pass
- `RV64 release build for kernel, hotswap CLI, supervisor, bench, and demo v1/v2; aarch64 and x86_64 kernel release builds` — pass
- `pwsh -NoProfile -File ./gen_disk.ps1` — pass; fresh disk image contains signed `/bin/hotswap`
- `cargo test --target x86_64-unknown-linux-gnu --test hotswap-smoke hotswap_cli_preserves_demo_state -- --nocapture --test-threads=1` — pass
- `cargo test --target x86_64-unknown-linux-gnu --test hotswap-smoke supervisor_hotswap_preserves_demo_state -- --nocapture --test-threads=1` — pass
- `cargo test --target x86_64-unknown-linux-gnu --test hotswap-smoke supervisor_rejects_unauthorized_hotswap_sender -- --nocapture --test-threads=1` — pass
- `cargo test --target x86_64-unknown-linux-gnu --test hotswap-smoke -- --nocapture --test-threads=1` — pass 15/15, zero skips
- Standard review: PASS 9.2; domain-risk review: PASS; artifact validation: PASS
- AArch64/x86_64 runtime boot smoke remains host-gated and deferred; only their kernel release builds are claimed here

## Risk Assessment
- **Supervisor unreachable** (crashed mid-restart) → CLI must surface "supervisor unavailable, retry" not hang. Mitigation: bounded recv timeout on the reply; init restarts the supervisor (Permanent policy).
- **Protocol scope creep** — resist adding tid-based swaps if service-only suffices (YAGNI).
- **Test flakiness** — swap timing under QEMU TCG. Mitigation: use the harness `cmd && echo DONE$?` barrier pattern ([[project-test-harness-wait-for-race]]), not bare `wait_for` after send.

## Security Considerations
- The supervisor gates on holding SupervisorCap for the mechanism syscalls; the CLI itself needs no new cap (it just sends IPC). Ensure the supervisor validates request framing (`HotswapRequest::parse` already rejects malformed input, `main.rs:40-42`).

## Next Steps
Phase 04 deletes the kernel orchestrator once this + Phase 03 are green.
