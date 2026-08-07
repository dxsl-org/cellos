//! Runtime witness that the `hotswap` CLI preserves demo state across cutover.

use api::syscall::service;
use ostd::io::println;
use ostd::syscall::sys_lookup_service;

use super::hotswap_supervisor::{
    expect_reply, read_counter, wait_for_hotswap_demo_replacement_ticks,
};

const WAIT_TICKS: usize = 500;

pub fn run() {
    let Some(old_tid) = sys_lookup_service(service::HOTSWAP_DEMO) else {
        fail("demo-v1 is not registered");
    };

    for _ in 0..5 {
        if !expect_reply(old_tid, b"inc", b"ok") {
            fail("demo-v1 increment failed");
        }
    }
    println("[hotswap-cli-probe] ready (v1 counter=5)");

    let Some(new_tid) = wait_for_hotswap_demo_replacement_ticks(old_tid, WAIT_TICKS) else {
        fail("replacement did not publish a new HOTSWAP_DEMO tid");
    };
    if read_counter(new_tid, b"v2:") != Some(5) {
        fail("replacement counter was not preserved");
    }
    println("[hotswap-cli-probe] PASS (v1 counter=5 -> v2 counter=5)");
    ostd::syscall::sys_exit(0);
}

fn fail(message: &str) -> ! {
    println(&alloc::format!("[hotswap-cli-probe] FAIL ({message})"));
    ostd::syscall::sys_exit(1)
}
