//! Foreground runtime witnesses for state-preserving hotswap.
//!
//! The demo provider is a supervised service (init autoload, packaging feature
//! `hotswap-demo`), not a shell background job, so each step runs to completion
//! in the foreground:
//!   `bench hotswap-prime [cached]` → `hotswap hotswap-demo /bin/hotswap-demo-v2`
//!   → `bench hotswap-verify N`.

use api::syscall::service;
use ostd::io::println;
use ostd::service::provider_tid;

use super::hotswap_supervisor::{expect_reply, read_counter, spawn_cached_sender_probe};

/// Drive the running v1 provider to counter 5. With `cached`, also spawn the
/// cached-sender probe that injects one increment during the cutover.
pub fn prime(cached: bool) {
    let Some(tid) = provider_tid(service::HOTSWAP_DEMO) else {
        fail("prime", "demo-v1 is not registered");
    };
    for _ in 0..5 {
        if !expect_reply(tid, b"inc", b"ok") {
            fail("prime", "demo-v1 increment failed");
        }
    }
    if read_counter(tid, b"v1:") != Some(5) {
        fail("prime", "demo-v1 counter is not 5");
    }
    if cached {
        if provider_tid(service::SUPERVISOR).is_none() {
            fail("prime", "supervisor is not registered");
        }
        if !spawn_cached_sender_probe(tid) {
            fail("prime", "cannot spawn cached-sender probe");
        }
    }
    println(if cached {
        "[hotswap-prime] ready (v1 counter=5, cached sender armed)"
    } else {
        "[hotswap-prime] ready (v1 counter=5)"
    });
    ostd::syscall::sys_exit(0);
}

/// After the swap: the registered provider is v2 and holds `expected`.
pub fn verify(expected: u32) {
    let Some(tid) = provider_tid(service::HOTSWAP_DEMO) else {
        fail("verify", "no provider registered after the swap");
    };
    match read_counter(tid, b"v2:") {
        Some(value) if value == expected => {
            println(&alloc::format!(
                "[hotswap-verify] PASS (v2 counter={value})"
            ));
            ostd::syscall::sys_exit(0);
        }
        Some(value) => fail(
            "verify",
            &alloc::format!("v2 counter={value}, expected {expected}"),
        ),
        None => fail("verify", "registered provider is not v2"),
    }
}

fn fail(step: &str, message: &str) -> ! {
    println(&alloc::format!("[hotswap-{step}] FAIL ({message})"));
    ostd::syscall::sys_exit(1)
}
