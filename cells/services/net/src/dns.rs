//! Service-level DNS resolution.
//!
//! The net cell owns exactly one smoltcp `dns::Socket`; every consumer resolves
//! through `NetRequest::Resolve`, so literal parsing, the SLIRP alias table and
//! the wire query live in one place instead of one copy per tool. Resolution
//! order:
//!
//! 1. **IPv4 literal** — returned without touching the wire.
//! 2. **SLIRP alias** — `gateway`/`host` → 10.0.2.2 (host loopback), `dns` →
//!    the leased DNS server, `localhost` → 127.0.0.1.
//! 3. **UDP A-record query** to the DNS server from the DHCP lease (option 6),
//!    falling back to SLIRP's 10.0.2.3 when the lease carries none.
//!
//! The query is synchronous: the service is a single-threaded message loop, so a
//! lookup parks request handling for at most [`RESOLVE_BUDGET`]. Other sockets
//! keep progressing while it waits because the wait loop polls the whole
//! interface, not just the DNS socket.

use alloc::vec;
use ostd::syscall::sys_wait_completion;
use smoltcp::iface::{Interface, SocketHandle, SocketSet};
use smoltcp::socket::dns;
use smoltcp::time::Duration;
use smoltcp::wire::{DnsQueryType, IpAddress};

use crate::interface::VirtioNetDevice;
use crate::service_runtime::now_instant;

/// QEMU SLIRP's built-in resolver — the fallback when the DHCP lease carries no
/// DNS option. SLIRP forwards it to the host's own resolvers.
pub const SLIRP_DNS_SERVER: [u8; 4] = [10, 0, 2, 3];

/// SLIRP's alias for the host loopback: guest traffic to it lands on the host's
/// 127.0.0.1, which is how the integration tests reach host mocks.
const SLIRP_HOST: [u8; 4] = [10, 0, 2, 2];

/// Ceiling on one A-record lookup. smoltcp retransmits at 1 s, then 2 s; a
/// server that has not answered within this window is treated as unreachable.
const RESOLVE_BUDGET: Duration = Duration::from_millis(3_000);

/// Wake cadence while a query is in flight — one scheduler tick (10 ms).
const RX_POLL_TICKS: u64 = 1;

/// Iteration backstop for [`Resolver::resolve`].
///
/// The budget above is wall-clock. At one wake per tick this is twice what the
/// budget can consume, so it only bites if the clock itself stops advancing —
/// and then the service still answers `Err` instead of parking its message loop
/// forever (which the watchdog would read as a hung cell).
const RESOLVE_POLL_CEILING: usize = 600;

/// Resolve without the wire: IPv4 literals and the SLIRP names.
///
/// `server` is the DNS server currently in force, so `dns` answers with the
/// address the resolver would actually query.
pub fn static_lookup(hostname: &str, server: [u8; 4]) -> Option<[u8; 4]> {
    match hostname {
        "dns" => return Some(server),
        "gateway" | "host" => return Some(SLIRP_HOST),
        "localhost" => return Some([127, 0, 0, 1]),
        _ => {}
    }
    parse_ipv4(hostname)
}

/// Strict dotted-quad parser: exactly four decimal octets, no leading zeros,
/// each ≤ 255. Anything else is a hostname and goes to the resolver.
fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in s.split('.') {
        if count == 4 {
            return None;
        }
        octets[count] = parse_octet(part)?;
        count += 1;
    }
    (count == 4).then_some(octets)
}

fn parse_octet(s: &str) -> Option<u8> {
    if s.is_empty() || s.len() > 3 || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    let mut n: u16 = 0;
    for ch in s.bytes() {
        if !ch.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (ch - b'0') as u16;
    }
    (n <= 255).then_some(n as u8)
}

/// The net cell's resolver: one `dns::Socket` plus the server it queries.
pub struct Resolver {
    handle: SocketHandle,
    server: [u8; 4],
}

impl Resolver {
    /// Install the DNS socket in the interface's socket set.
    pub fn install(sockets: &mut SocketSet<'_>) -> Self {
        let server = SLIRP_DNS_SERVER;
        let handle = sockets.add(dns::Socket::new(&[ip_address(server)], vec![]));
        Self { handle, server }
    }

    /// The DNS server in force (lease option 6, else SLIRP's).
    pub fn server(&self) -> [u8; 4] {
        self.server
    }

    /// Adopt the DNS server handed out by DHCP.
    pub fn set_server(&mut self, sockets: &mut SocketSet<'_>, server: [u8; 4]) {
        if server == self.server {
            return;
        }
        self.server = server;
        sockets
            .get_mut::<dns::Socket>(self.handle)
            .update_servers(&[ip_address(server)]);
    }

    /// Resolve `hostname` to an IPv4 address, or `None` if the server does not
    /// answer within [`RESOLVE_BUDGET`] (the caller replies `Err`).
    ///
    /// Blocks: drives the interface in a bounded loop so the query, its
    /// retransmits and any RX frames are all serviced.
    pub fn resolve(
        &self,
        hostname: &str,
        iface: &mut Interface,
        device: &mut VirtioNetDevice,
        sockets: &mut SocketSet<'_>,
    ) -> Option<[u8; 4]> {
        let query = {
            let socket = sockets.get_mut::<dns::Socket>(self.handle);
            socket
                .start_query(iface.context(), hostname, DnsQueryType::A)
                .ok()?
        };

        let deadline = now_instant() + RESOLVE_BUDGET;
        for _ in 0..RESOLVE_POLL_CEILING {
            device.pump_rx_split();
            iface.poll(now_instant(), device, sockets);
            match sockets
                .get_mut::<dns::Socket>(self.handle)
                .get_query_result(query)
            {
                Ok(addresses) => return first_ipv4(&addresses),
                Err(dns::GetQueryResultError::Failed) => return None,
                Err(dns::GetQueryResultError::Pending) => {}
            }
            if now_instant() >= deadline {
                break;
            }
            wait_for_frame();
        }
        sockets
            .get_mut::<dns::Socket>(self.handle)
            .cancel_query(query);
        None
    }
}

fn first_ipv4(addresses: &[IpAddress]) -> Option<[u8; 4]> {
    addresses.iter().find_map(|address| match address {
        IpAddress::Ipv4(v4) => Some(v4.0),
        #[allow(unreachable_patterns)] // reason: proto-ipv6 is not enabled here
        _ => None,
    })
}

fn ip_address(ip: [u8; 4]) -> IpAddress {
    IpAddress::v4(ip[0], ip[1], ip[2], ip[3])
}

/// Park until an RX frame arrives or one scheduler tick elapses. The timeout is
/// the contract: a dropped reply must not park the service loop forever.
fn wait_for_frame() {
    let _ = sys_wait_completion(api::syscall::events::NET_RX, RX_POLL_TICKS);
}

#[cfg(test)]
mod tests;
