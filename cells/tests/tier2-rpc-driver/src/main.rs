//! Tier-1 driver of the Phase-02 cross-tier fixture.
//!
//! `init` spawns the Tier-2 provider first and hands this cell its tid through the
//! reviewed argv stash. The driver then runs one exchange in each direction and
//! four refusals, and prints one marker per leg so the lane asserts outcomes rather
//! than a summary line:
//!
//! | Leg | What it witnesses |
//! |---|---|
//! | 1 | Tier-1 → Tier-2: a typed, nontrivial copied request is delivered and answered |
//! | 2 | Tier-2 → Tier-1: the private-root provider called the named VFS service and said so in its reply |
//! | 3 | An oversize frame is refused by the kernel **before** delivery |
//! | 4 | A method the provider does not authorize is refused, with no side effect |
//! | 5 | The fixture's service id resolves to **no** binding (a private-root Cell cannot register) |
//! | 6 | A descriptor naming the provider is refused once it is gone |
//!
//! What it does not attempt: a wrong **user buffer** on the syscall copy path. A
//! `#![forbid(unsafe_code)]` Cell cannot fabricate a pointer, and the
//! address-containment witness for this class already runs as `/bin/tier2-exploit`.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::vec;
use api::ipc::IPC_BUF_SIZE;
use ostd::io::println;
use ostd::ipc::service_call_typed;
use ostd::syscall::{sys_exit, sys_lookup_service_bound, sys_send, SyscallResult};
use ostd::task::yield_now;
use tier2_rpc_proto::{
    checksum, EchoRequest, EchoResponse, METHOD_ECHO, METHOD_UNAUTHORIZED, SERVICE_ID, UNAUTHORIZED,
};

api::declare_manifest!(block_io = false, network = false, spawn = false);

// `StateRestore` is how a cell reads the argv its spawner stashed; `LookupService`
// also authorizes `LookupServiceBound` (allowlist bit 37).
api::declare_syscalls![Log, Exit, Send, Recv, Yield, LookupService, StateRestore];

ostd::cell_main!(cell_main);

/// Payload size for the echo leg: big enough that a truncated or substituted body
/// changes the checksum, small enough to leave the reply inside one frame.
const PAYLOAD_BYTES: usize = 512;

/// Yields allowed for the provider to exit before the stale-descriptor leg gives up.
const STALE_RETRIES: usize = 200;

fn fail(reason: &str) -> ! {
    println(&format!("[tier2-rpc] FAIL — {reason}"));
    sys_exit(1)
}

/// The provider tid `init` handed this cell.
fn provider_tid() -> usize {
    let args = ostd::args();
    match args
        .first()
        .and_then(|arg| arg.parse::<usize>().ok())
        .filter(|tid| *tid != 0)
    {
        Some(tid) => tid,
        None => fail("driver argument: provider tid"),
    }
}

fn cell_main() {
    let provider = provider_tid();
    println(&format!("[tier2-rpc] driver-start provider={provider}"));
    let mut send_buf = [0u8; IPC_BUF_SIZE];
    let mut recv_buf = [0u8; IPC_BUF_SIZE];

    // ── Leg 1 + 2: one typed exchange each way ────────────────────────────────
    let nonce = 0x5A17_0001u64;
    let payload = vec![0xA5u8; PAYLOAD_BYTES];
    let request = EchoRequest {
        method: METHOD_ECHO,
        nonce,
        payload: &payload,
    };
    let response = match service_call_typed::<EchoRequest, EchoResponse>(
        provider,
        &request,
        &mut send_buf,
        &mut recv_buf,
    ) {
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

    // ── Leg 3: an oversize frame, refused by the kernel before delivery ───────
    // The provider is still waiting for its second request here, so a frame that
    // were delivered would be observed as one.
    let oversize = vec![0u8; IPC_BUF_SIZE + 1];
    match sys_send(provider, &oversize) {
        SyscallResult::Err(_) => println("[tier2-rpc] OVERSIZE=REFUSED"),
        SyscallResult::Ok(bytes) => {
            println(&format!("[tier2-rpc] OVERSIZE=ACCEPTED bytes={bytes}"));
            fail("the kernel delivered an oversize frame across tiers")
        }
    }

    // ── Leg 4: a method the provider does not authorize ───────────────────────
    let denied = EchoRequest {
        method: METHOD_UNAUTHORIZED,
        nonce,
        payload: &[],
    };
    match service_call_typed::<EchoRequest, EchoResponse>(
        provider,
        &denied,
        &mut send_buf,
        &mut recv_buf,
    ) {
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

    // ── Leg 5: the fixture is not a registered service, and must not be ────────
    // A private-root Cell cannot register (`RegisterService` is `SpawnCap`-gated),
    // which is why this exchange is tid-addressed. Witness the consequence.
    let mut record = [0u8; api::service_binding::SERVICE_BINDING_LEN];
    match sys_lookup_service_bound(SERVICE_ID, &mut record) {
        None => println("[tier2-rpc] PROVIDER-REGISTRY=NONE"),
        Some(binding) => {
            println(&format!("[tier2-rpc] PROVIDER-REGISTRY={binding:?}"));
            fail("a private-root fixture appeared in the service registry")
        }
    }

    // ── Leg 6: the descriptor is refused once the provider is gone ────────────
    // The provider serves two requests and exits. A send may still be accepted
    // while it is on its way out — a message to a dying peer is not a stale
    // descriptor — so retry until the kernel refuses, and refuse to pass by
    // timeout: never getting an error would mean the tid kept accepting work.
    let mut refused = false;
    for _ in 0..STALE_RETRIES {
        if matches!(sys_send(provider, &[METHOD_ECHO]), SyscallResult::Err(_)) {
            refused = true;
            break;
        }
        yield_now();
    }
    if !refused {
        fail("a descriptor naming an exited provider kept accepting frames");
    }
    println("[tier2-rpc] STALE-PEER=REFUSED");

    println("[tier2-rpc] DRIVER-DONE");
    sys_exit(0)
}
