use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const NO_CONNECT_REPLY: u64 = u64::MAX;
static LAST_CONNECT_REPLY: AtomicU64 = AtomicU64::new(NO_CONNECT_REPLY);
static BUFFER_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn record_connect_reply(reply: &[u8; 8]) {
    LAST_CONNECT_REPLY.store(u64::from_le_bytes(*reply), Ordering::Relaxed);
}

pub(crate) fn record_buffer_allocation() {
    BUFFER_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
}

use alloc::collections::BTreeMap;
use embedded_tls::TlsError;
use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    socket::tcp,
    time::Instant,
    wire::{EthernetAddress, HardwareAddress},
};

use crate::{
    interface::VirtioNetDevice,
    socket_table::{SocketOwner, SocketTable},
    tls::socket::TlsSocketEntry,
    tls_handler::handle_tls_raw,
    tls_wire::TLS_CONNECT_OP,
};

fn interface_config() -> Config {
    Config::new(HardwareAddress::Ethernet(EthernetAddress([
        0x02, 0, 0, 0, 0, 1,
    ])))
}

#[test]
fn handler_rejects_before_allocating_tcp_capability() {
    LAST_CONNECT_REPLY.store(NO_CONNECT_REPLY, Ordering::Relaxed);
    let mut device = VirtioNetDevice::new();
    let mut iface = Interface::new(interface_config(), &mut device, Instant::from_millis(0));
    let mut storage = [SocketStorage::EMPTY];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let mut table = SocketTable::new();
    let mut tls_table = BTreeMap::new();
    let owner = SocketOwner {
        cell_id: 1,
        generation: 1,
    };
    let mut request = [0u8; 21];
    request[0] = TLS_CONNECT_OP;
    request[15..17].copy_from_slice(&4u16.to_le_bytes());
    request[17..].copy_from_slice(b"mock");

    handle_tls_raw(
        &request,
        1,
        owner,
        &mut iface,
        &mut device,
        &mut sockets,
        &mut table,
        &mut tls_table,
    );

    assert_eq!(table.next_cap_for_test(), 1);
    assert_eq!(sockets.iter().count(), 0);
    assert!(tls_table.is_empty());
    assert_eq!(LAST_CONNECT_REPLY.load(Ordering::Relaxed), 0);
}

#[test]
fn socket_entry_rejects_before_handshake_state() {
    BUFFER_ALLOCATIONS.store(0, Ordering::Relaxed);
    let mut device = VirtioNetDevice::new();
    let mut iface = Interface::new(interface_config(), &mut device, Instant::from_millis(0));
    let mut storage = [SocketStorage::EMPTY];
    let mut sockets = SocketSet::new(&mut storage[..]);
    let handle = sockets.add(tcp::Socket::new(
        tcp::SocketBuffer::new(alloc::vec![0u8; 64]),
        tcp::SocketBuffer::new(alloc::vec![0u8; 64]),
    ));
    let sockets_ptr = &mut sockets as *mut SocketSet<'_> as *mut ();
    unsafe {
        crate::tls::transport::set_tls_context(
            &mut iface as *mut Interface,
            &mut device as *mut VirtioNetDevice,
            sockets_ptr,
        );
    }

    let result = unsafe { TlsSocketEntry::handshake(handle, "mock") };

    assert!(matches!(result, Err(TlsError::InvalidCertificate)));
    assert_eq!(BUFFER_ALLOCATIONS.load(Ordering::Relaxed), 0);
}
