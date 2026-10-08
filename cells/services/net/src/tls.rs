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
//!   transport — nonblocking embedded-io-async smoltcp TCP transport
//!   socket   — owned TLS state and async handshake
//!   dispatch — deferred IPC operation scheduler

pub mod clock;
pub mod dispatch;
pub mod provider;
#[cfg(test)]
pub mod relay_certificate;
#[cfg(test)]
pub mod relay_profile;
pub mod rng;
pub mod roots;
pub mod socket;
pub mod transport;

#[cfg(test)]
#[path = "tls/authenticated-time-precheck-tests.rs"]
pub(crate) mod authenticated_time_precheck_tests;
