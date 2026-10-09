//! Tier-1 driver of the Phase-02 cross-tier fixture.
//!
//! `init` spawns the Tier-2 provider and **registers** it under the fixture's service
//! id: a private-root Cell cannot register itself (`RegisterService` is `SpawnCap`-gated
//! and its ceiling is empty), but its spawner can, which is the same pattern init uses
//! for the hypervisor. The driver therefore resolves the **named** service, not a tid it
//! was handed, and every leg goes through the shipped SDK.
//!
//! | Leg | What it witnesses |
//! |---|---|
//! | 1 | The named service resolves to a live binding with the provider's real `(cell_id, generation)`, and the SDK agrees with the raw opcode |
//! | 2 | Tier-1 → Tier-2: a typed, nontrivial copied request is delivered and answered |
//! | 3 | Tier-2 → Tier-1: the private-root provider called the named VFS service and said so in its reply |
//! | 4 | An oversize frame is refused by the kernel **before** delivery |
//! | 5 | A method the provider does not authorize is refused, with no side effect |
//! | 6 | Once the provider is gone, the descriptor the handle holds is **refused**, not re-targeted |
//!
//! Not attempted: a wrong **user buffer** on the syscall copy path. A
//! `#![forbid(unsafe_code)]` Cell cannot fabricate a pointer, and the
//! address-containment witness for that class already runs as `/bin/tier2-exploit`.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::vec;
use api::ipc::IPC_BUF_SIZE;
use api::service_binding::{ServiceBinding, SERVICE_BINDING_LEN};
use ostd::io::println;
use ostd::service::ServiceRef;
use ostd::syscall::{sys_exit, sys_lookup_service_bound, sys_send, SyscallResult};
use ostd::task::yield_now;
use ostd::ViError;
use tier2_rpc_proto::{
    checksum, EchoRequest, EchoResponse, METHOD_ECHO, METHOD_UNAUTHORIZED, SERVICE_ID, UNAUTHORIZED,
};

api::declare_manifest!(block_io = false, network = false, spawn = false);

// `LookupService` also authorizes `LookupServiceBound` (allowlist bit 37), which is the
// whole resolution path this driver needs.
api::declare_syscalls![Log, Exit, Send, Recv, Yield, LookupService];

ostd::cell_main!(cell_main);

/// Payload size for the echo leg: big enough that a truncated or substituted body
/// changes the checksum, small enough to leave the reply inside one frame.
const PAYLOAD_BYTES: usize = 512;

/// Yields allowed for the provider to exit before the stale-descriptor leg gives up.
const STALE_POLLS: usize = 2000;

fn fail(reason: &str) -> ! {
    println(&format!("[tier2-rpc] FAIL — {reason}"));
    sys_exit(1)
}

/// The kernel's current binding for the fixture's service, or `None` when nothing live
/// is registered under it. Reads the registry directly, without touching the SDK's cache.
fn live_binding() -> Option<ServiceBinding> {
    let mut record = [0u8; SERVICE_BINDING_LEN];
    sys_lookup_service_bound(SERVICE_ID, &mut record)
}

fn cell_main() {
    // ── Leg 1: the named service resolves, with the provider's real identity ──
    let mut provider: ServiceRef<{ SERVICE_ID }> = ServiceRef::new();
    let binding = match provider.binding() {
        Some(binding) => binding,
        None => fail("the fixture's service is not resolvable by name"),
    };
    println(&format!(
        "[tier2-rpc] PROVIDER-BINDING tid={} cell={} gen={}",
        binding.tid, binding.cell_id, binding.generation
    ));
    if binding.cell_id == 0 || binding.generation == 0 {
        fail("the registered binding carries no live Cell identity");
    }
    if live_binding() != Some(binding) {
        fail("the SDK's named resolution disagreed with the raw bound lookup");
    }
    println("[tier2-rpc] PROVIDER-NAME-MATCHES-RAW=true");

    let mut response_buffer = [0u8; IPC_BUF_SIZE];

    // ── Leg 2 + 3: one typed exchange each way, through the SDK ──────────────
    let nonce = 0x5A17_0002u64;
    let payload = vec![0xA5u8; PAYLOAD_BYTES];
    let request = EchoRequest {
        method: METHOD_ECHO,
        nonce,
        payload: &payload,
    };
    let response = match provider.call::<EchoRequest, EchoResponse>(&request, &mut response_buffer)
    {
        Ok(response) => response,
        Err(error) => {
            println(&format!("[tier2-rpc] EXCHANGE-ERROR {error:?}"));
            fail("typed cross-tier call did not complete")
        }
    };
    if response.nonce != nonce
        || response.payload_len as usize != PAYLOAD_BYTES
        || response.payload_checksum != checksum(&payload)
    {
        println(&format!("[tier2-rpc] ECHO-MISMATCH {response:?}"));
        fail("the Tier-2 provider echoed a different body");
    }
    println("[tier2-rpc] TIER1-TO-TIER2=OK");
    if response.vfs_stat_ok {
        println(&format!(
            "[tier2-rpc] TIER2-TO-TIER1=OK root_is_dir={}",
            response.vfs_root_is_dir
        ));
    } else {
        fail("the private-root provider could not call the named VFS service");
    }

    // ── Leg 4: an oversize frame, refused by the kernel before delivery ───────
    // The provider is still waiting for its second request here, so a frame that were
    // delivered would be observed as one.
    let oversize = vec![0u8; IPC_BUF_SIZE + 1];
    match sys_send(binding.tid as usize, &oversize) {
        SyscallResult::Err(_) => println("[tier2-rpc] OVERSIZE=REFUSED"),
        SyscallResult::Ok(bytes) => {
            println(&format!("[tier2-rpc] OVERSIZE=ACCEPTED bytes={bytes}"));
            fail("the kernel delivered an oversize frame across tiers")
        }
    }

    // ── Leg 5: a method the provider does not authorize ──────────────────────
    let denied = EchoRequest {
        method: METHOD_UNAUTHORIZED,
        nonce,
        payload: &[],
    };
    match provider.call::<EchoRequest, EchoResponse>(&denied, &mut response_buffer) {
        Ok(response) if response.error == Some(UNAUTHORIZED) => {
            println("[tier2-rpc] UNAUTHORIZED-METHOD=REFUSED")
        }
        Ok(other) => {
            println(&format!(
                "[tier2-rpc] UNAUTHORIZED-METHOD=ACCEPTED {other:?}"
            ));
            fail("an unauthorized method was served")
        }
        Err(error) => {
            println(&format!("[tier2-rpc] UNAUTHORIZED-METHOD=ERROR {error:?}"));
            fail("the unauthorized-method leg did not get a typed refusal")
        }
    }

    // ── Leg 6: the descriptor the handle holds is refused, not re-targeted ───
    // The provider served its budget and exits. Watch the registry through the raw
    // opcode — deliberately not through the SDK, whose cache this leg is about — until
    // no live binding remains, then call. The call must fail: the SDK must not send to
    // the retired tid, and must not silently resolve onto some other incarnation.
    let mut gone = false;
    for _ in 0..STALE_POLLS {
        if live_binding().is_none() {
            gone = true;
            break;
        }
        yield_now();
    }
    if !gone {
        fail("the fixture's registry entry stayed live after the provider exited");
    }
    println("[tier2-rpc] PROVIDER-GONE=none");
    match provider.call::<EchoRequest, EchoResponse>(&denied, &mut response_buffer) {
        Err(ViError::NotFound) => println("[tier2-rpc] STALE-BINDING=REFUSED"),
        Ok(response) => {
            println(&format!("[tier2-rpc] STALE-BINDING=SERVED {response:?}"));
            fail("a call under a stale descriptor was served")
        }
        Err(other) => {
            println(&format!("[tier2-rpc] STALE-BINDING=ERROR {other:?}"));
            fail("a stale descriptor did not report the refusal the contract names")
        }
    }
    if provider.is_live() {
        fail("the handle still calls its descriptor live after the refusal");
    }
    println("[tier2-rpc] STALE-BINDING-CLEARED=true");

    println("[tier2-rpc] DRIVER-DONE");
    sys_exit(0)
}
