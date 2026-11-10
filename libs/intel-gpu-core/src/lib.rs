#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! Hardware-independent logic for the Intel integrated-GPU driver Cell
//! (ADR-0033, plan `.agents/261104-0801-x86-igpu-compute/`).
//!
//! Everything here is pure computation over byte/word buffers, so it is
//! host-tested before any machine exists. MMIO, DMA and syscalls live in the
//! driver Cell, not here; the `XE_GPU` IPC frames ([`wire`]) and their per-client
//! server ([`server`]) are byte codecs shared by the driver and the Mesa shim.
//!
//! Behaviour follows the MIT-licensed Linux `xe` driver; each module names the
//! upstream file it mirrors. No firmware is parsed beyond its public CSS header.

extern crate alloc;

pub mod backend;
pub mod ct;
pub mod guc_fw;
pub mod ppgtt;
pub mod pte;
pub mod server;
pub mod syncobj;
pub mod uapi;
pub mod wire;
