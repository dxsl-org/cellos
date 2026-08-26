//! TLS 1.3 support for the net service cell (embedded-tls 0.19).
//!
//! Build flavors (mutually exclusive cargo features):
//!   tls-roots-embedded — single pinned CA via pki::CertVerifier (G1 default)
//!   tls-roots-full     — rustls-webpki multi-root (G2, deferred P04)
//!   tls-insecure       — UnsecureProvider, no verification (dev/lab only)
//!
//! Module layout:
//!   clock    — ViTlsClock (fail-closed pending protected authenticated time)
//!   roots    — ca_cert() single trust anchor, cfg-selected by tls-ca-* feature
//!   provider — ViTlsProvider (CryptoProvider with infallible verifier())
//!   rng      — ViRng (VirtIO-RNG-backed ChaCha20)
//!   transport — SmoltcpTlsTransport (embedded-io Read+Write over smoltcp TCP)
//!   socket   — TlsSocketEntry (per-connection TLS state + handshake)
//!   block_on — blocking executor shim

pub mod block_on;
pub mod clock;
pub mod provider;
pub mod relay_certificate;
pub mod relay_profile;
pub mod rng;
pub mod roots;
pub mod socket;
pub mod transport;
