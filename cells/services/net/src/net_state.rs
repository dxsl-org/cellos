//! What the net service keeps across loop turns, and its boot. The turn work
//! that runs before every receive is in `net_pump.rs`.

use alloc::collections::BTreeMap;
use ostd::io::println;
use ostd::service::owners::OwnerWatches;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet, SocketStorage},
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};

use super::probe::Probe;
use super::{
    now_instant, sys_get_time, PendingDns, PendingReady, DNS_QUERY_LIMIT, MAC, OWNER_WATCH_CAPACITY,
};
use crate::dhcp::{add_dhcp_socket, DhcpState};
use crate::{
    dns::Resolver,
    interface::VirtioNetDevice,
    link_local::LinkLocal,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
    tls_handler::TlsPending,
};

/// The service loop's state. `'s` is the socket storage, owned by the frame
/// of [`super::run`], which never returns.
pub(super) struct NetState<'s> {
    pub(super) device: VirtioNetDevice,
    pub(super) iface: Interface,
    pub(super) sockets: SocketSet<'s>,
    pub(super) table: SocketTable,
    pub(super) tls_table: BTreeMap<u64, TlsSocketEntry>,
    pub(super) tls_pending: TlsPending,
    #[cfg(feature = "qemu-dev-https")]
    pub(super) dev_clock: crate::tls::qemu_time::ClockClient,
    pub(super) pending_ready: BTreeMap<SocketOwner, PendingReady>,
    pub(super) pending_dns: heapless::Vec<PendingDns, DNS_QUERY_LIMIT>,
    /// One DNS socket serves every `NetRequest::Resolve`; the server starts as
    /// SLIRP's and is replaced by whatever the DHCP lease names.
    pub(super) resolver: Resolver,
    /// Owners whose root death is watched ([`super::OWNER_WATCH_CAPACITY`]).
    pub(super) watches: OwnerWatches,
    pub(super) dhcp_handle: SocketHandle,
    pub(super) dhcp_state: DhcpState,
    pub(super) dhcp_start_ticks: u64,
    /// RFC 3927 claim, started once DHCP has had its head start; dropped when a
    /// lease arrives, because a lease outranks a link-local address.
    pub(super) link_local: Option<LinkLocal>,
    pub(super) last_poll_ticks: u64,
    pub(super) local_ip: [u8; 4],
    pub(super) ipc_burst_grace: u8,
    pub(super) mac_synchronized: bool,
    pub(super) probe: Probe,
}

impl<'s> NetState<'s> {
    pub(super) fn new(socket_storage: &'s mut [SocketStorage<'s>]) -> Self {
        println("[net] Network Service v0.1: smoltcp + NIC Driver Cell + DHCP");

        let mut device = VirtioNetDevice::new();

        // Query the NIC Driver Cell's hardware MAC. A brief yield window allows
        // a concurrently starting driver cell to publish its endpoint before smoltcp starts.
        let mut hardware_mac = None;
        for _ in 0..100 {
            if let Some(mac) = device.query_driver_mac() {
                hardware_mac = Some(mac);
                break;
            }
            ostd::syscall::sys_yield();
        }

        let initial_mac = match hardware_mac {
            Some(mac) => {
                println(&alloc::format!(
                    "[net] hardware MAC from NIC driver: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0],
                    mac[1],
                    mac[2],
                    mac[3],
                    mac[4],
                    mac[5]
                ));
                EthernetAddress(mac)
            }
            None => {
                println("[net] NIC driver MAC unavailable — using fallback MAC 52:54:00:12:34:56");
                MAC
            }
        };

        device.set_host_mac(initial_mac.0);
        let config = crate::stack_entropy::seeded_config(HardwareAddress::Ethernet(initial_mac));
        let mut iface = Interface::new(config, &mut device, now_instant());
        iface.update_ip_addrs(|addresses| {
            let _ = addresses.push(IpCidr::new(IpAddress::v4(0, 0, 0, 0), 0));
        });

        let mut sockets = SocketSet::new(socket_storage);
        let resolver = Resolver::install(&mut sockets);
        let dhcp_handle = add_dhcp_socket(&mut sockets);
        let state = Self {
            device,
            iface,
            sockets,
            table: SocketTable::new(),
            tls_table: BTreeMap::new(),
            tls_pending: TlsPending::default(),
            #[cfg(feature = "qemu-dev-https")]
            dev_clock: crate::tls::qemu_time::ClockClient::default(),
            pending_ready: BTreeMap::new(),
            pending_dns: heapless::Vec::new(),
            resolver,
            watches: OwnerWatches::new(OWNER_WATCH_CAPACITY),
            dhcp_handle,
            dhcp_state: DhcpState::Pending,
            dhcp_start_ticks: sys_get_time(),
            link_local: None,
            last_poll_ticks: sys_get_time(),
            local_ip: [0; 4],
            ipc_burst_grace: 0,
            mac_synchronized: hardware_mac.is_some(),
            probe: Probe::new(),
        };
        println("[net] Starting DHCP...");
        state
    }
}
