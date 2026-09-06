//! Benchmark scenarios — one module per measurement target.

#[allow(dead_code)]
pub mod base_tray_handoff;
#[allow(dead_code)]
pub mod c2c_broker_oracle;
#[allow(dead_code)]
pub mod c2c_broker_oracle_client;
pub mod c2c_broker_oracle_orchestrator;
pub mod c2c_broker_oracle_report;
#[allow(dead_code)]
pub mod c2c_broker_oracle_wire;
pub mod context_switch;
pub mod control_loop;
pub mod hotswap_cli_probe;
pub mod hotswap_supervisor;
pub mod ipc_fastpath;
pub mod ipc_send_recv;
pub mod lab_carrier_transfer;
#[allow(dead_code)]
pub mod lab_transfer_contract;
pub mod memory_footprint;
pub mod native_stateful;
pub mod preempt_latency;
pub mod rt_load;
pub mod smp;
pub mod snapshot_authority;
pub mod stationary_assembly;
pub mod syscall_yield;
pub mod vfs_getfile_breakdown;
