//! `DriverShared` and the queue descriptors: the configuration the driver
//! hands the device by address (`VD_DSL`/`VD_DSH`, revision 1 layout).
//!
//! The `#[repr(C)]` mirrors below follow FreeBSD's `if_vmxreg.h` structs
//! (`vmxnet3_driver_shared`, `vmxnet3_txq_shared`, `vmxnet3_rxq_shared`; ISC,
//! see `NOTICE`). They exist only to derive field offsets: every field is
//! naturally aligned in the packed device layout, so `repr(C)` reproduces it,
//! and the tests pin each offset and size. Values are written byte-exact
//! through a [`Sink`] (the DMA area, or a byte vector in tests).

pub(crate) use crate::dma::Sink;
use crate::irq::IntrPlan;
use core::mem::{offset_of, size_of};

/// Most interrupt vectors the device interface describes (8 TX + 16 RX + 1).
const MAX_INTRS: usize = 25;
/// `DriverShared.magic` of revision 1.
pub(crate) const REV1_MAGIC: u32 = 0xBABE_FEE1;
/// Guest OS word: FreeBSD-type, 64-bit (the value OpenBSD's driver also uses).
const GOS_FREEBSD_64: u32 = 0x10 | 0x02;
/// `rxmode`: unicast (0x01) | broadcast (0x04) | all-multicast (0x08).
const RXMODE: u32 = 0x01 | 0x04 | 0x08;
const MTU: u32 = 1500;

#[repr(C)]
#[allow(dead_code)]
struct ConfDesc {
    version: u32,
    len: u32,
    paddr: u64,
}

#[repr(C)]
#[allow(dead_code)] // layout mirror: only `offset_of!`/`size_of` read it
struct DriverShared {
    magic: u32,
    pad1: u32,
    version: u32,
    guest: u32,
    vmxnet3_revision: u32,
    upt_version: u32,
    upt_features: u64,
    driver_data: u64,
    queue_shared: u64,
    driver_data_len: u32,
    queue_shared_len: u32,
    mtu: u32,
    nrxsg_max: u16,
    ntxqueue: u8,
    nrxqueue: u8,
    reserved1: [u32; 4],
    automask: u8,
    nintr: u8,
    evintr: u8,
    modlevel: [u8; MAX_INTRS],
    ictrl: u32,
    reserved2: [u32; 2],
    rxmode: u32,
    mcast_tablelen: u16,
    pad2: u16,
    mcast_table: u64,
    vlan_filter: [u32; 128],
    rss: ConfDesc,
    pm: ConfDesc,
    plugin: ConfDesc,
    event: u32,
    reserved3: [u32; 5],
}

/// The configuration part both queue descriptors share; the 80-byte stats
/// block and padding fill each to 256 bytes.
#[repr(C)]
#[allow(dead_code)]
struct TxQueueShared {
    npending: u32,
    intr_threshold: u32,
    reserved1: u64,
    cmd_ring: u64,
    data_ring: u64,
    comp_ring: u64,
    driver_data: u64,
    reserved2: u64,
    cmd_ring_len: u32,
    data_ring_len: u32,
    comp_ring_len: u32,
    driver_data_len: u32,
    intr_idx: u8,
    pad1: [u8; 7],
    stopped: u8,
    pad2: [u8; 3],
    error: u32,
    stats: [u64; 10],
    pad3: [u8; 88],
}

#[repr(C)]
#[allow(dead_code)]
struct RxQueueShared {
    update_rxhead: u8,
    pad1: [u8; 7],
    reserved1: u64,
    cmd_ring: [u64; 2],
    comp_ring: u64,
    driver_data: u64,
    reserved2: u64,
    cmd_ring_len: [u32; 2],
    comp_ring_len: u32,
    driver_data_len: u32,
    intr_idx: u8,
    pad2: [u8; 7],
    stopped: u8,
    pad3: [u8; 3],
    error: u32,
    stats: [u64; 10],
    pad4: [u8; 88],
}

pub(crate) const DRIVER_SHARED_LEN: usize = size_of::<DriverShared>();
pub(crate) const TXQ_LEN: usize = size_of::<TxQueueShared>();
pub(crate) const QUEUES_LEN: usize = TXQ_LEN + size_of::<RxQueueShared>();
/// Byte offsets the cell reads back (device-written).
pub(crate) const EVENT_OFF: usize = offset_of!(DriverShared, event);
pub(crate) const RXQ_UPDATE_RXHEAD_OFF: usize = TXQ_LEN + offset_of!(RxQueueShared, update_rxhead);

/// Device-visible addresses and sizes of the queue 0 rings.
pub(crate) struct QueueConfig {
    pub tx_ring: u64,
    pub tx_data: u64,
    pub tx_comp: u64,
    pub rx_ring: [u64; 2],
    pub rx_comp: u64,
    pub tx_len: u32,
    pub rx_len: [u32; 2],
    pub rx_comp_len: u32,
}

/// Write `DriverShared` at `base`: one TX and one RX queue described at
/// `queues_iova`, unicast + broadcast + all-multicast, untagged frames
/// (VLAN ID 0) admitted, no offloads.
pub(crate) fn write_driver_shared(
    s: &mut impl Sink,
    base: usize,
    queues_iova: u64,
    plan: IntrPlan,
) {
    let at = |field: usize| base + field;
    s.put32(at(offset_of!(DriverShared, magic)), REV1_MAGIC);
    s.put32(at(offset_of!(DriverShared, version)), 1);
    s.put32(at(offset_of!(DriverShared, guest)), GOS_FREEBSD_64);
    s.put32(at(offset_of!(DriverShared, vmxnet3_revision)), 1);
    s.put32(at(offset_of!(DriverShared, upt_version)), 1);
    s.put64(at(offset_of!(DriverShared, queue_shared)), queues_iova);
    s.put32(
        at(offset_of!(DriverShared, queue_shared_len)),
        QUEUES_LEN as u32,
    );
    s.put32(at(offset_of!(DriverShared, mtu)), MTU);
    s.put16(at(offset_of!(DriverShared, nrxsg_max)), 1);
    s.put8(at(offset_of!(DriverShared, ntxqueue)), 1);
    s.put8(at(offset_of!(DriverShared, nrxqueue)), 1);
    s.put8(at(offset_of!(DriverShared, automask)), 0);
    s.put8(at(offset_of!(DriverShared, nintr)), plan.nintr);
    s.put8(at(offset_of!(DriverShared, evintr)), plan.event);
    s.put32(at(offset_of!(DriverShared, ictrl)), plan.ictrl);
    s.put32(at(offset_of!(DriverShared, rxmode)), RXMODE);
    s.put32(at(offset_of!(DriverShared, vlan_filter)), 1);
}

/// Write the TX then the RX queue descriptor at `base`.
pub(crate) fn write_queues(s: &mut impl Sink, base: usize, q: &QueueConfig, plan: IntrPlan) {
    let tx = |field: usize| base + field;
    s.put64(tx(offset_of!(TxQueueShared, cmd_ring)), q.tx_ring);
    s.put64(tx(offset_of!(TxQueueShared, data_ring)), q.tx_data);
    s.put64(tx(offset_of!(TxQueueShared, comp_ring)), q.tx_comp);
    s.put32(tx(offset_of!(TxQueueShared, cmd_ring_len)), q.tx_len);
    s.put32(tx(offset_of!(TxQueueShared, data_ring_len)), q.tx_len);
    s.put32(tx(offset_of!(TxQueueShared, comp_ring_len)), q.tx_len);
    s.put8(tx(offset_of!(TxQueueShared, intr_idx)), plan.tx);
    let rx = |field: usize| base + TXQ_LEN + field;
    for ring in 0..2 {
        s.put64(
            rx(offset_of!(RxQueueShared, cmd_ring) + ring * 8),
            q.rx_ring[ring],
        );
        s.put32(
            rx(offset_of!(RxQueueShared, cmd_ring_len) + ring * 4),
            q.rx_len[ring],
        );
    }
    s.put64(rx(offset_of!(RxQueueShared, comp_ring)), q.rx_comp);
    s.put32(rx(offset_of!(RxQueueShared, comp_ring_len)), q.rx_comp_len);
    s.put8(rx(offset_of!(RxQueueShared, intr_idx)), plan.rx);
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
