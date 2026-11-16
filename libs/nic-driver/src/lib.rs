//! Shared skeleton of the Ethernet Driver Cells (Phase 18, census §1.8).
//!
//! Every NIC cell (`igc`, `ixgbe`, `i40e`, `r8169`, `bcm57xx`, `e1000`,
//! `e1000e`, `igb`, `vmxnet3`) runs the same life cycle. This crate owns it:
//!
//! 1. **Probe**: name the controller exactly with `FindPcieDeviceByVendor`
//!    (424) over the chip's ID list, for a bounded window ([`FIND_ATTEMPTS`]
//!    rounds), counting only `Ok(true)` as found.
//! 2. **Identity gate**: [`NicDevice::classify`] refuses any ID the chip code
//!    cannot drive before MMIO is touched (exit 0, "driver gate closed").
//! 3. **Claim and init**: [`NicDevice::init`] claims its BARs through
//!    [`Probe`] and brings the controller up. DMA addresses are programmed only
//!    after [`dma::DmaSet::alloc_authorized`] has authorized every slot.
//! 4. **Publish, then register**: the device is published in the cell's
//!    [`State`] before `RegisterNicDriver` (417), because the kernel may route
//!    requests as soon as the syscall returns. A refused registration drops
//!    the device and exits 1.
//! 5. **Serve**: only the live `service::NET` provider may drive the NIC; the
//!    NIC wire ([`wire::proto::nic`]) is dispatched here, unchanged.
//! 6. **Shutdown**: `Shutdown`/`ShutdownWith` exit 0.
//!
//! A chip supplies its register model, PHY, reset, identity table,
//! descriptor formats and programming order by implementing [`NicDevice`],
//! then expands [`cell!`]. `declare_manifest!` and `declare_syscalls!` stay in
//! each cell, because their signed allowlists differ.
//!
//! Law 4 exception: `unsafe` here is confined to fixed-array construction
//! ([`dma::try_init_array`]) and to volatile access to DMA pages the cell owns
//! ([`ring`]); every access is bounds-checked against the page first.

#![cfg_attr(not(test), no_std)]

mod cell;
pub mod dma;
pub mod irq;
mod probe;
pub mod regs;
pub mod ring;

pub use cell::{on_event, State, FIND_ATTEMPTS, FRAME_MAX};
pub use probe::Probe;

use types::ViResult;

/// One Ethernet controller family, as the shared cell skeleton drives it.
pub trait NicDevice: Sized + Send + 'static {
    /// Log tag, brackets included (`"[igc]"`). Every line the skeleton prints
    /// starts with it.
    const TAG: &'static str;
    /// PCI vendor ID passed to `FindPcieDeviceByVendor`.
    const VENDOR: u16;
    /// Device IDs queried, in order. Empty means no part is admitted yet: the
    /// probe finds nothing and the cell reports itself absent.
    const QUERY_IDS: &'static [u16];
    /// Text printed after the tag when no controller is found, or `None` to
    /// exit silently (`/bin/e1000`, whose tag sibling lanes assert absent).
    const ABSENT_LINE: Option<&'static str>;
    /// Largest frame accepted by `OP_TX` and returned by `OP_RX`
    /// (at most [`FRAME_MAX`]).
    const MAX_TX: usize = FRAME_MAX;

    /// What [`classify`](Self::classify) decides about the part.
    type Identity: Copy;
    /// Why [`init`](Self::init) failed; printed with `{:?}`.
    type Error: core::fmt::Debug;

    /// The identity of `vendor:device`, or `None` to fail closed.
    fn classify(vendor: u16, device: u16) -> Option<Self::Identity>;
    /// Part name for the bind line.
    fn identity_name(id: &Self::Identity) -> &'static str;
    /// Claim the BARs through `probe` and bring the controller up.
    fn init(probe: &Probe, id: Self::Identity) -> Result<Self, Self::Error>;
    /// Text after `"{TAG} controller bound vvvv:dddd {name} "` on the bind
    /// line. Kept byte-exact: lanes assert it.
    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;
    /// Station address served by `OP_GETMAC`.
    fn mac(&self) -> [u8; 6];
    /// Transmit one frame of 1..=[`MAX_TX`](Self::MAX_TX) bytes.
    fn send(&mut self, frame: &[u8]) -> ViResult<()>;
    /// Copy the next received frame into `out`; 0 when none is ready.
    fn recv(&mut self, out: &mut [u8]) -> usize;
}

/// `aa:bb:cc:dd:ee:ff`, for bind lines.
pub struct MacFmt(pub [u8; 6]);

impl core::fmt::Display for MacFmt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

/// Expand the cell entry point for the [`NicDevice`] type `$device`: the
/// cell's device state, its event handler and `ostd::run_app!`.
#[macro_export]
macro_rules! cell {
    ($device:ty) => {
        static NIC_STATE: $crate::State<$device> = $crate::State::new();

        fn nic_event(_ctx: &mut $crate::__private::AppContext, event: $crate::__private::AppEvent) {
            $crate::on_event(&NIC_STATE, event);
        }

        $crate::__private::run_app!(nic_event);
    };
}

#[doc(hidden)]
pub mod __private {
    pub use ostd::app::{AppContext, AppEvent};
    pub use ostd::run_app;
}

#[cfg(test)]
mod tests {
    use super::MacFmt;

    #[test]
    fn mac_is_lowercase_colon_separated() {
        let text = format!("{}", MacFmt([0x52, 0x54, 0x00, 0x12, 0x34, 0xAB]));
        assert_eq!(text, "52:54:00:12:34:ab");
    }
}
