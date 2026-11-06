//! Host tests for `shared.rs`: the layout is pinned to the packed byte
//! offsets of the device interface, and the encoders write exactly them.

use super::*;
use std::collections::BTreeMap;

/// A byte vector that also records which offsets were written.
struct Bytes(Vec<u8>, BTreeMap<usize, usize>);

impl Bytes {
    fn new(len: usize) -> Self {
        Self(vec![0; len], BTreeMap::new())
    }
    fn put(&mut self, offset: usize, bytes: &[u8]) {
        self.0[offset..offset + bytes.len()].copy_from_slice(bytes);
        self.1.insert(offset, bytes.len());
    }
    fn u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap())
    }
    fn u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.0[offset..offset + 8].try_into().unwrap())
    }
}

impl Sink for Bytes {
    fn put8(&mut self, offset: usize, value: u8) {
        self.put(offset, &[value]);
    }
    fn put16(&mut self, offset: usize, value: u16) {
        self.put(offset, &value.to_le_bytes());
    }
    fn put32(&mut self, offset: usize, value: u32) {
        self.put(offset, &value.to_le_bytes());
    }
    fn put64(&mut self, offset: usize, value: u64) {
        self.put(offset, &value.to_le_bytes());
    }
}

#[test]
fn driver_shared_layout_is_the_revision_1_interface() {
    assert_eq!(DRIVER_SHARED_LEN, 720);
    assert_eq!(offset_of!(DriverShared, version), 8);
    assert_eq!(offset_of!(DriverShared, upt_features), 24);
    assert_eq!(offset_of!(DriverShared, queue_shared), 40);
    assert_eq!(offset_of!(DriverShared, queue_shared_len), 52);
    assert_eq!(offset_of!(DriverShared, mtu), 56);
    assert_eq!(offset_of!(DriverShared, nrxsg_max), 60);
    assert_eq!(offset_of!(DriverShared, ntxqueue), 62);
    assert_eq!(offset_of!(DriverShared, automask), 80);
    assert_eq!(offset_of!(DriverShared, evintr), 82);
    assert_eq!(offset_of!(DriverShared, modlevel), 83);
    assert_eq!(offset_of!(DriverShared, ictrl), 108);
    assert_eq!(offset_of!(DriverShared, rxmode), 120);
    assert_eq!(offset_of!(DriverShared, mcast_table), 128);
    assert_eq!(offset_of!(DriverShared, vlan_filter), 136);
    assert_eq!(offset_of!(DriverShared, rss), 648);
    assert_eq!(offset_of!(DriverShared, plugin), 680);
    assert_eq!(EVENT_OFF, 696);
}

#[test]
fn queue_descriptor_layout_is_256_bytes_each() {
    assert_eq!(TXQ_LEN, 256);
    assert_eq!(QUEUES_LEN, 512);
    assert_eq!(offset_of!(TxQueueShared, cmd_ring), 16);
    assert_eq!(offset_of!(TxQueueShared, data_ring), 24);
    assert_eq!(offset_of!(TxQueueShared, comp_ring), 32);
    assert_eq!(offset_of!(TxQueueShared, cmd_ring_len), 56);
    assert_eq!(offset_of!(TxQueueShared, comp_ring_len), 64);
    assert_eq!(offset_of!(TxQueueShared, intr_idx), 72);
    assert_eq!(offset_of!(TxQueueShared, error), 84);
    assert_eq!(offset_of!(RxQueueShared, cmd_ring), 16);
    assert_eq!(offset_of!(RxQueueShared, comp_ring), 32);
    assert_eq!(offset_of!(RxQueueShared, cmd_ring_len), 56);
    assert_eq!(offset_of!(RxQueueShared, comp_ring_len), 64);
    assert_eq!(offset_of!(RxQueueShared, intr_idx), 72);
    assert_eq!(RXQ_UPDATE_RXHEAD_OFF, 256);
}

#[test]
fn encoders_write_the_configuration_at_its_offsets() {
    let mut bytes = Bytes::new(2048);
    let plan = IntrPlan::for_vectors(Some(3));
    write_driver_shared(&mut bytes, 0, 0xABC0_0400, plan);
    assert_eq!(bytes.u32(0), REV1_MAGIC);
    assert_eq!(bytes.u32(16), 1, "revision 1");
    assert_eq!(bytes.u32(20), 1, "UPT 1");
    assert_eq!(bytes.u64(24), 0, "no offload features");
    assert_eq!(bytes.u64(40), 0xABC0_0400);
    assert_eq!(bytes.u32(52), 512);
    assert_eq!(bytes.u32(56), 1500);
    assert_eq!(&bytes.0[60..64], &[1, 0, 1, 1]);
    assert_eq!(
        &bytes.0[80..83],
        &[0, 3, 2],
        "no automask, 3 vectors, events on 2"
    );
    assert_eq!(bytes.u32(120), 0x1D, "ucast | bcast | allmulti | promisc");
    assert_eq!(bytes.u32(136), 1, "VLAN ID 0 (untagged) admitted");
    assert!(bytes.1.keys().all(|&o| o < DRIVER_SHARED_LEN));

    let q = QueueConfig {
        tx_ring: 0x1000,
        tx_data: 0x2000,
        tx_comp: 0x1200,
        rx_ring: [0x1400, 0x1600],
        rx_comp: 0x1800,
        tx_len: 32,
        rx_len: [32, 32],
        rx_comp_len: 64,
    };
    write_queues(&mut bytes, 1024, &q, plan);
    let (tx, rx) = (1024, 1024 + 256);
    assert_eq!(
        (bytes.u64(tx + 16), bytes.u64(tx + 24), bytes.u64(tx + 32)),
        (0x1000, 0x2000, 0x1200)
    );
    assert_eq!(
        (bytes.u32(tx + 56), bytes.u32(tx + 60), bytes.u32(tx + 64)),
        (32, 32, 32)
    );
    assert_eq!(bytes.0[tx + 72], 0);
    assert_eq!(
        (bytes.u64(rx + 16), bytes.u64(rx + 24), bytes.u64(rx + 32)),
        (0x1400, 0x1600, 0x1800)
    );
    assert_eq!(
        (bytes.u32(rx + 56), bytes.u32(rx + 60), bytes.u32(rx + 64)),
        (32, 32, 64)
    );
    assert_eq!(bytes.0[rx + 72], 1);
}
