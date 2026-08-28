//! WASM host cell — runs WebAssembly (.wasm) binaries as Tier 2 cells.
//!
//! Usage: spawn `/bin/wasm` with the `.wasm` file path as argv.
//! The shell sets this via `sys_set_spawn_args("/data/apps/app.wasm")`.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate alloc;

mod loader;

use driver_wasm::imports::register_vi_imports;
use driver_wasm::{HostState, WasmConfig, WasmRuntime};
use loader::load_wasm_bytes;

api::declare_syscalls![Send, Recv, Log, Heartbeat, LookupService, StateRestore];

ostd::cell_main!(cell_main);

fn cell_main() {
    let argv = ostd::args();
    let path = argv
        .first()
        .map(|arg| arg.as_str())
        .unwrap_or("/data/apps/app.wasm");

    let wasm_bytes = match load_wasm_bytes(path) {
        Ok(bytes) => bytes,
        Err(_) => {
            ostd::io::print("[wasm] error: could not read ");
            ostd::io::println(path);
            ostd::syscall::sys_exit(1);
        }
    };

    let config = WasmConfig::default();
    let runtime = WasmRuntime::new(&config);

    let module = match runtime.load_module(&wasm_bytes) {
        Ok(m) => m,
        Err(_) => {
            ostd::io::print("[wasm] error: invalid WASM module at ");
            ostd::io::println(path);
            ostd::syscall::sys_exit(1);
        }
    };

    let mut store = runtime.new_store(&config, HostState { cell_task_id: 0 });
    let mut linker = runtime.new_linker();
    register_vi_imports(&mut linker);

    let instance = match linker.instantiate_and_start(&mut store, &module) {
        Ok(i) => i,
        Err(_) => {
            ostd::io::print("[wasm] error: instantiation failed\n");
            ostd::syscall::sys_exit(1);
        }
    };

    let run_fn = match instance.get_typed_func::<(), ()>(&store, "run") {
        Ok(f) => f,
        Err(_) => {
            ostd::io::print("[wasm] error: module must export 'run: () -> ()'\n");
            ostd::syscall::sys_exit(1);
        }
    };

    loop {
        match run_fn.call(&mut store, ()) {
            Ok(()) => break,
            Err(_) => {
                store.set_fuel(config.fuel_per_tick).ok();
                ostd::task::yield_now();
            }
        }
    }

    ostd::syscall::sys_exit(0);
}
