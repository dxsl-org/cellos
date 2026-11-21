use alloc::vec;

use super::net_buf::{RxBuffer, TxBuffer};
use super::{EthernetAddress, VirtIONetRaw, NET_HDR_SIZE};
use crate::{hal::Hal, transport::Transport, Error, Result};

/// Driver for a VirtIO network device.
///
/// Unlike [`VirtIONetRaw`], it uses [`RxBuffer`]s for transmission and
/// reception rather than the raw slices. On initialization, it pre-allocates
/// all receive buffers and puts them all in the receive queue.
///
/// The virtio network device is a virtual ethernet card.
///
/// It has enhanced rapidly and demonstrates clearly how support for new
/// features are added to an existing device.
/// Empty buffers are placed in one virtqueue for receiving packets, and
/// outgoing packets are enqueued into another for transmission in that order.
/// A third command queue is used to control advanced filtering features.
pub struct VirtIONet<H: Hal, T: Transport, const QUEUE_SIZE: usize> {
    inner: VirtIONetRaw<H, T, QUEUE_SIZE>,
    rx_buffers: [Option<RxBuffer>; QUEUE_SIZE],
}

impl<H: Hal, T: Transport, const QUEUE_SIZE: usize> VirtIONet<H, T, QUEUE_SIZE> {
    /// Create a new VirtIO-Net driver.
    pub fn new(transport: T, buf_len: usize) -> Result<Self> {
        let mut inner = VirtIONetRaw::new(transport)?;

        const NONE_BUF: Option<RxBuffer> = None;
        let mut rx_buffers = [NONE_BUF; QUEUE_SIZE];
        for (i, rx_buf_place) in rx_buffers.iter_mut().enumerate() {
            let mut rx_buf = RxBuffer::new(i, buf_len);
            // Safe because the buffer lives as long as the queue.
            let token = unsafe { inner.receive_begin(rx_buf.as_bytes_mut())? };
            assert_eq!(token, i as u16);
            *rx_buf_place = Some(rx_buf);
        }

        Ok(VirtIONet { inner, rx_buffers })
    }

    /// Acknowledge interrupt.
    pub fn ack_interrupt(&mut self) -> bool {
        self.inner.ack_interrupt()
    }

    /// Disable interrupts.
    pub fn disable_interrupts(&mut self) {
        self.inner.disable_interrupts()
    }

    /// Enable interrupts.
    pub fn enable_interrupts(&mut self) {
        self.inner.enable_interrupts()
    }

    /// Get MAC address.
    pub fn mac_address(&self) -> EthernetAddress {
        self.inner.mac_address()
    }

    /// Whether can send packet.
    pub fn can_send(&self) -> bool {
        self.inner.can_send()
    }

    /// Whether can receive packet.
    pub fn can_recv(&self) -> bool {
        self.inner.poll_receive().is_some()
    }

    /// Receives a [`RxBuffer`] from network. If currently no data, returns an
    /// error with type [`Error::NotReady`].
    ///
    /// It will try to pop a buffer that completed data reception in the
    /// NIC queue.
    pub fn receive(&mut self) -> Result<RxBuffer> {
        if let Some(token) = self.inner.poll_receive() {
            // The token is the device's used-ring id: never index with it unchecked.
            let mut rx_buf = self
                .rx_buffers
                .get_mut(usize::from(token))
                .and_then(Option::take)
                .ok_or(Error::WrongToken)?;
            if token != rx_buf.idx {
                return Err(Error::WrongToken);
            }

            // Safe because `token` == `rx_buf.idx`, we are passing the same
            // buffer as we passed to `VirtQueue::add` and it is still valid.
            let (_hdr_len, pkt_len) =
                unsafe { self.inner.receive_complete(token, rx_buf.as_bytes_mut())? };
            // The used length is device-reported; the packet cannot extend past the buffer.
            let room = rx_buf.as_bytes().len().saturating_sub(NET_HDR_SIZE);
            rx_buf.set_packet_len(pkt_len.min(room));
            Ok(rx_buf)
        } else {
            Err(Error::NotReady)
        }
    }

    /// Gives back the ownership of `rx_buf`, and recycles it for next use.
    ///
    /// It will add the buffer back to the NIC queue.
    pub fn recycle_rx_buffer(&mut self, mut rx_buf: RxBuffer) -> Result {
        // Safe because we take the ownership of `rx_buf` back to `rx_buffers`,
        // it lives as long as the queue.
        let new_token = unsafe { self.inner.receive_begin(rx_buf.as_bytes_mut()) }?;
        // `rx_buffers[new_token]` is expected to be `None` since it was taken
        // away at `Self::receive()` and has not been added back.
        if self.rx_buffers[new_token as usize].is_some() {
            return Err(Error::WrongToken);
        }
        rx_buf.idx = new_token;
        self.rx_buffers[new_token as usize] = Some(rx_buf);
        Ok(())
    }

    /// Allocate a new buffer for transmitting.
    pub fn new_tx_buffer(&self, buf_len: usize) -> TxBuffer {
        TxBuffer(vec![0; buf_len])
    }

    /// Sends a [`TxBuffer`] to the network, and blocks until the request
    /// completed.
    pub fn send(&mut self, tx_buf: TxBuffer) -> Result {
        self.inner.send(tx_buf.packet())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        device::net::{Config, Status, QUEUE_RECEIVE},
        hal::fake::FakeHal,
        transport::{
            fake::{FakeTransport, QueueStatus, State},
            DeviceType,
        },
        volatile::ReadOnly,
    };
    use alloc::sync::Arc;
    use core::ptr::NonNull;
    use std::sync::Mutex;

    const QUEUE_SIZE: usize = 4;
    const BUF_LEN: usize = 2048;

    fn config() -> Config {
        Config {
            mac: ReadOnly::new([0x52, 0x54, 0, 0x12, 0x34, 0x56]),
            status: ReadOnly::new(Status::empty()),
            max_virtqueue_pairs: ReadOnly::new(1),
            mtu: ReadOnly::new(1500),
        }
    }

    fn device(
        config: &mut Config,
    ) -> (
        VirtIONet<FakeHal, FakeTransport<Config>, QUEUE_SIZE>,
        Arc<Mutex<State>>,
    ) {
        let state = Arc::new(Mutex::new(State {
            queues: vec![QueueStatus::default(), QueueStatus::default()],
            ..Default::default()
        }));
        let transport = FakeTransport {
            device_type: DeviceType::Network,
            max_queue_size: QUEUE_SIZE as u32,
            device_features: 0,
            config_space: NonNull::from(config),
            state: state.clone(),
        };
        (VirtIONet::new(transport, BUF_LEN).unwrap(), state)
    }

    /// 07 S-10: a used-ring id past the queue is refused, not used as an index.
    #[test]
    fn receive_refuses_a_used_id_outside_the_queue() {
        let mut config = config();
        let (mut net, state) = device(&mut config);
        state
            .lock()
            .unwrap()
            .write_to_queue_reporting::<QUEUE_SIZE>(
                QUEUE_RECEIVE,
                &[0xAB; 64],
                0xFFFF,
                (NET_HDR_SIZE + 64) as u32,
            );
        assert!(matches!(net.receive(), Err(Error::WrongToken)));
    }

    /// 07 S-10: a used length past the buffer yields a packet clamped to the buffer.
    #[test]
    fn receive_clamps_a_used_length_past_the_buffer() {
        let mut config = config();
        let (mut net, state) = device(&mut config);
        state
            .lock()
            .unwrap()
            .write_to_queue_reporting::<QUEUE_SIZE>(QUEUE_RECEIVE, &[0xAB; 64], 0, 0x10000);
        let rx = net.receive().ok().expect("clamped packet");
        assert_eq!(rx.packet_len(), BUF_LEN - NET_HDR_SIZE);
        assert_eq!(rx.packet().len(), BUF_LEN - NET_HDR_SIZE);
        // The fake device wrote the 64 bytes from the start of the buffer (header included).
        assert_eq!(rx.packet()[..4], [0xAB; 4]);
    }
}
