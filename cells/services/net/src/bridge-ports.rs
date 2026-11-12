//! Guest bridge ports beside the host stack.
//!
//! Each L2 client (a hypervisor Cell, one per guest) owns one port, keyed by
//! its Cell owner generation and released when that generation exits. Frames
//! from the NIC are demultiplexed by destination MAC: a port's address goes to
//! that port only, broadcast and multicast go to the host stack and every port,
//! anything else goes to the host stack.
//!
//! Invariant: a port's address is a unicast station address that is neither the
//! host stack's nor another port's. It is checked when a port opens and on
//! every frame a guest sends, so host unicast never reaches a guest, one guest
//! never receives another's unicast, and no guest sends as the host.

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};

use crate::socket_table::SocketOwner;

/// Ports open at once (one per running guest).
pub(crate) const MAX_PORTS: usize = 8;
/// Frames held for one port between its polls. A guest that stops polling
/// loses its own newest frames, never the host stack's.
const PORT_RX_LIMIT: usize = 64;

/// Why a port could not open or a guest frame was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PortError {
    /// Zero or multicast: not an address anyone can send from.
    NotStation,
    /// The host stack's address or another port's.
    AddressInUse,
    /// Every port is taken.
    Full,
}

struct Port {
    owner: SocketOwner,
    mac: [u8; 6],
    rx: VecDeque<Box<[u8]>>,
}

impl Port {
    fn offer(&mut self, frame: &[u8]) {
        if self.rx.len() < PORT_RX_LIMIT {
            self.rx.push_back(Box::from(frame));
        }
    }
}

#[derive(Default)]
pub(crate) struct Bridge {
    ports: Vec<Port>,
}

fn is_station(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac[0] & 0x01 == 0
}

fn source_mac(frame: &[u8]) -> Option<[u8; 6]> {
    frame.get(6..12)?.try_into().ok()
}

impl Bridge {
    fn port_mut(&mut self, owner: SocketOwner) -> Option<&mut Port> {
        self.ports.iter_mut().find(|port| port.owner == owner)
    }

    /// Whether `owner` may use `mac`.
    fn check(&self, owner: SocketOwner, mac: [u8; 6], host_mac: [u8; 6]) -> Result<(), PortError> {
        if !is_station(mac) {
            return Err(PortError::NotStation);
        }
        if mac == host_mac
            || self
                .ports
                .iter()
                .any(|port| port.owner != owner && port.mac == mac)
        {
            return Err(PortError::AddressInUse);
        }
        Ok(())
    }

    fn add(&mut self, owner: SocketOwner, mac: [u8; 6]) -> Result<(), PortError> {
        if self.ports.len() >= MAX_PORTS {
            return Err(PortError::Full);
        }
        self.ports.push(Port {
            owner,
            mac,
            rx: VecDeque::new(),
        });
        Ok(())
    }

    /// Open `owner`'s port at `mac`. An open port is left as it is: the address
    /// its guest sends from (see [`Self::admit_tx`]) outranks the one the
    /// client names on each poll.
    pub(crate) fn open(
        &mut self,
        owner: SocketOwner,
        mac: [u8; 6],
        host_mac: [u8; 6],
    ) -> Result<(), PortError> {
        if self.ports.iter().any(|port| port.owner == owner) {
            return Ok(());
        }
        self.check(owner, mac, host_mac)?;
        self.add(owner, mac)
    }

    /// Admit a frame `owner`'s guest sent, adopting its source address as the
    /// port's (a learning bridge: replies go where the guest sends from).
    /// A refused frame must not be sent.
    pub(crate) fn admit_tx(
        &mut self,
        owner: SocketOwner,
        frame: &[u8],
        host_mac: [u8; 6],
    ) -> Result<(), PortError> {
        let source = source_mac(frame).ok_or(PortError::NotStation)?;
        self.check(owner, source, host_mac)?;
        match self.port_mut(owner) {
            Some(port) => {
                port.mac = source;
                Ok(())
            }
            None => self.add(owner, source),
        }
    }

    /// Close `owner`'s port and drop its queued frames.
    pub(crate) fn release(&mut self, owner: SocketOwner) -> bool {
        let before = self.ports.len();
        self.ports.retain(|port| port.owner != owner);
        self.ports.len() != before
    }

    /// Deliver one received frame to the ports it is for. Returns whether the
    /// host stack receives it too.
    pub(crate) fn route(&mut self, frame: &[u8]) -> bool {
        let Some(destination) = frame.get(..6) else {
            return true;
        };
        if destination[0] & 0x01 != 0 {
            for port in &mut self.ports {
                port.offer(frame);
            }
            return true;
        }
        match self
            .ports
            .iter_mut()
            .find(|port| port.mac[..] == *destination)
        {
            Some(port) => {
                port.offer(frame);
                false
            }
            None => true,
        }
    }

    /// Next frame queued for `owner`'s port.
    pub(crate) fn pop(&mut self, owner: SocketOwner) -> Option<Box<[u8]>> {
        self.port_mut(owner)?.rx.pop_front()
    }

    /// The address `owner`'s port answers to (host tests).
    #[cfg(test)]
    pub(crate) fn mac(&self, owner: SocketOwner) -> Option<[u8; 6]> {
        self.ports
            .iter()
            .find(|port| port.owner == owner)
            .map(|port| port.mac)
    }

    /// Frames queued across every port (`loop-trace` images).
    #[cfg(feature = "loop-trace")]
    pub(crate) fn queued(&self) -> usize {
        self.ports.iter().map(|port| port.rx.len()).sum()
    }
}

#[cfg(test)]
#[path = "bridge-ports-tests.rs"]
mod tests;
