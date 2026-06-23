#![no_std]
#![no_main]

//! **net-broker** — Cluster net-broker Cell (service::NET_BROKER = 8).
//!
//! The single userspace trust anchor for all cross-machine communication:
//! * P04 — Noise KKpsk0 point-to-point transport (clatter crate)
//! * P05 — XChaCha20-Poly1305 SwarmBeacon UDP multicast discovery
//! * P06 — RemoteServiceProxy routing matrix
//! * P08 — Task-claiming gossip + lease lifecycle
//! * P09 — Runtime enrollment / merge-split
//!
//! ## Design invariants (from plan.md + docs/specs/14-distributed.md)
//! 1. Broker runs at **NORMAL** priority (init spawns without SpawnPinned).
//! 2. Re-arms `sys_heartbeat(500)` at the TOP of every dispatch-loop iteration,
//!    including inside any blocking/spin-poll sub-loop (P04 Noise handshake).
//! 3. Init registers service::NET_BROKER on the broker's behalf; the broker does
//!    not self-register (it lacks SpawnCap).
//! 4. No blocking recv at Init — sockets can be set up, but the first RECV must
//!    happen inside the dispatch loop.
//! 5. P04/P05 crypto transport panics at Init if VirtIO-RNG is absent (fail-closed
//!    entropy gate, mirroring `ViRng::new()` in the net cell TLS module).
//!
//! ## Hot-swap limitation (OQ5 — unresolved)
//! In-flight IPC and Noise session re-establish on hot-swap are undefined.
//! The broker does NOT implement `ViStateTransfer`. A respawn means all cluster
//! sessions are torn down and must be re-negotiated by peers.

extern crate alloc;

// NetworkCap (TCP/UDP) + VFS read cap (for /etc/cellos/cluster.key in P04).
// spawn = false: the broker routes IPC, it never spawns cells.
api::declare_manifest!(
    block_io = false,
    network  = true,
    spawn    = false
);

// Declare cluster membership — broker participates in the "robots" private cluster.
// Hardcoded demo cluster name; P09 enrollment will replace this with a config read.
api::declare_cluster!(mode = Private, name = "robots");

// Allow-list narrow enough for the broker dispatch loop; expanded in P04/P05.
// RegisterService is NOT listed — init registers service::NET_BROKER on the
// broker's behalf (init has SpawnCap; the broker does not).
api::declare_syscalls![
    Send, Recv, TryRecv, Reply, Log, Heartbeat,
    LookupService, GetTime, GetRandom,
    WaitForEvent,
];

mod rng;
mod transport;

mod beacon;
mod enrollment;
mod gossip;
mod lease;
mod routing;

use ostd::io::println;
use ostd::syscall::{sys_heartbeat, sys_try_recv, SyscallResult};
use rng::BrokerRng;
use transport::StaticKeypair;

/// IPC buffer for incoming dispatch requests.
const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;

/// Heartbeat interval in milliseconds — re-armed every dispatch-loop iteration.
const HEARTBEAT_MS: u64 = 500;

#[no_mangle]
pub fn main() {
    println("[net-broker] Cluster net-broker v0.1 (P03 skeleton)");
    println("[net-broker] service::NET_BROKER = 8 (registered by init)");

    // Fail-closed entropy gate — panics if VirtIO-RNG device is absent.
    // This MUST run before any crypto operation; mirrors ViRng::new() in the net cell.
    let mut rng = BrokerRng::new_seeded();
    println("[net-broker] VirtIO-RNG entropy gate passed.");

    // Generate a per-run X25519 static keypair; broadcast public half via P05 beacon.
    let _static_kp = StaticKeypair::generate(&mut rng);
    println("[net-broker] static keypair ready (public key broadcast via beacon in P05).");

    // TODO P04: load K1 PSK from /etc/cellos/cluster.key via VfsFileKeySource.
    // TODO P05: bind UDP multicast socket + join beacon group (non-blocking).
    // TODO P05: first beacon RECV is in the loop below, NOT here.

    let mut buf = [0u8; IPC_BUF_SIZE];

    loop {
        // INVARIANT: heartbeat re-armed at the top of every iteration, including
        // iterations that go deep into P04 handshake spin-polls.  If this call
        // moves further down, the RT watchdog will kill the broker mid-handshake.
        sys_heartbeat(HEARTBEAT_MS);

        // TODO P05: check beacon send timer; multicast beacon if interval elapsed.
        // TODO P05: try_recv beacon UDP socket and process.
        // TODO P04: poll Noise handshake progress for any pending peer sessions.
        // TODO P08: tick lease renewal / peer-loss sweep.

        buf.fill(0);
        match sys_try_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                dispatch(&buf, sender);
            }
            _ => {
                // No IPC message ready. Yield briefly to avoid burning the CPU
                // in a hot spin-loop; the scheduler will reschedule as needed.
                // TODO P04/P05: replace yield with sys_wait_for_event (NIC-RX)
                // once transport sockets are active, mirroring the net cell.
                ostd::task::yield_now();
            }
        }
    }
}

/// Dispatch an incoming IPC message. Extended per phase.
fn dispatch(_buf: &[u8], _sender: usize) {
    // TODO P06: route RemoteServiceProxy calls via routing matrix.
    // TODO P08: handle lease request / renew / release.
    // TODO P09: handle enrollment / merge-split messages.
}
