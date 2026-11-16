//! MSI-X wake-ups for bounded bring-up, link and TX-completion waits
//! (H-S3-4 b).
//!
//! An interrupt is only a hint to re-read the device: every park is bounded by
//! [`PARK_TICKS`], every return re-reads the condition, and every wait has its
//! own park budget, so a lost or revoked interrupt degrades to slow polling,
//! never to a hang. RX is not interrupt-driven: the net service asks for
//! frames (`OP_RX`), so chips keep their RX cause masked.

use abi::completion::source::IRQ;
use abi::pcie_function::IRQ_MODE_ALLOW_MSIX;
use ostd::io::print_fmt;
use ostd::ipc::{wait_any, Wake};
use ostd::syscall::sys_alloc_pcie_irq;
use types::ViResult;

/// Longest single park, in scheduler ticks (10 ms each).
pub const PARK_TICKS: u64 = 10;
/// Condition polls before the first park (QEMU completes TX synchronously).
const SPIN_BEFORE_PARK: u32 = 4_096;
/// Park expiries logged per cell lifetime.
const EXPIRY_LOG_LIMIT: u32 = 3;

/// How a controller's waits learn that something changed.
pub struct BoundedWait {
    tag: &'static str,
    /// MSI-X entries held, or `None` when polled.
    vectors: Option<u8>,
    /// Why the cell is polled (`irq=polled reason=…`).
    reason: &'static str,
    /// Parks ended by an interrupt / by their deadline (bind-line evidence).
    wakes: u32,
    expiries: u32,
}

impl BoundedWait {
    /// Ask 435 for `want` MSI-X entries of the owned function `bdf`. MSI is
    /// not accepted: per-queue causes need MSI-X. A refusal leaves the cell
    /// polled (`reason=alloc-refused`).
    pub fn setup(tag: &'static str, bdf: u32, want: usize) -> Self {
        let (vectors, reason) = match sys_alloc_pcie_irq(bdf, want, IRQ_MODE_ALLOW_MSIX) {
            Ok(alloc) => (Some(alloc.count), ""),
            Err(error) => {
                let _ = print_fmt(format_args!("{tag} MSI-X refused ({error:?}); polling\n"));
                (None, "alloc-refused")
            }
        };
        Self {
            tag,
            vectors,
            reason,
            wakes: 0,
            expiries: 0,
        }
    }

    /// MSI-X entries held (`None` when polled).
    pub fn vectors(&self) -> Option<u8> {
        self.vectors
    }

    /// Why the cell is polled (empty with MSI-X).
    pub fn reason(&self) -> &'static str {
        self.reason
    }

    /// Parks so far ended by an interrupt.
    pub fn wakes(&self) -> u32 {
        self.wakes
    }

    /// Parks so far ended by their deadline.
    pub fn expiries(&self) -> u32 {
        self.expiries
    }

    /// Wait (bounded) until `ready` holds: a short spin, then up to `parks`
    /// interrupt parks with MSI-X, or `polls` spins when polled. The device is
    /// re-read after every return. `Ok(false)` means the budget ran out.
    /// For devices that keep a vector armed on their own.
    pub fn wait_for(
        &mut self,
        parks: u32,
        polls: u32,
        ready: impl FnMut() -> ViResult<bool>,
    ) -> ViResult<bool> {
        self.wait::<false>(parks, polls, || Ok(()), ready)
    }

    /// As [`wait_for`](Self::wait_for), for devices whose causes must be
    /// re-armed: before every park `rearm` runs and the condition is
    /// re-checked, so a cause that lands afterwards raises a fresh message.
    pub fn wait_for_rearmed(
        &mut self,
        parks: u32,
        polls: u32,
        rearm: impl FnMut() -> ViResult<()>,
        ready: impl FnMut() -> ViResult<bool>,
    ) -> ViResult<bool> {
        self.wait::<true>(parks, polls, rearm, ready)
    }

    fn wait<const REARM: bool>(
        &mut self,
        parks: u32,
        polls: u32,
        mut rearm: impl FnMut() -> ViResult<()>,
        mut ready: impl FnMut() -> ViResult<bool>,
    ) -> ViResult<bool> {
        for _ in 0..SPIN_BEFORE_PARK {
            if ready()? {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        if self.vectors.is_some() {
            for _ in 0..parks {
                if REARM {
                    rearm()?;
                    if ready()? {
                        return Ok(true);
                    }
                }
                if !self.park() {
                    break;
                }
                if ready()? {
                    return Ok(true);
                }
            }
            if self.vectors.is_some() {
                return Ok(false);
            }
        }
        for _ in 0..polls {
            if ready()? {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        Ok(false)
    }

    /// Park on this task's MSI-X lines for at most [`PARK_TICKS`]. `false`
    /// means the wait was refused and the cell is now polled.
    fn park(&mut self) -> bool {
        let tag = self.tag;
        match wait_any(IRQ, PARK_TICKS) {
            Ok(Wake::Irq(_)) => {
                self.wakes += 1;
                true
            }
            Ok(_) => {
                self.expiries += 1;
                if self.expiries <= EXPIRY_LOG_LIMIT {
                    let _ = print_fmt(format_args!(
                        "{tag} irq wait expired after {PARK_TICKS} ticks; re-reading the device\n"
                    ));
                }
                true
            }
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "{tag} irq=polled reason=wait-refused ({error:?})\n"
                ));
                self.vectors = None;
                self.reason = "wait-refused";
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::ViError;

    fn polled() -> BoundedWait {
        BoundedWait {
            tag: "[test]",
            vectors: None,
            reason: "alloc-refused",
            wakes: 0,
            expiries: 0,
        }
    }

    #[test]
    fn polled_wait_spins_then_polls_within_budget() {
        let mut wait = polled();
        let mut calls = 0u32;
        let hit = SPIN_BEFORE_PARK + 10;
        let ready = wait
            .wait_for(50, 100, || {
                calls += 1;
                Ok(calls == hit)
            })
            .unwrap();
        assert!(ready);
        assert_eq!(calls, hit);

        let mut calls = 0u32;
        let ready = wait
            .wait_for_rearmed(
                50,
                100,
                || panic!("polled waits never re-arm"),
                || {
                    calls += 1;
                    Ok(false)
                },
            )
            .unwrap();
        assert!(!ready, "budget exhausted");
        assert_eq!(calls, SPIN_BEFORE_PARK + 100);
        assert_eq!((wait.wakes(), wait.expiries()), (0, 0));
    }

    #[test]
    fn device_errors_end_the_wait() {
        let mut wait = polled();
        assert_eq!(wait.wait_for(1, 1, || Err(ViError::IO)), Err(ViError::IO));
    }
}
