//! The system text clipboard (`wire::display::clipboard`).
//!
//! Only the owner of the surface with keyboard focus may set it or ask for it.
//! The text lives in one compositor Grant, registered at the first change and
//! rewritten at each. A Grant has one receiver at a time, so every offer shares
//! it again, read-only, with the owner it goes to. Each surface remembers the
//! serial it last saw (`SurfaceState::clipboard_seen`): focus moving to a
//! surface that has not seen the current text offers it once, and a setter
//! has seen what it set, so a peer echoing the text back cannot loop.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use wire::display::{ClipboardChanged, ClipboardGet, ClipboardSet, CLIPBOARD_MAX};

use crate::surface_table::SurfaceTable;

mod syscalls;
#[cfg(test)]
mod tests;

pub use syscalls::Syscalls;

/// The kernel calls the clipboard makes, a seam so its rules run on the host.
pub trait Kernel {
    /// Copy `dst.len()` bytes from the start of `grant`, which `owner` must own.
    fn copy_from(&mut self, grant: usize, owner: usize, dst: &mut [u8]) -> bool;
    /// Register a Grant of [`CLIPBOARD_MAX`] bytes owned by this task.
    fn register(&mut self) -> Option<usize>;
    /// Write `text` at the start of this task's `grant`.
    fn write(&mut self, grant: usize, text: &[u8]) -> bool;
    /// Share this task's `grant` read-only with `tid`, replacing any receiver.
    fn share(&mut self, grant: usize, tid: usize) -> bool;
    /// Send `frame` to `tid` without blocking; whether it was delivered.
    fn send(&mut self, tid: usize, frame: &[u8]) -> bool;
}

/// Keyboard focus: `(owner tid, surface cap)`, `None` for the console shell.
pub type Focus = Option<(usize, u64)>;

/// The clipboard text and where it lives.
pub struct Clipboard {
    text: Vec<u8>,
    /// Changes since boot, from 1; 0 while nothing was ever copied.
    serial: u32,
    /// The compositor's Grant holding `text`; 0 until the first change.
    grant: usize,
}

impl Clipboard {
    pub const fn new() -> Self {
        Self {
            text: Vec::new(),
            serial: 0,
            grant: 0,
        }
    }

    /// Take the text `sender` offers in `request`. Returns its length when it
    /// changed the clipboard; refused requests and identical text change
    /// nothing.
    pub fn set(
        &mut self,
        request: &ClipboardSet,
        sender: usize,
        focus: Focus,
        table: &mut SurfaceTable,
        kernel: &mut impl Kernel,
    ) -> Option<usize> {
        let cap = u64::from(request.cap);
        let len = request.len as usize;
        if !holds_focus(cap, sender, focus, table) || !(1..=CLIPBOARD_MAX).contains(&len) {
            return None;
        }
        let mut text = vec![0; len];
        if !kernel.copy_from(request.grant as usize, sender, &mut text)
            || core::str::from_utf8(&text).is_err()
        {
            return None;
        }
        let changed = text != self.text;
        if changed {
            let grant = self.grant(kernel)?;
            if !kernel.write(grant, &text) {
                return None;
            }
            self.text = text;
            self.serial = self.serial.wrapping_add(1).max(1);
        }
        mark_seen(table, cap, self.serial);
        changed.then_some(len)
    }

    /// Answer `request` with the current text when `sender` owns the focused
    /// surface it names; otherwise nothing. Returns whether it was sent.
    pub fn get(
        &mut self,
        request: &ClipboardGet,
        sender: usize,
        focus: Focus,
        table: &mut SurfaceTable,
        kernel: &mut impl Kernel,
    ) -> bool {
        let cap = u64::from(request.cap);
        holds_focus(cap, sender, focus, table) && self.offer(cap, sender, table, kernel)
    }

    /// Offer the text to the focused surface if it has not seen it yet. Run
    /// after anything that may move focus; an undelivered offer is retried at
    /// the next call.
    pub fn offer_to_focus(&self, focus: Focus, table: &mut SurfaceTable, kernel: &mut impl Kernel) {
        let Some((owner, cap)) = focus else {
            return;
        };
        let unseen = table
            .get(cap)
            .is_some_and(|surface| surface.clipboard_seen != self.serial);
        if self.serial != 0 && unseen && holds_focus(cap, owner, focus, table) {
            self.offer(cap, owner, table, kernel);
        }
    }

    /// Send `CLIPBOARD_CHANGED` for `cap` to its `owner`; the surface has seen
    /// the text once it is delivered.
    fn offer(
        &self,
        cap: u64,
        owner: usize,
        table: &mut SurfaceTable,
        kernel: &mut impl Kernel,
    ) -> bool {
        let grant = if self.serial == 0 {
            0
        } else if kernel.share(self.grant, owner) {
            self.grant
        } else {
            return false;
        };
        let event = ClipboardChanged {
            cap: cap as u32,
            grant: grant as u32,
            len: self.text.len() as u32,
            serial: self.serial,
        };
        if !kernel.send(owner, &event.encode()) {
            return false;
        }
        mark_seen(table, cap, self.serial);
        true
    }

    /// The compositor Grant, registered at the first change.
    fn grant(&mut self, kernel: &mut impl Kernel) -> Option<usize> {
        if self.grant == 0 {
            self.grant = kernel.register()?;
        }
        Some(self.grant)
    }
}

/// Whether `cap` is the focused surface and `sender` owns it.
fn holds_focus(cap: u64, sender: usize, focus: Focus, table: &SurfaceTable) -> bool {
    focus == Some((sender, cap))
        && table
            .get(cap)
            .is_some_and(|surface| surface.owner == sender)
}

fn mark_seen(table: &mut SurfaceTable, cap: u64, serial: u32) {
    if let Some(surface) = table.get_mut(cap) {
        surface.clipboard_seen = serial;
    }
}
