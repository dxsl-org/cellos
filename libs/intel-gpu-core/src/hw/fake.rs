//! A recording [`GpuHw`] for host tests: every device effect becomes an [`Event`] in call
//! order, exports come from a table the test fills, and bans or submit failures are
//! injected per queue.

use super::{ExportError, ExportMapper, ExportRecord, GpuHw, HwQueue, HwVm};
use crate::backend::XeError;
use crate::pte::PatIndex;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    MapExport(u64),
    Revoke(u64),
    VmCreate(HwVm),
    Map {
        vm: HwVm,
        va: u64,
        dma: u64,
        len: u64,
        writable: bool,
    },
    MapNull {
        vm: HwVm,
        va: u64,
        len: u64,
    },
    Unmap {
        vm: HwVm,
        va: u64,
        len: u64,
    },
    VmDestroy(HwVm),
    QueueCreate(HwQueue, HwVm),
    Submit(HwQueue, u64),
    QueueDestroy(HwQueue),
}

#[derive(Default)]
pub struct RecordingHw {
    pub events: Vec<Event>,
    pub exports: BTreeMap<u64, ExportRecord>,
    pub bans: BTreeMap<HwQueue, u32>,
    /// Queues whose ring is full: submissions answer `TryAgain`.
    pub full: BTreeSet<HwQueue>,
    /// `vm_map` fails with `OutOfMemory` once this many maps succeeded.
    pub map_budget: Option<usize>,
    next_vm: u32,
    next_queue: u32,
}

impl RecordingHw {
    /// An export of `len` bytes at `iova` owned by `(cell, generation)`.
    pub fn add_export(
        &mut self,
        id: u64,
        cell: u64,
        generation: u64,
        iova: u64,
        len: u64,
        write: bool,
    ) {
        self.exports.insert(
            id,
            ExportRecord {
                export_id: id,
                owner_cell_id: cell,
                owner_generation: generation,
                iova,
                len,
                device_write: write,
            },
        );
    }

    pub fn count(&self, pred: impl Fn(&Event) -> bool) -> usize {
        self.events.iter().filter(|e| pred(e)).count()
    }

    pub fn position(&self, pred: impl Fn(&Event) -> bool) -> Option<usize> {
        self.events.iter().position(pred)
    }
}

impl ExportMapper for RecordingHw {
    fn map_export(&mut self, export_id: u64) -> Result<ExportRecord, ExportError> {
        self.events.push(Event::MapExport(export_id));
        self.exports
            .get(&export_id)
            .copied()
            .ok_or(ExportError::NotFound)
    }

    fn revoke_export(&mut self, export_id: u64) -> Result<(), ExportError> {
        self.events.push(Event::Revoke(export_id));
        self.exports
            .remove(&export_id)
            .map(|_| ())
            .ok_or(ExportError::NotFound)
    }
}

impl GpuHw for RecordingHw {
    fn vm_create(&mut self) -> Result<HwVm, XeError> {
        self.next_vm += 1;
        let vm = HwVm(self.next_vm);
        self.events.push(Event::VmCreate(vm));
        Ok(vm)
    }

    fn vm_map(
        &mut self,
        vm: HwVm,
        va: u64,
        dma: u64,
        len: u64,
        _pat: PatIndex,
        writable: bool,
    ) -> Result<(), XeError> {
        if let Some(budget) = &mut self.map_budget {
            if *budget == 0 {
                return Err(XeError::OutOfMemory);
            }
            *budget -= 1;
        }
        self.events.push(Event::Map {
            vm,
            va,
            dma,
            len,
            writable,
        });
        Ok(())
    }

    fn vm_map_null(&mut self, vm: HwVm, va: u64, len: u64, _pat: PatIndex) -> Result<(), XeError> {
        self.events.push(Event::MapNull { vm, va, len });
        Ok(())
    }

    fn vm_unmap(&mut self, vm: HwVm, va: u64, len: u64) {
        self.events.push(Event::Unmap { vm, va, len });
    }

    fn vm_destroy(&mut self, vm: HwVm) {
        self.events.push(Event::VmDestroy(vm));
    }

    fn queue_create(&mut self, vm: HwVm, _engine_class: u16) -> Result<HwQueue, XeError> {
        self.next_queue += 1;
        let q = HwQueue(self.next_queue);
        self.events.push(Event::QueueCreate(q, vm));
        Ok(q)
    }

    fn queue_submit(&mut self, queue: HwQueue, batch_va: u64) -> Result<(), XeError> {
        if self.full.contains(&queue) {
            return Err(XeError::TryAgain);
        }
        self.events.push(Event::Submit(queue, batch_va));
        Ok(())
    }

    fn queue_ban(&mut self, queue: HwQueue) -> u32 {
        self.bans.get(&queue).copied().unwrap_or(0)
    }

    fn queue_destroy(&mut self, queue: HwQueue) {
        self.events.push(Event::QueueDestroy(queue));
    }
}
