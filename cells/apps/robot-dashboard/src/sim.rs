// SPDX-License-Identifier: MIT
//! Robot sensor simulator — deterministic, no libm, no floating-point trig.
//!
//! All values are in [0.0, 1.0] (normalized).  `tick()` is called at
//! `SIM_TICK_MS`-millisecond intervals.  The log queue is drained by `main`
//! after each tick via `pop_log_event()`.

#![allow(dead_code)]

extern crate alloc;
use alloc::{format, string::String, vec::Vec};

/// How many milliseconds between simulator ticks.
pub const SIM_TICK_MS: u64 = 500;

/// Simulated robot state.
pub struct SimState {
    /// Tick counter (incremented once per `tick()`).
    t: u32,
    /// Battery charge [0.0, 1.0].
    pub battery: f32,
    /// CPU utilization [0.0, 1.0].
    pub cpu: f32,
    /// Motor temperature normalized to [0.0, 1.0] (0 = 20 °C, 1 = 100 °C).
    pub motor_temp: f32,
    /// Pending log events to be consumed by the app loop.
    log_queue: Vec<String>,
}

impl SimState {
    pub fn new() -> Self {
        Self {
            t:          0,
            battery:    1.0,
            cpu:        0.2,
            motor_temp: 0.2,
            log_queue:  Vec::new(),
        }
    }

    /// Advance simulation by one tick.
    ///
    /// Contract: called exactly once per `SIM_TICK_MS` ms.  Push-side
    /// log entries are batched here; the caller drains them with
    /// `pop_log_event()` after every tick.
    pub fn tick(&mut self) {
        self.t += 1;

        // Battery: slow discharge 1.0 → 0.7 over 300 ticks (~2.5 min wall time).
        self.battery = (1.0 - self.t as f32 * 0.001).clamp(0.7, 1.0);

        // CPU: triangle wave 0.1–0.9 (no libm needed).
        let phase = (self.t % 20) as f32 / 20.0;
        self.cpu = if phase < 0.5 {
            0.1 + phase * 1.6
        } else {
            0.9 - (phase - 0.5) * 1.6
        };

        // Motor temperature: slow ramp 0.2 → 0.8.
        self.motor_temp = (0.2 + self.t as f32 * 0.003).clamp(0.2, 0.8);

        // Periodic log events.
        if self.t % 10 == 0 {
            self.log_queue.push(format!(
                "t={}s  Battery {:.0}%",
                self.t / 2,
                self.battery * 100.0,
            ));
        }
        if self.t % 7 == 0 {
            self.log_queue.push(format!(
                "t={}s  CPU {:.0}%",
                self.t / 2,
                self.cpu * 100.0,
            ));
        }
        if self.t % 15 == 0 {
            self.log_queue.push(format!(
                "t={}s  Motor {:.0}C",
                self.t / 2,
                20.0 + self.motor_temp * 80.0,
            ));
        }
    }

    /// Dequeue the oldest pending log line, or `None` if the queue is empty.
    pub fn pop_log_event(&mut self) -> Option<String> {
        if self.log_queue.is_empty() {
            None
        } else {
            Some(self.log_queue.remove(0))
        }
    }
}
