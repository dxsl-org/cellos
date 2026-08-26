# Phase 01 — Complete robot-demo: MQTT telemetry + build verification

**Status:** ⬜ Todo  
**Priority:** High — blocking Phase 02  
**Blocked by:** nothing (Phase 27 typed IPC is merged)

---

## Context Links

- Skeleton: [cells/apps/robot-demo/src/main.rs](../../cells/apps/robot-demo/src/main.rs)
- Cargo: [cells/apps/robot-demo/Cargo.toml](../../cells/apps/robot-demo/Cargo.toml)
- MQTT protocol reference: [cells/apps/net-tools/src/bin/mqtt.rs](../../cells/apps/net-tools/src/bin/mqtt.rs)
- Net IPC types: [libs/api/src/ipc.rs](../../libs/api/src/ipc.rs)
- Service registry: [libs/api/src/syscall.rs](../../libs/api/src/syscall.rs) (`service::NET`)
- GPIO driver: [cells/drivers/gpio/src/lib.rs](../../cells/drivers/gpio/src/lib.rs)

---

## Overview

`cells/apps/robot-demo/src/main.rs` has a working GPIO control loop but MQTT is a serial stub. This phase implements real MQTT telemetry using `NetRequest` directly (inline MQTT protocol, no spawn of `/bin/mqtt`).

**Scope boundary:**
- ✅ Replace `publish_telemetry` stub with real MQTT CONNECT + PUBLISH + close
- ✅ Add `declare_syscalls![Send, Recv, Log, LookupService, Heartbeat]`
- ✅ Add `network = true` to `declare_manifest!`
- ✅ Sensor simulation on non-sensor paths (tick-based alternating HIGH/LOW)
- ✅ Graceful fallback when MQTT broker not reachable
- ✅ Verify `aarch64-unknown-none-softfloat` build succeeds
- ❌ Out of scope: MQTT subscribe, QoS > 0, TLS, multiple broker retry

---

## Key Insights

1. **No sys_spawn_args**: robot-demo doesn't take CLI args today. MQTT broker is hardcoded to QEMU gateway `[10, 0, 2, 2]:1883`. This is correct for the reference demo.

2. **Service lookup**: use `sys_lookup_service(service::NET)` — not hardcoded TID 6. The service registry clears dead endpoints; this also tests the full lookup path.

3. **Sensor on QEMU ARM**: PL061 GPIO pin 2 has no physical connection in QEMU virt. The `simulate_loop()` path already uses tick-based alternation. For the real GPIO path, we'll synthesize the sensor value from a tick counter (even = HIGH, odd = LOW) while still writing the actuator to the real GPIO pin — this shows GPIO output works while keeping the demo visually interesting.

4. **MQTT publish-only**: CONNECT → PUBLISH one telemetry message → CLOSE. No CONNACK wait loop needed for QoS-0 (can skip CONNACK check on timeout for robustness). Use a max-poll limit.

5. **Buffer layout**: postcard overhead + MQTT packet fit in 512-byte IPC buf. MQTT CONNECT packet is 18 bytes; PUBLISH with short topic+payload < 100 bytes. TcpSend chunks of up to 480 bytes.

6. **Heartbeat**: robot-demo is a short-lived transient cell (5-10 iterations). No heartbeat needed. But `Heartbeat` should still be in the allowlist so the kernel doesn't deny if ostd calls it internally.

---

## Architecture

```
main()
 ├── Pl061Gpio::open()  → Ok: real GPIO path
 │    ├── configure pins (SENSOR=input, ACTUATOR=output)
 │    └── for i in 0..LOOP_CYCLES:
 │         ├── sensor_val = (i % 2 == 0)   ← synthetic tick-based
 │         ├── gpio.write_pin(ACTUATOR, sensor_val)
 │         ├── log tick/sensor/actuator state
 │         └── yield_now()
 │    └── publish_telemetry("robot-demo/gpio", "loop_done", LOOP_CYCLES)
 │
 └── Err(PermissionDenied): simulate path
      ├── for i in 0..LOOP_CYCLES: log simulated tick
      └── publish_telemetry("robot-demo/sim", "sim_done", LOOP_CYCLES)

publish_telemetry(topic_suffix, event, count)
 ├── net_ep = sys_lookup_service(service::NET)
 ├── if net_ep == 0 → log "no net service" and return
 ├── NetRequest::TcpConnect { addr: [10,0,2,2], port: 1883 }
 ├── if Err → log "mqtt broker unreachable" and return
 ├── mqtt_connect(cap)          ← send MQTT CONNECT packet
 ├── wait for CONNACK (max 200 polls)
 ├── mqtt_publish(cap, topic, payload)
 └── NetRequest::TcpClose { cap }
```

---

## Related Code Files

**Modify:**
- `cells/apps/robot-demo/src/main.rs` — replace stub, add declarations
- `cells/apps/robot-demo/Cargo.toml` — no new deps needed (api + ostd already present)

---

## Implementation Steps

1. **Update `declare_manifest!`**: add `network = true` to existing manifest.

2. **Add `declare_syscalls!`**: `api::declare_syscalls![Send, Recv, Log, LookupService, Heartbeat];`

3. **Add imports**: `use api::ipc::{IPC_BUF_SIZE, NetRequest, NetResponse}; use ostd::syscall::{sys_recv, sys_send, sys_lookup_service, SyscallResult};`

4. **Change `control_step`**: replace `gpio.read_pin(SENSOR_PIN)` with synthetic tick-based value `(tick % 2 == 0)` while still writing GPIO actuator. Update comment.

5. **Implement `mqtt_connect(net_ep, cap) -> bool`**: 
   - Builds 18-byte MQTT CONNECT packet (`[0x10, 0x10, 0x00, 0x04, 'M','Q','T','T', 0x04, 0x02, 0x00, 0x3C, 0x00, 0x04, 'v','i','o','s']`)
   - Sends via `NetRequest::TcpSend`
   - Polls for CONNACK (max 200 × yield) via `NetRequest::TcpRecv { buf_len: 16 }`
   - Returns true if CONNACK received (b[0] == 0x20 && b[3] == 0x00)

6. **Implement `mqtt_publish(net_ep, cap, topic, payload)`**:
   - Builds MQTT PUBLISH packet (header + topic length (2 BE) + topic + payload)
   - Sends via `NetRequest::TcpSend`

7. **Rewrite `publish_telemetry(topic_suffix, event, count)`**:
   - Lookup net service: `sys_lookup_service(service::NET)`
   - Guard `if net_ep == 0 { log; return; }`
   - `NetRequest::TcpConnect { addr: [10,0,2,2], port: 1883 }`
   - Guard on Err: log "broker unreachable" and return
   - `mqtt_connect(net_ep, cap)` — if false: close + return
   - Build JSON payload: `{"device":"robot-demo","event":"<event>","count":<n>}`
   - `mqtt_publish(net_ep, cap, "vios/robot", &payload)`
   - `NetRequest::TcpClose { cap }`

8. **Net IPC helper**: add `net_send(net_ep, cap_id, data) -> usize` and `net_recv(net_ep, cap_id, buf_len, out) -> usize` helpers to avoid repeating IPC encode/decode boilerplate.

9. **Build check**: run `cargo check --manifest-path cells/apps/robot-demo/Cargo.toml` for default (riscv64) target. Then build for aarch64:
   ```
   cargo build --manifest-path cells/apps/robot-demo/Cargo.toml \
     --target aarch64-unknown-none-softfloat --release
   ```
   Fix any target-conditional compilation errors.

---

## Todo List

- [ ] `declare_manifest!(network=true, gpio=true, ...)`
- [ ] `declare_syscalls![Send, Recv, Log, LookupService, Heartbeat]`
- [ ] Add imports for net IPC
- [ ] Synthetic tick-based sensor value in `control_step`
- [ ] `mqtt_connect` helper function
- [ ] `mqtt_publish` helper function
- [ ] `publish_telemetry` — real implementation
- [ ] `net_send` / `net_recv` helpers
- [ ] `cargo check` riscv64 (default target)
- [ ] `cargo build` aarch64-unknown-none-softfloat

---

## Success Criteria

- `cargo check` passes for riscv64 (default)
- `cargo build --target aarch64-unknown-none-softfloat` passes
- `publish_telemetry` calls `NetRequest::TcpConnect` → MQTT CONNECT → PUBLISH → TcpClose
- Broker-absent path: logs warning and returns cleanly (no panic)
- GPIO-absent path (riscv64): simulation loop runs, telemetry still attempted

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| CONNACK timeout in QEMU (net service DHCP not ready) | Medium | Max 200-poll limit; silent skip if fails |
| aarch64 target missing `alloc` feature in some dep | Low | api + ostd already used by other ARM cells |
| MQTT packet length calculation off-by-one | Low | Follow mqtt.rs existing encode logic exactly |

---

## Security Considerations

- `network = true` manifest flag required; kernel grants cap at spawn
- No user input to broker address — hardcoded to QEMU gateway, no injection risk
- All sends bounded by IPC_BUF_SIZE
