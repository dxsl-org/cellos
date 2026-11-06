//! Which transport the network backend uses for an endpoint.
//!
//! WHY: the network backend's reply is parsed for `TOOL_CALL:` lines, so whoever
//! controls the bytes on the wire proposes the agent's next action. Plaintext
//! HTTP hands that to anyone on path. TLS (through service-net, which verifies
//! the server chain and SNI) is therefore the default for every endpoint.
//!
//! The single exception is opt-in and lab-only: with the `qemu-plaintext-llm`
//! feature, the QEMU user-network host alias [`QEMU_SLIRP_HOST`] may be reached
//! in plaintext, because there the "network" is QEMU's in-process NAT to the
//! developer's own machine (`tools/hypha-mock-llm`). A cell cannot tell QEMU from
//! a real LAN where 10.0.2.2 is an arbitrary host, which is why this is a build
//! choice rather than a runtime guess. Tool calls stay operator-confirmed either
//! way (`core`'s `tool_gate`).

/// QEMU user-mode networking's alias for the host (`-netdev user` default).
pub const QEMU_SLIRP_HOST: [u8; 4] = [10, 0, 2, 2];
/// Ports the host-side mock serves (`mock_proxy.py`, `--plain`).
pub const TLS_PORT: u16 = 8443;
pub const PLAINTEXT_PORT: u16 = 8080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Tls,
    Plaintext,
}

impl Transport {
    pub const fn port(self) -> u16 {
        match self {
            Transport::Tls => TLS_PORT,
            Transport::Plaintext => PLAINTEXT_PORT,
        }
    }

    pub const fn is_tls(self) -> bool {
        matches!(self, Transport::Tls)
    }
}

/// TLS unless the build opted into QEMU plaintext *and* `ip` is the QEMU host
/// alias. Every other endpoint is TLS whatever the build says.
pub const fn select_transport(ip: [u8; 4], qemu_plaintext: bool) -> Transport {
    let slirp_host = ip[0] == QEMU_SLIRP_HOST[0]
        && ip[1] == QEMU_SLIRP_HOST[1]
        && ip[2] == QEMU_SLIRP_HOST[2]
        && ip[3] == QEMU_SLIRP_HOST[3];
    if qemu_plaintext && slirp_host {
        Transport::Plaintext
    } else {
        Transport::Tls
    }
}

#[cfg(test)]
#[path = "endpoint-tests.rs"]
mod tests;
