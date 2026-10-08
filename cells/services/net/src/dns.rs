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
//! Wire queries retain a bounded smoltcp DNS slot and are polled by the net
//! reactor alongside TCP; a slow resolver never parks other clients.

use alloc::vec;
use smoltcp::iface::{Interface, SocketHandle, SocketSet};
use smoltcp::socket::dns;
use smoltcp::wire::{DnsQueryType, IpAddress};

/// QEMU SLIRP's built-in resolver — the fallback when the DHCP lease carries no
/// DNS option. SLIRP forwards it to the host's own resolvers.
pub const SLIRP_DNS_SERVER: [u8; 4] = [10, 0, 2, 3];

/// SLIRP's alias for the host loopback: guest traffic to it lands on the host's
/// 127.0.0.1, which is how the integration tests reach host mocks.
const SLIRP_HOST: [u8; 4] = [10, 0, 2, 2];


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

    /// Begin a wire query without parking the net service loop. The returned
    /// handle owns one smoltcp DNS slot until poll_result or cancel.
    pub fn start(
        &self,
        hostname: &str,
        iface: &mut Interface,
        sockets: &mut SocketSet<'_>,
    ) -> Option<dns::QueryHandle> {
        sockets
            .get_mut::<dns::Socket>(self.handle)
            .start_query(iface.context(), hostname, DnsQueryType::A)
            .ok()
    }

    pub fn poll_result(
        &self,
        query: dns::QueryHandle,
        sockets: &mut SocketSet<'_>,
    ) -> Option<Option<[u8; 4]>> {
        match sockets.get_mut::<dns::Socket>(self.handle).get_query_result(query) {
            Ok(addresses) => Some(first_ipv4(&addresses)),
            Err(dns::GetQueryResultError::Failed) => Some(None),
            Err(dns::GetQueryResultError::Pending) => None,
        }
    }

    pub fn cancel(&self, query: dns::QueryHandle, sockets: &mut SocketSet<'_>) {
        sockets.get_mut::<dns::Socket>(self.handle).cancel_query(query);
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


#[cfg(test)]
mod tests;
