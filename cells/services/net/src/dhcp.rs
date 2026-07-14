//! DHCPv4 boot client for the net service cell.
//!
//! Adds a smoltcp `Dhcpv4Socket` to the socket set and drives it through the
//! DISCOVER → OFFER → REQUEST → ACK sequence.  On success it calls
//! `interface.update_ip_addrs()` and `interface.routes_mut()` to apply the
//! leased address and default gateway.

use crate::interface::VirtioNetDevice;
use ostd::io::println;
use smoltcp::{
    iface::{Interface, SocketSet},
    socket::dhcpv4,
    time::Instant,
    wire::IpCidr,
};

/// State of the DHCP boot sequence.
#[derive(Debug, PartialEq, Eq)]
pub enum DhcpState {
    /// Awaiting DHCP ACK.
    Pending,
    /// IP address acquired successfully.
    Acquired,
    #[allow(dead_code)] // reason: will be used when retry limits are wired in
    Failed,
}

/// Add a `Dhcpv4Socket` to `sockets` and return its handle.
pub fn add_dhcp_socket(sockets: &mut SocketSet<'_>) -> smoltcp::iface::SocketHandle {
    sockets.add(dhcpv4::Socket::new())
}

/// Poll the DHCP socket and apply a lease when one arrives.
///
/// Returns `DhcpState::Acquired` once an IP address is configured on `iface`.
pub fn poll_dhcp(
    handle: smoltcp::iface::SocketHandle,
    iface: &mut Interface,
    sockets: &mut SocketSet<'_>,
    device: &mut VirtioNetDevice,
    now: Instant,
) -> DhcpState {
    iface.poll(now, device, sockets);

    let socket = sockets.get_mut::<dhcpv4::Socket>(handle);
    if let Some(event) = socket.poll() {
        match event {
            dhcpv4::Event::Configured(config) => {
                iface.update_ip_addrs(|addrs| {
                    if let Some(slot) = addrs.iter_mut().find(|a| matches!(a, IpCidr::Ipv4(_))) {
                        *slot = IpCidr::Ipv4(config.address);
                    } else {
                        let _ = addrs.push(IpCidr::Ipv4(config.address));
                    }
                });
                if let Some(gw) = config.router {
                    iface.routes_mut().add_default_ipv4_route(gw).ok();
                }
                println("[net] DHCP acquired — IP configured");
                return DhcpState::Acquired;
            }
            dhcpv4::Event::Deconfigured => {
                println("[net] DHCP: deconfigured");
            }
        }
    }
    DhcpState::Pending
}
