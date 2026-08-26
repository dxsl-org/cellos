---
title: "Phase X-5 — MQTT Client Cell"
description: "New /bin/mqtt cell: QoS-0 publish + subscribe over TCP via net IPC."
status: ready
priority: P3
effort: 3h
branch: main
tags: [ViCell, networking, mqtt, iot]
created: 2026-06-05
---

# Phase X-5 — MQTT Client Cell

MQTT 3.1.1 QoS-0 client as a new `/bin/mqtt` cell.
Reuses net IPC (same opcodes as nc.rs). No kernel/driver changes.

## Phases

| # | File | Lines budget |
|---|------|-------------|
| 01 | [mqtt.rs](phase-01-mqtt-rs.md) | 185 lines |
| 02 | [Cargo + disk + test](phase-02-wiring.md) | small edits |

## Dependencies

- Depends on: nothing (net IPC already functional)
- Blocks: nothing
- lib.rs: add `spawn_mqtt_broker()` helper (~40 lines)

## Quick Test Command

After implementation:
```
cargo test --manifest-path tests/integration/Cargo.toml \
  --target x86_64-pc-windows-msvc "mqtt"
```
