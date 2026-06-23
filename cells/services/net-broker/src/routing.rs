/// RemoteServiceProxy — broker-side routing table + cluster-RPC dispatch.
///
/// ## Protocol (matches libs/ostd/src/cluster.rs byte layout)
///
/// Incoming request (from a local cell via IPC):
///   [0] opcode: 0x01 = LookupRemote(service_id)
///   [1..3] service_id (u16 LE)
///
/// Broker reply:
///   [0] 0x00 = Found; [1..9] proxy_tid (u64 LE, = this broker's own TID)
///   [0] 0x01 = NotFound
///   [0] 0x02 = Err
///
/// When a cell routes a service call via the broker TID returned above, the broker
/// deserializes the outer P08 gossip envelope (phase), extracts (target_service_id,
/// payload), looks up the Noise session for the owning peer, encrypts and forwards.
/// The response travels the reverse path.
///
/// ## Routing table design
///
/// A `ServiceRoute` records: which peer machine owns a service_id locally.
/// The broker learns routes via P08 gossip announcements. Routes expire when
/// the peer's beacon times out (see beacon::PeerTable::timed_out_count).
///
/// P06 ships the data structures and LookupRemote dispatch; P08 wires gossip
/// announcements into insert_route / remove_routes_for_peer.
#[allow(dead_code)] // Wired into dispatch loop in P08

use ostd::syscall::sys_lookup_service;
use api::syscall::service;

// ── Opcodes ────────────────────────────────────────────────────────────────────

const OP_LOOKUP_REMOTE: u8 = 0x01;

const RESP_FOUND:     u8 = 0x00;
const RESP_NOT_FOUND: u8 = 0x01;
const RESP_ERR:       u8 = 0x02;

// ── RoutingTable ───────────────────────────────────────────────────────────────

const MAX_ROUTES: usize = 32;

/// One known remote service binding.
#[derive(Clone, Copy)]
pub struct ServiceRoute {
    pub service_id: u16,
    pub machine_id: u64,
}

/// Routing table — maps service_id → machine_id for remote services.
///
/// Only stores routes for services NOT present locally (local lookups bypass
/// the broker entirely via init's RegisterService).
pub struct RoutingTable {
    routes: [Option<ServiceRoute>; MAX_ROUTES],
}

impl RoutingTable {
    pub const fn new() -> Self {
        Self { routes: [None; MAX_ROUTES] }
    }

    /// Add or overwrite a route for `(service_id, machine_id)`.
    pub fn insert_route(&mut self, service_id: u16, machine_id: u64) {
        // Update existing entry for same service_id if present.
        for r in self.routes.iter_mut().flatten() {
            if r.service_id == service_id {
                r.machine_id = machine_id;
                return;
            }
        }
        // Insert in first free slot.
        for slot in self.routes.iter_mut() {
            if slot.is_none() {
                *slot = Some(ServiceRoute { service_id, machine_id });
                return;
            }
        }
        // Table full — evict oldest by scanning from the start (simple FIFO).
        self.routes[0] = Some(ServiceRoute { service_id, machine_id });
    }

    /// Remove all routes belonging to a peer that has timed out.
    pub fn remove_routes_for_peer(&mut self, machine_id: u64) {
        for slot in self.routes.iter_mut() {
            if slot.map(|r| r.machine_id == machine_id).unwrap_or(false) {
                *slot = None;
            }
        }
    }

    /// Find which machine provides `service_id`. Returns `None` if unknown.
    pub fn lookup(&self, service_id: u16) -> Option<u64> {
        self.routes.iter().flatten()
            .find(|r| r.service_id == service_id)
            .map(|r| r.machine_id)
    }

    pub fn route_count(&self) -> usize {
        self.routes.iter().filter(|s| s.is_some()).count()
    }
}

// ── RemoteServiceProxy ─────────────────────────────────────────────────────────

/// Handles incoming IPC from local cells wanting cluster-scoped service lookup.
pub struct RemoteServiceProxy {
    table: RoutingTable,
    /// Cached self TID — returned as proxy_tid in LookupRemote responses.
    self_tid: usize,
}

impl RemoteServiceProxy {
    pub fn new() -> Self {
        // Init registers NET_BROKER before the dispatch loop starts; this lookup
        // returns our own TID (the broker is the only provider of this service).
        let self_tid = sys_lookup_service(service::NET_BROKER).unwrap_or(0);
        Self {
            table: RoutingTable::new(),
            self_tid,
        }
    }

    /// Process one incoming IPC byte slice.
    /// Returns up to 9 bytes to write back as the IPC reply.
    pub fn handle(&self, buf: &[u8], out: &mut [u8; 9]) -> usize {
        if buf.is_empty() {
            out[0] = RESP_ERR;
            return 1;
        }
        match buf[0] {
            OP_LOOKUP_REMOTE => {
                if buf.len() < 3 {
                    out[0] = RESP_ERR;
                    return 1;
                }
                let svc = u16::from_le_bytes([buf[1], buf[2]]);
                match self.table.lookup(svc) {
                    Some(_machine_id) => {
                        // Return our own TID as the proxy. Caller will route
                        // subsequent calls to us; we forward via Noise (P08).
                        out[0] = RESP_FOUND;
                        out[1..9].copy_from_slice(&(self.self_tid as u64).to_le_bytes());
                        9
                    }
                    None => {
                        out[0] = RESP_NOT_FOUND;
                        1
                    }
                }
            }
            _ => {
                out[0] = RESP_ERR;
                1
            }
        }
    }

    /// Expose table for P08 gossip integration.
    pub fn table_mut(&mut self) -> &mut RoutingTable {
        &mut self.table
    }
}
