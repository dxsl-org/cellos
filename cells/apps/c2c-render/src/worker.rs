#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Log, Recv, IpcCurrent, IpcReply, GetTime, GrantSlice];

ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(2 * 1024 * 1024);

fn cell_main() {
    app_c2c_render::worker_runtime::run::<true>()
}
