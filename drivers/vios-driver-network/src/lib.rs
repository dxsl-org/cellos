#![no_std]

extern crate alloc;

// use ostd::prelude::*; // Fix unused import later
use smoltcp::iface::{Interface, SocketStorage};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
use smoltcp::phy::{Device, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::socket::tcp;
use alloc::collections::BTreeMap;
use alloc::vec::Vec; // Fix Alloc

/// Virtual Network Device (Loopback/Simulation)
pub struct VirtualNetworkDevice {
    mtu: usize,
}

impl VirtualNetworkDevice {
    pub fn new() -> Self {
        Self { mtu: 1500 }
    }
}

pub struct VirtualRxToken(Vec<u8>);
impl RxToken for VirtualRxToken {
     fn consume<R, F>(self, f: F) -> R
     where F: FnOnce(&mut [u8]) -> R {
         let mut buffer = self.0;
         f(&mut buffer)
     }
}

pub struct VirtualTxToken;
impl TxToken for VirtualTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where F: FnOnce(&mut [u8]) -> R {
        let mut buffer = [0u8; 1500];
        let res = f(&mut buffer[..len]);
        log::info!("NET: Transmitted {} bytes", len);
        res
    }
}

impl Device for VirtualNetworkDevice {
    type RxToken<'a> = VirtualRxToken;
    type TxToken<'a> = VirtualTxToken;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // Simulation: No data received yet
        None
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(VirtualTxToken)
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mut caps = smoltcp::phy::DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = self.mtu;
        caps
    }
}

use smoltcp::iface::{Config, SocketSet};

pub struct NetworkDriver<'a> {
    interface: Interface,
    device: VirtualNetworkDevice,
    sockets: SocketSet<'a>,
}

impl<'a> NetworkDriver<'a> {
    pub fn new(mut device: VirtualNetworkDevice) -> Self {
        // Create interface
        let mut config = Config::new(EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]).into());
        config.random_seed = 0x1234; // Simulation deterministic seed
        
        let interface = Interface::new(config, &mut device, Instant::now());
        
        // Create sockets
        // For no_std without alloc feature in smoltcp, we need static storage or reference.
        // Assuming we pass storage in later or use a fixed vec for now.
        let sockets = SocketSet::new(Vec::new());

        Self {
            interface,
            device,
            sockets,
        }
    }

    pub fn poll(&mut self, timestamp: Instant) {
        // smoltcp::Interface::poll returns bool (true if something happened)
        // It does not return Result anymore in 0.10? Or maybe it does but I need to handle it differently.
        // Actually, poll returns `bool` in later versions, signifying readiness. Wait.
        // Let's check docs: `poll` typically returns `bool` in 0.8+, maybe changed in 0.10?
        // The error suggests it has type `bool`.
        
        let processed = self.interface.poll(timestamp, &mut self.device, &mut self.sockets);
        if processed {
             log::trace!("NET: Poll processed packets.");
        }
    }
    
    pub fn init() {
        ostd::println!("Network Driver: Initializing smoltcp...");
        
        let device = VirtualNetworkDevice::new();
        let mut driver = Self::new(device);
        
        // Add a loopback IP
        driver.interface.update_ip_addrs(|ip_addrs| {
            ip_addrs.push(IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8)).unwrap();
        });

        ostd::println!("Network Driver: Loopback Interface (127.0.0.1) Ready.");
        
        // In a real driver, we'd store `driver` in a static or run a loop
        // driver.poll(Instant::now());
    }
}
