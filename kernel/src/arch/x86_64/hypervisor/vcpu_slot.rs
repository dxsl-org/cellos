//! One x86 vCPU of a VM, and the interrupts vCPUs send each other.
//!
//! The thread that runs a vCPU holds its [`VcpuSlot::vcpu`] lock for a whole
//! run — until the guest exits to the VMM, a host interrupt arrives, or the
//! run's budget ends — and only that lock: the VM registry is released before
//! the guest is entered, so other vCPUs of the VM run at the same time on
//! other CPUs. Other threads reach a running vCPU only through its mailbox
//! ([`VcpuShared`]) and a host interrupt that ends its time in the guest
//! ([`kick`]).

use hal::x86_64::vmx_vcpu::{Ipi, IpiKind, IpiTarget, VcpuShared};

use super::svm_registry::X86Vcpu;
use crate::sync::Spinlock;

/// A VM's vCPUs by index; a runner holds a reference across its run.
pub(super) type Vcpus = alloc::sync::Arc<[alloc::sync::Arc<VcpuSlot>]>;

pub(super) struct VcpuSlot {
    /// Instance lock (unranked): held by the vCPU's runner across a run, by
    /// `VcpuRegs` and by an SVM `InjectIrq`, never while the registry is
    /// taken.
    pub(super) vcpu: Spinlock<X86Vcpu>,
    /// Mailbox and running CPU (VMX).
    pub(super) shared: VcpuShared,
    pub(super) vmx: bool,
}

impl VcpuSlot {
    pub(super) fn new(vcpu: X86Vcpu) -> Self {
        let vmx = matches!(vcpu, X86Vcpu::Vmx(_));
        Self {
            vcpu: Spinlock::new(vcpu),
            shared: VcpuShared::default(),
            vmx,
        }
    }
}

/// Make a running vCPU see new mail: a host interrupt on the CPU it runs on
/// ends its time in the guest (external-interrupt exiting), and its next
/// entry takes the mail. A vCPU between runs takes it at its next run.
pub(super) fn kick(shared: &VcpuShared) {
    if let Some(cpu) = shared.running_on() {
        crate::sched::smp::send_ipi(cpu);
    }
}

/// Deliver an interrupt the local APIC of vCPU `sender` (0-based, its APIC
/// ID) sent, to every other vCPU of `vcpus` it addresses. A vCPU's APIC ID is
/// its index.
pub(super) fn deliver_ipi(vcpus: &Vcpus, sender: usize, ipi: Ipi) {
    for (index, slot) in vcpus.iter().enumerate() {
        let addressed = index != sender
            && match ipi.target {
                IpiTarget::Physical(id) => usize::from(id) == index,
                IpiTarget::Logical(mask) => slot.shared.logical_id() & mask != 0,
                IpiTarget::AllButSelf => true,
            };
        if !addressed {
            continue;
        }
        match ipi.kind {
            IpiKind::Fixed(vector) => slot.shared.post_ipi(vector),
            IpiKind::Init => slot.shared.post_init(),
            IpiKind::StartUp(vector) => slot.shared.post_sipi(vector),
        }
        kick(&slot.shared);
    }
}
