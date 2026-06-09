// SPDX-License-Identifier: MIT
//! Robot Dashboard — ViUI v2 demo Cell.
//!
//! Displays live simulated sensor data (battery, CPU, motor temperature) via
//! AnimatedSignal-driven ProgressBars, interactive sliders for speed/gain
//! control, start/stop buttons, and a scrolling event log ListView.
//!
//! Layout:
//! ```text
//! ┌──────────────┬──────────────────────────────┐
//! │ SENSORS      │  CONTROLS                    │
//! │ Battery XX%  │  Speed  [slider]  0.5        │
//! │ [ProgressBar]│  Gain   [slider]  0.3        │
//! │ CPU XX%      │  [  STOP  ]  [  START  ]     │
//! │ [ProgressBar]│                              │
//! │ Motor XXC    │                              │
//! │ [ProgressBar]│                              │
//! └──────────────┴──────────────────────────────┘
//! EVENT LOG
//! [ListView scrolling log]
//! ```
//!
//! ## Signal ownership contract
//!
//! `AnimatedSignal` is NOT Clone.  For each animated sensor:
//! 1. Call `.signal()` to clone the inner `Signal<f32>` while `AnimatedSignal`
//!    is still owned.
//! 2. Pass that `Signal<f32>` clone to widgets.
//! 3. Move the `AnimatedSignal` into `app.add_animation(Box::new(...))`.
//!
//! Derived text signals use `Signal::map(...).into_parts()`.
//! All `SubscriptionHandle`s from `into_parts()` are stored in `subs` so they
//! stay alive for the application lifetime.
//!
//! ## unsafe_code note
//!
//! `#[no_mangle]` on `main()` is required by the ViCell ELF loader and
//! triggers the `unsafe_attr` lint under `forbid(unsafe_code)`.  We therefore
//! do NOT use `forbid(unsafe_code)` at the crate level.  All logic in this
//! file and in `sim.rs` is genuinely unsafe-free — this mirrors the pattern
//! used in `cells/apps/bench` and `cells/apps/periph-demo`.

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

use alloc::{boxed::Box, string::String, vec, vec::Vec};

use api::display::PixelFormat;

use viui::{
    animation::AnimatedSignal,
    app_runner::ViApp,
    node_widgets::{
        button::Button,
        column::Column,
        label::Label,
        list_view::ListView,
        progress_bar::ProgressBar,
        row::Row,
        slider::Slider,
    },
    node::ViNode,
    renderer::FramebufferRenderer,
    signal::{Signal, SubscriptionHandle},
};

use ostd::{
    display::{wait_for_compositor, ViSurface},
    syscall::{sys_get_time, sys_heartbeat, sys_yield},
    MTIME_TICKS_PER_MS,
};

mod sim;
use sim::{SimState, SIM_TICK_MS};

const DISPLAY_W: u32 = 800;
const DISPLAY_H: u32 = 480;

// ─── Entry ────────────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn main() {
    ostd::io::println("[robot-dashboard] starting");

    // ── Surface + renderer ────────────────────────────────────────────────────
    let comp_tid = wait_for_compositor();
    let surf = match ViSurface::create(comp_tid, DISPLAY_W, DISPLAY_H, PixelFormat::Bgra8888) {
        Ok(s) => s,
        Err(_) => {
            ostd::io::println("[robot-dashboard] ERROR: could not create surface");
            ostd::syscall::sys_exit(1);
        }
    };
    let renderer = FramebufferRenderer::new(surf);

    // Register this cell to receive keyboard and mouse events.
    viui::input_bridge::request_input_focus();

    // ── Subscription handle storage (keeps derived signals alive) ─────────────
    let mut subs: Vec<SubscriptionHandle> = Vec::new();

    // ── Animated sensor signals ───────────────────────────────────────────────
    // AnimatedSignal is NOT Clone: extract inner Signal before moving into app.
    let battery_anim    = AnimatedSignal::new(1.0_f32);
    let cpu_anim        = AnimatedSignal::new(0.2_f32);
    let motor_anim      = AnimatedSignal::new(0.2_f32);

    let battery_f    = battery_anim.signal();   // Signal<f32> clone, shared with widgets
    let cpu_f        = cpu_anim.signal();
    let motor_f      = motor_anim.signal();

    // Keep a second clone of each sensor signal to write updates in the loop.
    // (The AnimatedSignal owns the primary; we bypass its tween for direct sets.)
    let battery_w = battery_f.clone();
    let cpu_w     = cpu_f.clone();
    let motor_w   = motor_f.clone();

    // ── Derived text labels ───────────────────────────────────────────────────
    let (battery_text, s1) = battery_f
        .map(|v| alloc::format!("Battery {:.0}%", v * 100.0))
        .into_parts();
    let (cpu_text, s2) = cpu_f
        .map(|v| alloc::format!("CPU {:.0}%", v * 100.0))
        .into_parts();
    let (motor_text, s3) = motor_f
        .map(|v| alloc::format!("Motor {:.0}C", 20.0 + v * 80.0))
        .into_parts();
    subs.push(s1);
    subs.push(s2);
    subs.push(s3);

    // ── Control signals ───────────────────────────────────────────────────────
    let speed_sig   = Signal::new(0.5_f32);
    let gain_sig    = Signal::new(0.3_f32);
    let running_sig = Signal::new(true);
    let log_sig: Signal<Vec<String>> = Signal::new(Vec::new());

    // Clones for button callbacks and loop writes.
    let running_stop  = running_sig.clone();
    let running_start = running_sig.clone();
    // running_sig itself is moved into the status_text map below; no separate write clone needed.
    let log_w         = log_sig.clone();

    // Status label: "RUNNING" / "STOPPED"
    let running_r = running_sig.clone();
    let (status_text, s4) = running_r
        .map(|r| if *r { String::from("RUNNING") } else { String::from("STOPPED") })
        .into_parts();
    subs.push(s4);

    // ── Speed/gain labels (show current slider value) ─────────────────────────
    let speed_for_lbl = speed_sig.clone();
    let (speed_text, s5) = speed_for_lbl
        .map(|v| alloc::format!("{:.2}", v))
        .into_parts();
    let gain_for_lbl = gain_sig.clone();
    let (gain_text, s6) = gain_for_lbl
        .map(|v| alloc::format!("{:.2}", v))
        .into_parts();
    subs.push(s5);
    subs.push(s6);

    // ── Build widget tree ─────────────────────────────────────────────────────
    let root = build_layout(
        battery_f,
        battery_text,
        cpu_f,
        cpu_text,
        motor_f,
        motor_text,
        speed_sig,
        speed_text,
        gain_sig,
        gain_text,
        status_text,
        log_sig,
        running_stop,
        running_start,
    );

    // ── Assemble ViApp ────────────────────────────────────────────────────────
    let mut app = ViApp::new(Box::new(root), Box::new(renderer));
    app.add_animation(Box::new(battery_anim));
    app.add_animation(Box::new(cpu_anim));
    app.add_animation(Box::new(motor_anim));

    // ── Main loop ─────────────────────────────────────────────────────────────
    let mut sim        = SimState::new();
    let mut last_sim   = sys_get_time();
    let mut last_frame = last_sim;
    let tick_interval  = SIM_TICK_MS * MTIME_TICKS_PER_MS;

    // Render at ~30 fps (33 ms between frames).
    const FRAME_INTERVAL: u64 = 33 * MTIME_TICKS_PER_MS;

    loop {
        // Heartbeat: disable hung-detector (0 = no deadline).
        sys_heartbeat(0);

        let now = sys_get_time();

        // ── Sim tick ─────────────────────────────────────────────────────────
        if now.wrapping_sub(last_sim) >= tick_interval {
            last_sim = now;
            sim.tick();

            // Push sensor values directly (bypass animation tween for demo clarity).
            battery_w.set(sim.battery);
            cpu_w.set(sim.cpu);
            motor_w.set(sim.motor_temp);

            // Drain log events.
            while let Some(line) = sim.pop_log_event() {
                log_w.update(|v| {
                    v.push(line.clone());
                    // Keep the log bounded to 200 entries.
                    if v.len() > 200 {
                        v.remove(0);
                    }
                });
            }
        }

        // ── Input collection ─────────────────────────────────────────────────
        let events = viui::input_bridge::collect_input_events(32);

        // ── Render tick ──────────────────────────────────────────────────────
        if now.wrapping_sub(last_frame) >= FRAME_INTERVAL {
            let dt_ms = (now.wrapping_sub(last_frame) / MTIME_TICKS_PER_MS) as u32;
            last_frame = now;
            app.tick_with_dt(&events, dt_ms);
        }

        // Yield to avoid busy-spinning the CPU.
        sys_yield();
    }
}

// ─── Layout builder ───────────────────────────────────────────────────────────
//
// Separated from main() to keep each function under 200 lines.
// All borrowed signals have already been cloned before this call.
#[allow(clippy::too_many_arguments)]
fn build_layout(
    battery_f:    Signal<f32>,
    battery_text: Signal<String>,
    cpu_f:        Signal<f32>,
    cpu_text:     Signal<String>,
    motor_f:      Signal<f32>,
    motor_text:   Signal<String>,
    speed_sig:    Signal<f32>,
    speed_text:   Signal<String>,
    gain_sig:     Signal<f32>,
    gain_text:    Signal<String>,
    status_text:  Signal<String>,
    log_sig:      Signal<Vec<String>>,
    running_stop:  Signal<bool>,
    running_start: Signal<bool>,
) -> impl ViNode {
    // ── Sensors column ────────────────────────────────────────────────────────
    let sensors = Column::new(vec![
        Box::new(Label::new(Signal::new(String::from("SENSORS")))) as Box<dyn ViNode>,
        Box::new(Label::new(battery_text)),
        Box::new(ProgressBar::new(battery_f).with_label()),
        Box::new(Label::new(cpu_text)),
        Box::new(ProgressBar::new(cpu_f).with_label()),
        Box::new(Label::new(motor_text)),
        Box::new(ProgressBar::new(motor_f).with_label()),
    ])
    .with_spacing(6.0)
    .with_padding(8.0);

    // ── Controls column ───────────────────────────────────────────────────────
    let speed_row = Row::new(vec![
        Box::new(Label::new(Signal::new(String::from("Speed")))) as Box<dyn ViNode>,
        Box::new(Slider::new(speed_sig)),
        Box::new(Label::new(speed_text)),
    ])
    .with_spacing(8.0);

    let gain_row = Row::new(vec![
        Box::new(Label::new(Signal::new(String::from("Gain")))) as Box<dyn ViNode>,
        Box::new(Slider::new(gain_sig)),
        Box::new(Label::new(gain_text)),
    ])
    .with_spacing(8.0);

    let btn_stop  = Button::new("STOP",  move || { running_stop.set(false); });
    let btn_start = Button::new("START", move || { running_start.set(true); });
    let btn_row = Row::new(vec![
        Box::new(btn_stop)  as Box<dyn ViNode>,
        Box::new(btn_start),
    ])
    .with_spacing(12.0);

    let controls = Column::new(vec![
        Box::new(Label::new(Signal::new(String::from("CONTROLS")))) as Box<dyn ViNode>,
        Box::new(speed_row),
        Box::new(gain_row),
        Box::new(btn_row),
        Box::new(Label::new(status_text)),
    ])
    .with_spacing(10.0)
    .with_padding(8.0);

    // ── Top row: sensors | controls ───────────────────────────────────────────
    let top_row = Row::new(vec![
        Box::new(sensors)  as Box<dyn ViNode>,
        Box::new(controls),
    ])
    .with_spacing(16.0)
    .with_padding(8.0);

    // ── Event log section ─────────────────────────────────────────────────────
    let log_section = Column::new(vec![
        Box::new(Label::new(Signal::new(String::from("EVENT LOG")))) as Box<dyn ViNode>,
        Box::new(ListView::new(log_sig).item_height(22.0)),
    ])
    .with_spacing(4.0)
    .with_padding(8.0);

    // ── Root column ───────────────────────────────────────────────────────────
    Column::new(vec![
        Box::new(top_row)     as Box<dyn ViNode>,
        Box::new(log_section),
    ])
    .with_spacing(8.0)
}
