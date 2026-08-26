# Phase 02 — Robot Dashboard Demo

**Status:** Planned  
**Stage:** G1  
**Priority:** High  
**Estimate:** 2 ngày  
**Depends on:** Phase 01 (ListView), embedded/robot P01-P03 (animation, slider, progress_bar — DONE)

---

## Context Links

- [`cells/apps/viui-demo/`](../../../cells/apps/viui-demo/) — existing demo pattern (main loop, compositor IPC)
- [`cells/apps/viui-demo/src/main.rs`](../../../cells/apps/viui-demo/src/main.rs) — ViApp::new + tick_with_dt pattern
- [`libs/viui/src/node_widgets/`](../../../libs/viui/src/node_widgets/) — widget APIs
- [`libs/viui/src/animation.rs`](../../../libs/viui/src/animation.rs) — AnimatedSignal, Tween
- [`Cargo.toml`](../../../Cargo.toml) — workspace members

---

## Overview

Build robot dashboard app thực tế — không phải toy counter. Mục đích:

1. **Integration test**: validate toàn bộ stack (font, animation, slider, touch, ListView) cùng nhau
2. **Showcase**: chứng minh ViUI đạt embedded/robot readiness cho G1
3. **Regression safety**: dashboard không chạy = có lỗi trong pipeline

---

## Dashboard Layout

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
│ 42°C [████░] │                             │
├──────────────┴──────────────────────────────┤
│ EVENT LOG                                   │
│ > Motor started at t=1203ms                 │
│ > Battery: 87% normal                       │
│ > Sensor A timeout — retry 1/3              │
└─────────────────────────────────────────────┘
```

---

## Architecture

### Cell structure

```
cells/apps/robot-dashboard/
├── Cargo.toml
├── src/
│   ├── main.rs       — main loop + layout
│   └── sim.rs        — simulated sensor data (no real HW needed)
```

Không dùng `build.rs` + `.vi` DSL cho phase này — pure Rust Layer 2 API để tránh phụ thuộc P04 DSL registry. P04 completion sau sẽ cho phép port sang `.vi`.

### Cargo.toml

```toml
[package]
name    = "robot-dashboard"
version = "0.1.0"
edition = "2021"

[dependencies]
viui  = { path = "../../../libs/viui" }
ostd  = { path = "../../../libs/ostd" }
libm  = { version = "0.2", default-features = false }

[[bin]]
name             = "robot-dashboard"
path             = "src/main.rs"
```

### main.rs skeleton

```rust
#![no_std]
#![no_main]

extern crate alloc;
use alloc::{boxed::Box, string::String, vec::Vec, format};

use ostd::startup::entry;
use viui::{
    ViApp,
    node_widgets::{Column, Row, Label, ProgressBar, Slider, Button, ListView},
    animation::AnimatedSignal,
    signal::Signal,
    theme::KioskTheme,
};

use crate::sim::{SimState, SIM_TICK_MS};

mod sim;

entry!(main);

fn main() {
    let renderer = /* copy pattern từ viui-demo */;

    // ── Sensor signals ─────────────────────────────────────────────
    let battery_sig    = AnimatedSignal::new(0.87_f32);
    let cpu_sig        = AnimatedSignal::new(0.23_f32);
    let motor_temp_sig = AnimatedSignal::new(0.42_f32);

    // ── Control signals ─────────────────────────────────────────────
    let speed   = Signal::new(0.5_f32);
    let gain    = Signal::new(0.3_f32);
    let running = Signal::new(true);

    // ── Event log ───────────────────────────────────────────────────
    let log: Signal<Vec<String>> = Signal::new(Vec::new());

    // ── Live indicator ──────────────────────────────────────────────
    let live_blink = AnimatedSignal::new(1.0_f32); // 1=green, 0=red (blink)

    // ── Layout ──────────────────────────────────────────────────────
    // [sensors panel]
    let battery_label = Label::new({
        let b = battery_sig.signal();
        Signal::map(b, |v| format!("Battery {:.0}%", v * 100.0))
    });
    let sensors = Column::new(vec![
        Box::new(Label::new(Signal::new("SENSORS".into()))),
        Box::new(battery_label),
        Box::new(ProgressBar::new(battery_sig.signal()).color(viui::canvas::Color::GREEN)),
        Box::new(Label::new(Signal::new("CPU".into()))),
        Box::new(ProgressBar::new(cpu_sig.signal()).color(viui::canvas::Color::YELLOW)),
        Box::new(Label::new(Signal::new("Motor Temp".into()))),
        Box::new(ProgressBar::new(motor_temp_sig.signal()).color(viui::canvas::Color::RED)),
    ]);

    // [controls panel]
    let speed_c   = speed.clone();
    let gain_c    = gain.clone();
    let log_stop  = log.clone();
    let log_start = log.clone();
    let run_stop  = running.clone();
    let run_start = running.clone();

    let controls = Column::new(vec![
        Box::new(Label::new(Signal::new("CONTROLS".into()))),
        Box::new(Label::new(Signal::new("Speed".into()))),
        Box::new(Slider::new(speed.clone())
            .on_change(move |v| { speed_c.set(v); })),
        Box::new(Label::new(Signal::new("Gain".into()))),
        Box::new(Slider::new(gain.clone())
            .on_change(move |v| { gain_c.set(v); })),
        Box::new(Row::new(vec![
            Box::new(Button::new(Signal::new("STOP".into()), Box::new(move || {
                run_stop.set(false);
                log_stop.update(|v| v.push("Robot stopped".into()));
            }))),
            Box::new(Button::new(Signal::new("START".into()), Box::new(move || {
                run_start.set(true);
                log_start.update(|v| v.push("Robot started".into()));
            }))),
        ])),
    ]);

    let event_log = ListView::new(log.clone()).item_height(22.0);

    let root = Column::new(vec![
        Box::new(Label::new(Signal::new("ViCell Robot Dashboard".into()))),
        Box::new(Row::new(vec![
            Box::new(sensors),
            Box::new(controls),
        ])),
        Box::new(Label::new(Signal::new("EVENT LOG".into()))),
        Box::new(event_log),
    ]);

    let mut app = ViApp::new(Box::new(root), renderer)
        .with_theme(Box::new(KioskTheme));
    app.add_animation(Box::new(battery_sig.clone()));
    app.add_animation(Box::new(cpu_sig.clone()));
    app.add_animation(Box::new(motor_temp_sig.clone()));
    app.add_animation(Box::new(live_blink.clone()));

    // ── Main loop ────────────────────────────────────────────────────
    let mut sim        = SimState::new();
    let mut last_tick  = ostd::io::now_ms();
    let mut frame_time = ostd::io::now_ms();

    loop {
        let now    = ostd::io::now_ms();
        let dt     = (now - frame_time).min(100) as u32; // cap 100ms để tránh spiral
        frame_time = now;

        // Sensor update mỗi SIM_TICK_MS
        if now - last_tick > SIM_TICK_MS {
            last_tick = now;
            sim.tick();
            battery_sig.animate_to(sim.battery, 400);
            cpu_sig.animate_to(sim.cpu, 200);
            motor_temp_sig.animate_to(sim.motor_temp, 300);

            if let Some(event) = sim.pop_log_event() {
                log.update(|v| {
                    v.push(event);
                    // Cap log at 50 entries
                    if v.len() > 50 { v.remove(0); }
                });
            }
        }

        // Blink live indicator 0→1→0 every 1s
        if (now / 500) % 2 == 0 {
            live_blink.animate_to(1.0, 250);
        } else {
            live_blink.animate_to(0.3, 250);
        }

        let events = collect_events();
        app.tick_with_dt(&events, dt);
    }
}
```

### sim.rs

```rust
pub const SIM_TICK_MS: u64 = 500;

pub struct SimState {
    t:          u32,
    log_queue:  Vec<String>,
}

impl SimState {
    pub fn new() -> Self {
        Self { t: 0, log_queue: Vec::new() }
    }

    pub fn tick(&mut self) {
        self.t += 1;

        // Battery slow discharge: 1.0 → 0.7
        self.battery = (1.0 - self.t as f32 * 0.001).clamp(0.7, 1.0);

        // CPU: triangle wave 0.1–0.9
        let phase = (self.t % 20) as f32 / 20.0;
        self.cpu = if phase < 0.5 { 0.1 + phase * 1.6 } else { 0.9 - (phase - 0.5) * 1.6 };

        // Motor temp: slow ramp 0.2 → 0.8
        self.motor_temp = (0.2 + self.t as f32 * 0.003).clamp(0.2, 0.8);

        // Log events at intervals
        if self.t % 10 == 0 {
            self.log_queue.push(format!("t={}s  Battery {:.0}%", self.t / 2, self.battery * 100.0));
        }
        if self.t % 7 == 0 {
            self.log_queue.push(format!("t={}s  CPU spike {:.0}%", self.t / 2, self.cpu * 100.0));
        }
    }

    pub fn pop_log_event(&mut self) -> Option<String> {
        if self.log_queue.is_empty() { None } else { Some(self.log_queue.remove(0)) }
    }

    pub battery:    f32,
    pub cpu:        f32,
    pub motor_temp: f32,
}
```

---

## Signal::map

`AnimatedSignal::signal()` trả về `Signal<f32>`. Để derive label text, cần `Signal::map()`:

```rust
// libs/viui/src/signal.rs — nếu chưa có:
impl<T: Clone + 'static> Signal<T> {
    pub fn map<U: Clone + 'static>(&self, f: impl Fn(&T) -> U + 'static) -> Signal<U> {
        let init = f(&*self.borrow());
        let out  = Signal::new(init);
        let out2 = out.clone();
        self.subscribe(move |v| { out2.set(f(v)); });
        out
    }
}
```

Nếu đã có `Signal::map` → kiểm tra API signature. Nếu chưa có → thêm trong phase này (chỉ 10 dòng).

---

## Related Code Files

| File | Action |
|------|--------|
| `cells/apps/robot-dashboard/Cargo.toml` | CREATE |
| `cells/apps/robot-dashboard/src/main.rs` | CREATE |
| `cells/apps/robot-dashboard/src/sim.rs` | CREATE |
| `Cargo.toml` (workspace) | MODIFY — add robot-dashboard to members |
| `libs/viui/src/signal.rs` | MODIFY — add Signal::map if missing |

---

## Implementation Steps

1. Scaffold cell: `Cargo.toml` + empty `main.rs` + `sim.rs`
2. Add to workspace `Cargo.toml`
3. Implement `sim.rs` (SimState + fake sensor data)
4. Implement sensors panel (3 ProgressBar + labels)
5. Implement controls panel (2 Slider + 2 Button)
6. Implement event log (ListView + auto-add entries)
7. Check `Signal::map` exists — add if missing
8. Wire `collect_events()` (copy từ viui-demo)
9. `cargo check` full workspace
10. Boot trên ViOS QEMU, verify renders

---

## Todo

- [ ] Scaffold cells/apps/robot-dashboard/
- [ ] Add to workspace Cargo.toml
- [ ] Implement sim.rs (SimState + fields + tick + log)
- [ ] Implement sensors panel layout
- [ ] Implement controls panel layout
- [ ] Implement event log (ListView + Signal<Vec<String>>)
- [ ] Check/add Signal::map
- [ ] Wire collect_events (copy viui-demo pattern)
- [ ] cargo check full workspace
- [ ] Boot on QEMU, verify: sensors animate, slider works, log grows
- [ ] Verify no crash after 60s

---

## Success Criteria

- Dashboard boots + renders trên ViOS QEMU
- 3 sensor ProgressBars animate smooth khi sensor values thay đổi
- Slider drag thay đổi speed/gain
- STOP/START buttons thêm entries vào ListView
- ListView scroll down khi log grows quá chiều cao
- Font scalable 16px (không pixelated 8×8)
- Không crash sau 60s continuous run

---

## Risk

**Signal::map borrow**: nếu `signal.subscribe()` đã lấy Rc ref, map tạo vòng lặp
Rc → closure → Rc. Pattern: output signal không hold ref ngược về input. Kiểm tra drop behavior.

**collect_events() IPC**: copy từ viui-demo. Nếu input service chưa running trong QEMU session → dùng empty event list (demo vẫn hoạt động với simulated data).

**libm dep**: dùng triangle wave thay sinf để tránh libm dep. Đơn giản + deterministic.
