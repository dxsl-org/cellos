//! Benchmark scenarios — one module per measurement target.

pub mod context_switch;
pub mod control_loop;
pub mod ipc_send_recv;
pub mod memory_footprint;
pub mod preempt_latency;
pub mod rt_load;
pub mod smp;
pub mod syscall_yield;
