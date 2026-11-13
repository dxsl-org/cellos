//! The net service's turn work, run before every receive.

use ostd::io::println;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr};

use super::net_state::NetState;
use super::{
    apply_link_local, dotted_ipv4, drain_ready_waits, graceful_deadline_line, now_instant,
    poll_resolves, reap_aborted, reap_graceful_closes, sys_get_time, MTIME_TICKS_PER_MS,
    SMOLTCP_MAINTENANCE_TICKS,
};
use crate::dhcp::{poll_dhcp, DhcpState};
use crate::link_local::LinkLocal;

impl NetState<'_> {
    /// The turn work before every receive: late MAC binding, NIC drain, DHCP,
    /// link-local, one stack poll, TLS and DNS progress, close reapers and
    /// ready waits.
    pub(super) fn pump(&mut self) {
        if !self.mac_synchronized {
            if let Some(mac) = self.device.query_driver_mac() {
                println(&alloc::format!(
                    "[net] hardware MAC bound from NIC driver: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                ));
                self.iface
                    .set_hardware_addr(HardwareAddress::Ethernet(EthernetAddress(mac)));
                self.device.set_host_mac(mac);
                self.mac_synchronized = true;
                if self.dhcp_state == DhcpState::Pending {
                    self.sockets
                        .get_mut::<smoltcp::socket::dhcpv4::Socket>(self.dhcp_handle)
                        .reset();
                }
            }
        }
        self.probe.turn(&self.device);
        let drained = self.device.pump_rx_split();
        self.probe.rx_drained(drained);
        if self.dhcp_state == DhcpState::Pending {
            self.poll_dhcp();
        }
        self.poll_link_local();

        let now = sys_get_time();
        // Poll on every turn, not only on the 100 ms maintenance cadence:
        // smoltcp advances a handshake one step per poll, and a listener
        // stuck in SynReceived cannot report ACCEPT. Waiting for the
        // maintenance tick added up to 100 ms per handshake step.
        self.iface
            .poll(now_instant(), &mut self.device, &mut self.sockets);
        if now.wrapping_sub(self.last_poll_ticks) >= SMOLTCP_MAINTENANCE_TICKS {
            self.last_poll_ticks = now;
        }
        #[cfg(feature = "qemu-dev-https")]
        self.dev_clock
            .poll(&mut self.iface, &mut self.sockets, self.local_ip != [0; 4]);
        self.tls_pending.poll(
            &mut self.iface,
            &mut self.device,
            &mut self.sockets,
            &mut self.table,
            &mut self.tls_table,
        );
        poll_resolves(&self.resolver, &mut self.pending_dns, &mut self.sockets);
        let graceful_aborted =
            reap_graceful_closes(&mut self.table, &mut self.sockets, sys_get_time());
        if let Some(line) = graceful_deadline_line(graceful_aborted) {
            println(&line);
        }
        reap_aborted(&mut self.table, &mut self.sockets, sys_get_time());
        drain_ready_waits(&mut self.pending_ready, &mut self.sockets, &self.table);
    }

    fn poll_dhcp(&mut self) {
        let lease = poll_dhcp(
            self.dhcp_handle,
            &mut self.iface,
            &mut self.sockets,
            &mut self.device,
            now_instant(),
        );
        self.dhcp_state = lease.state;
        if self.dhcp_state == DhcpState::Acquired {
            // The lease has just overwritten the IPv4 slot, claimed
            // link-local address included.
            self.link_local = None;
            if let Some(IpCidr::Ipv4(cidr)) = self
                .iface
                .ip_addrs()
                .iter()
                .find(|address| matches!(address, IpCidr::Ipv4(_)))
            {
                self.local_ip.copy_from_slice(cidr.address().as_bytes());
                println(&alloc::format!(
                    "[net] IP address: {}",
                    dotted_ipv4(self.local_ip)
                ));
            }
        } else if self.link_local.is_none()
            && sys_get_time().wrapping_sub(self.dhcp_start_ticks) >= 4_000 * MTIME_TICKS_PER_MS
        {
            let HardwareAddress::Ethernet(addr) = self.iface.hardware_addr();
            self.link_local = Some(LinkLocal::new(addr.0, sys_get_time() / MTIME_TICKS_PER_MS));
        }
        // The lease names the resolver to use from here on (option 6);
        // without it the DNS socket keeps SLIRP's built-in default.
        if let Some(server) = lease.dns_server {
            self.resolver.set_server(&mut self.sockets, server);
            println(&alloc::format!("[net] DNS server: {}", dotted_ipv4(server)));
        }
    }

    /// The ARP tap is drained every turn so it never holds stale packets; they
    /// feed the claim while one runs.
    fn poll_link_local(&mut self) {
        let now_ms = sys_get_time() / MTIME_TICKS_PER_MS;
        while let Some(packet) = self.device.take_arp() {
            if let Some(engine) = self.link_local.as_mut() {
                engine.on_arp(packet, now_ms);
            }
        }
        if let Some(engine) = self.link_local.as_mut() {
            engine.poll(now_ms);
            while let Some(event) = engine.next_event() {
                apply_link_local(event, &mut self.iface, &mut self.device, &mut self.local_ip);
            }
        }
    }
}
