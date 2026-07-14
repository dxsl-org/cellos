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
api::declare_manifest!(block_io = false, network = true, spawn = false);

// Declare cluster membership — broker participates in the "robots" private cluster.
// Hardcoded demo cluster name; P09 enrollment will replace this with a config read.
api::declare_cluster!(mode = Private, name = "robots");

// Allow-list narrow enough for the broker dispatch loop; expanded in P04/P05.
// RegisterService is NOT listed — init registers service::NET_BROKER on the
// broker's behalf (init has SpawnCap; the broker does not).
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    GetTime,
    GetRandom,
    WaitForEvent,
];

mod connection_manager;
mod identity;
mod relay;
mod rng;
mod stun;
mod transport;

mod beacon;
mod enrollment;
mod gossip;
mod lease;
mod routing;

use identity::BrokerIdentity;
use ostd::io::{print, println};
use ostd::syscall::{sys_heartbeat, sys_try_recv, SyscallResult};
use relay::RelayClient;
use rng::BrokerRng;
use transport::StaticKeypair;

fn print_hex_byte(b: u8) {
    const HEX: &[u8] = b"0123456789abcdef";
    let hi = HEX[(b >> 4) as usize] as char;
    let lo = HEX[(b & 0xf) as usize] as char;
    let mut buf = [0u8; 2];
    buf[0] = hi as u8;
    buf[1] = lo as u8;
    if let Ok(s) = core::str::from_utf8(&buf) {
        print(s);
    }
}

/// IPC buffer for incoming dispatch requests.
const IPC_BUF_SIZE: usize = api::ipc::IPC_BUF_SIZE;

/// Heartbeat interval in milliseconds — re-armed every dispatch-loop iteration.
const HEARTBEAT_MS: u64 = 500;

#[no_mangle]
pub fn main() {
    println("[net-broker] Cluster net-broker v0.2 (G1 internet relay)");
    println("[net-broker] service::NET_BROKER = 8 (registered by init)");

    // Fail-closed entropy gate — panics if VirtIO-RNG is absent.
    let mut rng = BrokerRng::new_seeded();
    println("[net-broker] VirtIO-RNG entropy gate passed.");

    // Generate per-run X25519 static keypair. Public half = G1 CellNetId.
    let static_kp = StaticKeypair::generate(&mut rng);
    let mut identity = BrokerIdentity::from_static_pub(static_kp.public_bytes());
    identity.load_config();

    // Log the first 4 bytes of NodeId as a boot identifier.
    let nid = identity.node_id.0;
    print("[net-broker] NodeId prefix: ");
    for b in &nid[..4] {
        print_hex_byte(*b);
    }
    println("...");

    // TODO: load K1 PSK from /etc/cellos/cluster.key via VfsFileKeySource.
    // TODO P05: bind UDP multicast socket for LAN beacon.

    // Build a relay client from the first peer's relay config (G1 = 1 relay server).
    let relay_config = identity.get_peer(0).map(|p| (p.relay_ip, p.relay_port));
    let relay_ip = relay_config.map(|(ip, _)| ip).unwrap_or([0; 4]);
    let relay_port = relay_config.map(|(_, pt)| pt).unwrap_or(0);
    let relay_client = RelayClient::new(identity.node_id, relay_ip, relay_port);

    let mut buf = [0u8; IPC_BUF_SIZE];

    loop {
        // INVARIANT: heartbeat re-armed at top of every iteration.
        sys_heartbeat(HEARTBEAT_MS);

        // Poll relay for inbound frames (non-blocking).
        // TODO: wire incoming relay frames into routing dispatch.
        let _ = relay_client.is_connected();

        // TODO P05: check beacon timer; send LAN multicast beacon if due.
        // TODO P05: try_recv beacon UDP socket.
        // TODO P08: tick lease renewal / peer-loss sweep.

        buf.fill(0);
        match sys_try_recv(0, &mut buf) {
            SyscallResult::Ok(sender) if sender > 0 => {
                dispatch(&buf, sender);
            }
            _ => {
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
