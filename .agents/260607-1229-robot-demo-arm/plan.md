# Plan: Reference Robot Demo — Sensor → Actuator → MQTT on QEMU ARM

**Goal:** G1 graduation criterion 8 — fully working sensor→compute→actuator loop with live MQTT telemetry on QEMU ARM virt.

**Context:** `cells/apps/robot-demo` skeleton exists. GPIO control loop is complete. MQTT is a serial-print stub. Phase 27 just shipped typed net IPC. ARM64 HAL is complete; `run-arm-virt.ps1` boots the kernel.

**Key decision:** Implement MQTT inline via `NetRequest` (not spawning `/bin/mqtt`) to keep the demo self-contained and to exercise the typed IPC path from a non-net-tools cell.

---

## Phases

| # | File | Status | Summary |
|---|------|--------|---------|
| [01](phase-01-robot-demo-mqtt.md) | `cells/apps/robot-demo/` | ✅ Done | Complete MQTT publish + build verification |
| [02](phase-02-init-run-integration.md) | `cells/apps/init/`, `run-arm-virt.ps1` | ✅ Done | Init auto-start + QEMU MQTT host-forward |

Phase 02 blocks on Phase 01.

---

## Key Dependencies

- Phase 27 typed net IPC (merged) — provides `NetRequest::TcpConnect`, `NetResponse::CapId`
- ARM HAL complete — `hal/arch/arm/src/aarch64/` all modules present
- `run-arm-virt.ps1` — QEMU ARM virt machine, PL061 GPIO @ 0x0903_0000
- Service registry — `sys_lookup_service(service::NET)` for dynamic endpoint
