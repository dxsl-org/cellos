//! DICE-style CDI derivation + a fixed-width internal attestation token.
//!
//! This is the **entire CI-testable slice** of the Cellos attestation chain
//! (Mythos analysis dossier-4, decisions 1+2): the derivation math and the token
//! wire format need no root-of-trust hardware — synthetic aggregates exercise
//! them in tests exactly as the real boot-time measurement aggregate will later
//! (P01) and the real Silo-backed root will later (P02). Zero call-site change
//! when those land — see [`token::encode`]'s `sign_fn` seam.
//!
//! `#![no_std]` (host `std` only under `#[cfg(test)]`, matching `libs/http-core`
//! and `libs/types`) and no `alloc` — every value here is a fixed-size array.
#![cfg_attr(not(test), no_std)]

pub mod cdi;
pub mod hkdf;
pub mod sha256;
pub mod token;

pub use cdi::{derive_cdi, derive_chain};
pub use token::{AttestBody, AttestError};
