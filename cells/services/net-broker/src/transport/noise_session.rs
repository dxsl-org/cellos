use super::{tcp_read_msg, tcp_write_msg, StaticKeypair, HEARTBEAT_MS, NOISE_MSG_BUF, NOISE_TAG};
use crate::rng::BrokerRng;
use alloc::boxed::Box;
use api::cluster::CellNetId;
use clatter::{
    crypto::{cipher::ChaChaPoly, hash::Sha256},
    handshakepattern::noise_kk_psk0,
    traits::{Dh, Handshaker},
    transportstate::TransportState,
    KeyPair, NqHandshakeCore,
};
use ostd::service::NetRef;
use ostd::{syscall::sys_heartbeat, ViError, ViResult};
use service_net_broker::kms_dh::KmsBackedX25519;
use service_net_broker::noise_identity::handshake_prologue;

type Hs = NqHandshakeCore<KmsBackedX25519, ChaChaPoly, Sha256, BrokerRng>;
type Ts = TransportState<ChaChaPoly, Sha256>;

enum Phase {
    Handshake(Box<Hs>),
    Transport(Box<Ts>),
    Finalizing,
}

/// A single KKpsk0 session over a net-cell TCP socket.
pub struct NoiseSession {
    phase: Phase,
    pub cap_id: u32,
    pub cluster_id: u64,
}

impl NoiseSession {
    /// Construct a session whose cluster and ordered peer identities are bound
    /// into the Noise prologue.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rng: &mut BrokerRng,
        psk: &[u8; 32],
        my_static: &StaticKeypair,
        peer_static_pub: [u8; 32],
        cluster_id: u64,
        local_node_id: &CellNetId,
        remote_node_id: &CellNetId,
        cap_id: u32,
        is_initiator: bool,
    ) -> ViResult<Self> {
        let ephemeral = KmsBackedX25519::genkey_rng(rng).map_err(|_| ViError::IO)?;
        let prologue = handshake_prologue(
            cluster_id,
            &local_node_id.0,
            &remote_node_id.0,
            is_initiator,
        );
        let mut hs = NqHandshakeCore::<KmsBackedX25519, ChaChaPoly, Sha256, BrokerRng>::new(
            noise_kk_psk0(),
            prologue.as_slice(),
            is_initiator,
            Some(KeyPair {
                public: my_static.inner.public,
                secret: my_static.inner.secret.clone(),
            }),
            Some(ephemeral),
            Some(peer_static_pub),
            None,
        )
        .map_err(|_| ViError::InvalidArgument)?;
        hs.push_psk(psk);
        Ok(Self {
            phase: Phase::Handshake(Box::new(hs)),
            cap_id,
            cluster_id,
        })
    }

    /// Drive the two-message KKpsk0 handshake to completion.
    pub fn do_handshake(&mut self, net: &mut NetRef) -> ViResult<()> {
        let mut buf = [0u8; NOISE_MSG_BUF];
        let cap_id = self.cap_id;
        let is_init = match &self.phase {
            Phase::Handshake(hs) => hs.is_initiator(),
            _ => return Ok(()),
        };
        if is_init {
            sys_heartbeat(HEARTBEAT_MS);
            let n = self
                .handshake_mut()?
                .write_message(&[], &mut buf)
                .map_err(|_| ViError::IO)?;
            tcp_write_msg(net, cap_id, &buf[..n])?;
            sys_heartbeat(HEARTBEAT_MS);
            let n = tcp_read_msg(net, cap_id, &mut buf)?;
            self.handshake_mut()?
                .read_message(&buf[..n], &mut [])
                .map_err(|_| ViError::IO)?;
        } else {
            sys_heartbeat(HEARTBEAT_MS);
            let n = tcp_read_msg(net, cap_id, &mut buf)?;
            self.handshake_mut()?
                .read_message(&buf[..n], &mut [])
                .map_err(|_| ViError::IO)?;
            sys_heartbeat(HEARTBEAT_MS);
            let n = self
                .handshake_mut()?
                .write_message(&[], &mut buf)
                .map_err(|_| ViError::IO)?;
            tcp_write_msg(net, cap_id, &buf[..n])?;
        }
        let old = core::mem::replace(&mut self.phase, Phase::Finalizing);
        let hs = match old {
            Phase::Handshake(handshake) => *handshake,
            _ => return Err(ViError::IO),
        };
        self.phase = Phase::Transport(Box::new(TransportState::new(hs).map_err(|_| ViError::IO)?));
        Ok(())
    }

    /// Encrypt and send one length-prefixed Noise transport record.
    pub fn send(&mut self, net: &mut NetRef, plaintext: &[u8]) -> ViResult<()> {
        let mut out = [0u8; 4096 + NOISE_TAG];
        let n = match &mut self.phase {
            Phase::Transport(state) => state.send(plaintext, &mut out).map_err(|_| ViError::IO)?,
            _ => return Err(ViError::NotSupported),
        };
        tcp_write_msg(net, self.cap_id, &out[..n])
    }

    /// Receive and decrypt one Noise transport record.
    pub fn recv(&mut self, net: &mut NetRef, out: &mut [u8]) -> ViResult<usize> {
        let mut buf = [0u8; 4096 + NOISE_TAG];
        let n = tcp_read_msg(net, self.cap_id, &mut buf)?;
        match &mut self.phase {
            Phase::Transport(state) => state.receive(&buf[..n], out).map_err(|_| ViError::IO),
            _ => Err(ViError::NotSupported),
        }
    }

    fn handshake_mut(&mut self) -> ViResult<&mut Hs> {
        match &mut self.phase {
            Phase::Handshake(handshake) => Ok(handshake.as_mut()),
            _ => Err(ViError::IO),
        }
    }
}
