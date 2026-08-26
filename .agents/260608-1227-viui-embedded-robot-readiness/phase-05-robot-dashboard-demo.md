# Phase 05 — Robot Dashboard Demo

**Status:** Planned  
**Priority:** Medium — integration validation + showcase  
**Estimate:** 2 ngày  
**Depends on:** Phase 01 + 02 + 03 (font + widgets + animation must be done)

---

## Context Links

- [`cells/apps/viui-demo/`](../../../cells/apps/viui-demo/) — existing demo pattern
- [`cells/apps/viui-demo/src/main.rs`](../../../cells/apps/viui-demo/src/main.rs) — main loop pattern
- [`libs/viui/src/app_runner.rs`](../../../libs/viui/src/app_runner.rs) — ViApp API

---

## Overview

Build một robot dashboard app thực tế — không phải toy counter. Mục đích:

1. **Integration test**: validate toàn bộ stack Phase 01-04 cùng nhau
2. **Showcase**: chứng minh ViUI đạt embedded/robot readiness
3. **Regression safety**: nếu demo không chạy, có gì sai trong pipeline

---

## Dashboard Design

```
┌─────────────────────────────────────────────┐
│  ViCell Robot Dashboard          [●] Live   │
├──────────────┬──────────────────────────────┤
│ SENSORS      │  CONTROLS                    │
│              │                              │
│ Battery 87%  │  Speed     [────●────]  0.5  │
│ [████████░░] │                              │
│              │  Gain      [──●──────]  0.3  │
│ CPU 23%      │                              │
│ [██░░░░░░░░] │  [  STOP  ]  [  START  ]    │
│              │                              │
│ Motor Temp   │                              │
│ 42°C  [████░] │                             │
├──────────────┴──────────────────────────────┤
│ EVENT LOG                                   │
│ > Motor started at t=1203ms                 │
│ > Battery warning: 87% → charging           │
│ > Sensor A timeout — retry 1/3              │
└─────────────────────────────────────────────┘
```

---

## Requirements

- **Sensors panel**: 3 ProgressBar với animated Signal<f32> (battery, CPU, temp)
- **Controls panel**: 2 Slider (speed, gain) + 2 Button (STOP/START)
- **Event log**: ListView với Signal<Vec<String>>, auto-scroll on new entry
- **Status indicator**: animated blink (dot color alternates red/green via Tween)
- **Simulated sensor data**: update mỗi 500ms từ fake data generator trong main loop
- **Font**: scalable (16px labels, 20px section headers, 12px log entries)

---

## Architecture

### Cell structure

```
cells/apps/robot-dashboard/
├── Cargo.toml
├── build.rs          (viui-build compile *.vi)
├── src/
│   ├── main.rs       (main loop + sensor simulation)
│   ├── sensors.rs    (SimulatedSensors — generates fake readings)
│   └── dashboard.vi  (optional — DSL-defined layout if Phase 04 ready)
```

### main.rs skeleton

```rust
#![no_std]
#![no_main]

extern crate alloc;
use alloc::{boxed::Box, string::String, vec::Vec};

use ostd::startup::entry;
use viui::{
    ViApp,
    node_widgets::{Column, Row, Label, ProgressBar, Slider, Button, ListView, TouchArea},
    animation::AnimatedSignal,
    signal::Signal,
    theme::KioskTheme,
};

entry!(main);

fn main() {
    let renderer = /* get from compositor IPC */;

    // ── Signals ──────────────────────────────────────────────────────
    let battery     = AnimatedSignal::new(0.0_f32);
    let cpu_usage   = AnimatedSignal::new(0.0_f32);
    let motor_temp  = AnimatedSignal::new(0.0_f32);
    let speed       = Signal::new(0.5_f32);
    let gain        = Signal::new(0.3_f32);
    let running     = Signal::new(false);
    let log_entries: Signal<Vec<String>> = Signal::new(Vec::new());
    let status_color = AnimatedSignal::new(0.0_f32); // 0=red, 1=green

    // ── Layout ───────────────────────────────────────────────────────
    let sensors_panel = Column::new(vec![
        Box::new(Label::new(Signal::new("SENSORS".into()))),
        Box::new(ProgressBar::new(battery.signal()).color(Color::GREEN)),
        Box::new(ProgressBar::new(cpu_usage.signal()).color(Color::YELLOW)),
        Box::new(ProgressBar::new(motor_temp.signal()).color(Color::RED)),
    ]);

    let speed_clone = speed.clone();
    let gain_clone  = gain.clone();
    let log_clone   = log_entries.clone();
    let running_start = running.clone();

    let controls_panel = Column::new(vec![
        Box::new(Label::new(Signal::new("CONTROLS".into()))),
        Box::new(Slider::new(speed.clone()).on_change(move |v| {
            speed_clone.set(v);
        })),
        Box::new(Slider::new(gain.clone()).on_change(move |v| {
            gain_clone.set(v);
        })),
        Box::new(Row::new(vec![
            Box::new(Button::new(Signal::new("STOP".into()), Box::new(move || {
                running.set(false);
                log_clone.update(|v| v.push("Robot stopped".into()));
            }))),
            Box::new(Button::new(Signal::new("START".into()), Box::new(move || {
                running_start.set(true);
            }))),
        ])),
    ]);

    let log_list = ListView::new(log_entries.clone());

    let root = Column::new(vec![
        Box::new(Row::new(vec![
            Box::new(sensors_panel),
            Box::new(controls_panel),
        ])),
        Box::new(log_list),
    ]);

    let mut app = ViApp::new(Box::new(root), renderer);
    app.add_animation(Box::new(battery.clone()));
    app.add_animation(Box::new(cpu_usage.clone()));
    app.add_animation(Box::new(motor_temp.clone()));

    // ── Main loop ────────────────────────────────────────────────────
    let mut last_sensor_update = ostd::io::now_ms();
    let mut frame_time = ostd::io::now_ms();

    loop {
        let now = ostd::io::now_ms();
        let dt = (now - frame_time) as u32;
        frame_time = now;

        // Sensor simulation — update every 500ms
        if now - last_sensor_update > 500 {
            last_sensor_update = now;
            battery.animate_to(simulated_battery(), 400);
            cpu_usage.animate_to(simulated_cpu(), 200);
            motor_temp.animate_to(simulated_temp(), 300);
        }

        let events = collect_input_events();
        app.tick_with_dt(&events, dt);
    }
}
```

### Sensor simulation

```rust
// src/sensors.rs
static mut SIM_T: u32 = 0;

pub fn simulated_battery() -> f32 {
    // Slow discharge: 1.0 → 0.7 over 30s
    unsafe { SIM_T += 1; }
    (1.0 - unsafe { SIM_T } as f32 * 0.001).clamp(0.7, 1.0)
}

pub fn simulated_cpu() -> f32 {
    // Sine wave 0.1–0.9
    let t = unsafe { SIM_T } as f32 * 0.05;
    0.5 + 0.4 * libm::sinf(t)
}

pub fn simulated_temp() -> f32 {
    // Slow ramp 20°C → 60°C normalized 0–1
    (unsafe { SIM_T } as f32 * 0.002).clamp(0.0, 1.0)
}
```

**libm**: `no_std` math library — đã có trong ostd dep hoặc thêm vào Cargo.toml.

---

## Related Code Files

| File | Action |
|------|--------|
| `cells/apps/robot-dashboard/Cargo.toml` | CREATE |
| `cells/apps/robot-dashboard/src/main.rs` | CREATE |
| `cells/apps/robot-dashboard/src/sensors.rs` | CREATE |
| `Cargo.toml` (workspace) | MODIFY — add robot-dashboard to members |

---

## Implementation Steps

1. Scaffold cell: `Cargo.toml` + `main.rs` bones
2. Wire sensors panel (3 × ProgressBar với AnimatedSignal)
3. Wire controls panel (2 × Slider + 2 × Button)
4. Wire event log (ListView)
5. Wire input events (collect từ input service IPC)
6. Add sensor simulation loop
7. Build + boot on ViOS — verify visual output
8. Capture screenshot / record terminal output để document

---

## Todo

- [ ] Scaffold cells/apps/robot-dashboard/
- [ ] Add to workspace Cargo.toml
- [ ] Implement sensors panel với AnimatedSignal
- [ ] Implement controls panel với Slider + Button
- [ ] Implement event log với ListView
- [ ] Verify compositor IPC pattern (from viui-demo)
- [ ] `cargo check` full workspace
- [ ] Boot trên ViOS QEMU, verify dashboard renders
- [ ] Verify ProgressBar animates khi sensor values thay đổi
- [ ] Verify Slider drag cập nhật speed/gain label
- [ ] Verify ListView auto-adds log entries

---

## Success Criteria

- Dashboard boots và renders đầy đủ trên ViOS
- 3 sensor ProgressBars animate smooth khi giá trị thay đổi
- Slider drag thay đổi value, Label cập nhật realtime
- STOP button thêm entry vào ListView
- Không crash sau 60 giây chạy liên tục
- Font scalable 16px rõ ràng (không pixelated 8×8)

---

## Risk

**Compositor IPC**: viui-demo đã có pattern này — copy/adapt. Nếu compositor chưa up, dùng
`ViSurface` trực tiếp như viui-demo.

**libm sinf**: Nếu không có libm trong deps, replace sinf với linear ramp cho simulation —
không ảnh hưởng đến feature validation.

**Input service events → ViUI Event mapping**: cần convert từ input service IPC messages
sang `viui::Event` enum. Pattern này đã có trong input service design — reference từ input
cell source.
