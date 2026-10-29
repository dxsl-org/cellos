use alloc::format;
use text_engine::top::sample::Frame;
use text_engine::top::{text, TopOptions};

/// Write one batch frame: a status line (sample time on the monotonic clock, frames
/// left) and the shared text table, so `top -b` prints the same numbers as
/// `top --records` and the interactive view.
pub fn render_frame(frame: &Frame, options: &TopOptions, remaining: usize) {
    let seconds = frame.meta.capture_start_ns / 1_000_000_000;
    let mut out = format!("top: t={}s  batch  remaining {}\n", seconds, remaining);
    text::render_frame(frame, options.fields.as_deref(), &mut out);
    crate::executor::shell_print(&out);
}
