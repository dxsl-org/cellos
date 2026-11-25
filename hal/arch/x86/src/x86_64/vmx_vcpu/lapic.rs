// SPDX-License-Identifier: MIT
//! The guest's local APIC (SDM Vol. 3A ch. 11): the register page, the
//! request and in-service registers with their priority rules, EOI, the timer
//! in TSC-deadline mode, and interrupts between processors (fixed, INIT and
//! start-up).
//!
//! The guest reaches it through the page at [`PAGE`], which the nested page
//! table leaves unmapped so that every access exits, and through
//! `IA32_TSC_DEADLINE`. With a timer here the guest needs no tick from its
//! VMM: it arms a deadline, the run timer ends the run when it is due, and the
//! interrupt goes in from the kernel. An interrupt for another processor
//! leaves as an [`Ipi`], which the kernel delivers to that vCPU.
//!
//! Not modelled: the count-down timer modes (a guest that does not use the
//! TSC deadline gets no APIC timer interrupt, as before this model), NMI, SMI
//! and ExtINT interrupts between processors (dropped), the cluster model of
//! logical destinations (the flat model is), and the thermal,
//! performance-counter and error sources (nothing raises them; their LVT
//! entries only hold what the guest writes).

/// Guest-physical page of the registers.
pub(super) const PAGE: core::ops::Range<u64> = 0xFEE0_0000..0xFEE0_1000;
pub(super) const IA32_TSC_DEADLINE: u32 = 0x6E0;

/// Where an interrupt for other processors goes. The sender itself is never
/// in the set: its own share was delivered by its APIC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiTarget {
    /// The processor with this APIC ID.
    Physical(u8),
    /// Every processor whose logical ID (flat model) shares a bit with this.
    Logical(u8),
    AllButSelf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiKind {
    Fixed(u8),
    Init,
    /// Start-up: the processor begins in real mode at `vector << 12`.
    StartUp(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipi {
    pub target: IpiTarget,
    pub kind: IpiKind,
}

/// What CPUID says about the processor whose APIC ID is `id`, one of
/// `count`, over the filtered host values `regs` (EAX, EBX, ECX, EDX): a
/// package of `count` single-threaded cores, and with `timer` (a run timer,
/// which makes a deadline fire on time) the TSC-deadline mode and an APIC
/// timer that keeps running while the processor idles.
pub(super) fn cpuid(
    leaf: u32,
    subleaf: u32,
    mut regs: [u32; 4],
    timer: bool,
    id: u8,
    count: u8,
) -> [u32; 4] {
    const HTT: u32 = 1 << 28;
    let count = count.max(1);
    match leaf {
        1 => {
            regs[1] = (regs[1] & 0x0000_FFFF) | u32::from(id) << 24 | u32::from(count) << 16;
            regs[3] = if count > 1 {
                regs[3] | HTT
            } else {
                regs[3] & !HTT
            };
            if timer {
                regs[2] |= 1 << 24;
            }
        }
        // Deterministic cache parameters: cores per package, and which of
        // them share the cache (the last level all, the others one).
        4 if regs[0] & 0x1F != 0 => {
            let shared = if (regs[0] >> 5) & 0x7 >= 3 {
                u32::from(count) - 1
            } else {
                0
            };
            regs[0] = (regs[0] & 0x3FFF) | shared << 14 | (u32::from(count) - 1) << 26;
        }
        6 if timer => regs[0] |= 1 << 2,
        // Extended topology: level 0 one thread per core, level 1 `count`
        // cores, then the end of the list. EDX is the x2APIC ID throughout.
        0xB => {
            let core_shift = u32::BITS - (u32::from(count) - 1).leading_zeros();
            regs = match subleaf {
                0 => [0, 1, 1 << 8, 0],
                1 => [core_shift, u32::from(count), 2 << 8 | 1, 0],
                _ => [0, 0, subleaf & 0xFF, 0],
            };
            regs[3] = u32::from(id);
        }
        // V2 extended topology: absent, so the guest reads leaf 0Bh.
        0x1F => regs = [0; 4],
        _ => {}
    }
    regs
}

const ID: u32 = 0x020;
const VERSION: u32 = 0x030;
const TPR: u32 = 0x080;
const PPR: u32 = 0x0A0;
const EOI: u32 = 0x0B0;
const LDR: u32 = 0x0D0;
const DFR: u32 = 0x0E0;
const SVR: u32 = 0x0F0;
const ISR: u32 = 0x100;
const TMR: u32 = 0x180;
const IRR: u32 = 0x200;
const ICR_LOW: u32 = 0x300;
const ICR_HIGH: u32 = 0x310;
const LVT_TIMER: u32 = 0x320;
const LVT_ERROR: u32 = 0x370;
const TMICT: u32 = 0x380;
const TDCR: u32 = 0x3E0;

/// Version 0x14 (an integrated APIC) with six LVT entries (timer, thermal,
/// performance counter, LINT0, LINT1, error): bits 23:16 hold the last index.
const VERSION_VALUE: u32 = 0x14 | 5 << 16;
const SVR_ENABLE: u32 = 1 << 8;
const LVT_MASKED: u32 = 1 << 16;
/// LVT timer bits 18:17 = 10: TSC-deadline mode.
const TIMER_MODE: u32 = 0b11 << 17;
const TIMER_DEADLINE: u32 = 0b10 << 17;
/// ICR fields: delivery mode (10:8), destination mode (11: logical), level
/// (14: assert) and destination shorthand (19:18).
const ICR_DELIVERY: u32 = 0b111 << 8;
const ICR_LOGICAL: u32 = 1 << 11;
const ICR_ASSERT: u32 = 1 << 14;
const ICR_SHORTHAND: u32 = 0b11 << 18;
const SHORTHAND_SELF: u32 = 0b01 << 18;
const SHORTHAND_ALL: u32 = 0b10 << 18;
const SHORTHAND_ALL_BUT_SELF: u32 = 0b11 << 18;
const DELIVERY_FIXED: u32 = 0b000 << 8;
const DELIVERY_LOWEST: u32 = 0b001 << 8;
const DELIVERY_INIT: u32 = 0b101 << 8;
const DELIVERY_START_UP: u32 = 0b110 << 8;

/// Writable bits of the six LVT entries, in register order: vector and mask
/// everywhere, the timer mode, the delivery mode of the non-timer sources,
/// polarity and trigger mode of the two LINT pins.
const LVT_WRITABLE: [u32; 6] = [
    0x0007_00FF,
    0x0001_07FF,
    0x0001_07FF,
    0x0001_A7FF,
    0x0001_A7FF,
    0x0001_00FF,
];

#[derive(Clone, Copy)]
pub(super) struct Lapic {
    /// APIC ID (bits 31:24 of the ID register): the vCPU's index in its VM.
    id: u8,
    tpr: u32,
    ldr: u32,
    dfr: u32,
    svr: u32,
    lvt: [u32; 6],
    icr: [u32; 2],
    tmict: u32,
    tdcr: u32,
    /// Interrupt request and in-service registers, one bit per vector.
    irr: [u32; 8],
    isr: [u32; 8],
    /// TSC value at which the timer fires; 0 while it is not armed.
    deadline: u64,
}

impl Lapic {
    /// The state after reset: software-disabled, every LVT entry masked.
    pub(super) const fn new(id: u8) -> Self {
        Self {
            id,
            tpr: 0,
            ldr: 0,
            dfr: 0xFFFF_FFFF,
            svr: 0xFF,
            lvt: [LVT_MASKED; 6],
            icr: [0; 2],
            tmict: 0,
            tdcr: 0,
            irr: [0; 8],
            isr: [0; 8],
            deadline: 0,
        }
    }

    /// Read the register at `offset` into the page; an offset that names no
    /// register reads 0.
    pub(super) fn read(&self, offset: u32) -> u32 {
        match offset {
            ID => u32::from(self.id) << 24,
            VERSION => VERSION_VALUE,
            TPR => self.tpr,
            PPR => self.processor_priority(),
            LDR => self.ldr,
            DFR => self.dfr,
            SVR => self.svr,
            ISR..=0x170 if offset & 0xF == 0 => self.isr[((offset - ISR) >> 4) as usize],
            // Every interrupt here is edge-triggered: the trigger-mode register is 0.
            TMR..=0x1F0 => 0,
            IRR..=0x270 if offset & 0xF == 0 => self.irr[((offset - IRR) >> 4) as usize],
            ICR_LOW => self.icr[0],
            ICR_HIGH => self.icr[1],
            LVT_TIMER..=LVT_ERROR if offset & 0xF == 0 => {
                self.lvt[((offset - LVT_TIMER) >> 4) as usize]
            }
            TMICT => self.tmict,
            TDCR => self.tdcr,
            _ => 0,
        }
    }

    /// Write the register at `offset`; reserved bits and read-only registers
    /// keep their values. A command for other processors comes back.
    pub(super) fn write(&mut self, offset: u32, value: u32) -> Option<Ipi> {
        match offset {
            TPR => self.tpr = value & 0xFF,
            EOI => {
                if let Some(vector) = highest(&self.isr) {
                    clear(&mut self.isr, vector);
                }
            }
            LDR => self.ldr = value & 0xFF00_0000,
            DFR => self.dfr = value | 0x0FFF_FFFF,
            SVR => {
                self.svr = value & 0x3FF;
                if !self.enabled() {
                    // A software-disabled APIC masks every local source.
                    for entry in &mut self.lvt {
                        *entry |= LVT_MASKED;
                    }
                }
            }
            ICR_LOW => {
                self.icr[0] = value & 0x000C_CFFF;
                return self.send(value);
            }
            ICR_HIGH => self.icr[1] = value & 0xFF00_0000,
            LVT_TIMER..=LVT_ERROR if offset & 0xF == 0 => {
                let index = ((offset - LVT_TIMER) >> 4) as usize;
                let mut entry = value & LVT_WRITABLE[index];
                if !self.enabled() {
                    entry |= LVT_MASKED;
                }
                // Switching the timer's mode disarms it.
                if index == 0 && (entry ^ self.lvt[0]) & TIMER_MODE != 0 {
                    self.deadline = 0;
                }
                self.lvt[index] = entry;
            }
            TMICT => self.tmict = value,
            TDCR => self.tdcr = value & 0xB,
            _ => {}
        }
        None
    }

    /// The logical ID other processors address this one by (flat model).
    pub(super) fn logical_id(&self) -> u8 {
        (self.ldr >> 24) as u8
    }

    pub(super) fn id(&self) -> u8 {
        self.id
    }

    /// An interrupt command (ICR low word written; the destination is in the
    /// high word). This processor's own share is delivered here; the rest is
    /// returned for the kernel to deliver. A fixed interrupt below vector 16,
    /// an INIT or start-up to itself, an INIT level de-assert, and NMI, SMI
    /// and ExtINT commands deliver nothing.
    fn send(&mut self, command: u32) -> Option<Ipi> {
        let kind = match command & ICR_DELIVERY {
            DELIVERY_FIXED | DELIVERY_LOWEST if command as u8 >= 16 => {
                IpiKind::Fixed(command as u8)
            }
            DELIVERY_INIT if command & ICR_ASSERT != 0 => IpiKind::Init,
            DELIVERY_START_UP => IpiKind::StartUp(command as u8),
            _ => return None,
        };
        let destination = (self.icr[1] >> 24) as u8;
        let (to_self, target) = match command & ICR_SHORTHAND {
            SHORTHAND_SELF => (true, None),
            SHORTHAND_ALL => (true, Some(IpiTarget::AllButSelf)),
            SHORTHAND_ALL_BUT_SELF => (false, Some(IpiTarget::AllButSelf)),
            _ if command & ICR_LOGICAL != 0 => (
                self.logical_id() & destination != 0,
                Some(IpiTarget::Logical(destination)),
            ),
            _ if destination == 0xFF => (true, Some(IpiTarget::AllButSelf)),
            _ if destination == self.id => (true, None),
            _ => (false, Some(IpiTarget::Physical(destination))),
        };
        if let (true, IpiKind::Fixed(vector)) = (to_self, kind) {
            self.raise(vector);
        }
        target.map(|target| Ipi { target, kind })
    }

    /// INIT: the state after reset, keeping the APIC ID.
    pub(super) fn reset(&mut self) {
        *self = Self::new(self.id);
    }

    /// A fixed interrupt another processor sent this one.
    pub(super) fn receive(&mut self, vector: u8) {
        self.raise(vector);
    }

    fn enabled(&self) -> bool {
        self.svr & SVR_ENABLE != 0
    }

    /// Request `vector`. Vectors 0-15 are not interrupts and are dropped.
    fn raise(&mut self, vector: u8) {
        if vector >= 16 {
            self.irr[usize::from(vector >> 5)] |= 1 << (vector & 31);
        }
    }

    /// The priority below which nothing is delivered: the higher of the task
    /// priority and the class of the highest interrupt in service.
    fn processor_priority(&self) -> u32 {
        let in_service = highest(&self.isr).map_or(0, |vector| u32::from(vector) & 0xF0);
        if self.tpr >> 4 >= in_service >> 4 {
            self.tpr
        } else {
            in_service
        }
    }

    /// The guest's task-priority class, which is also its CR8.
    pub(super) fn task_priority_class(&self) -> u8 {
        (self.tpr >> 4) as u8
    }

    /// `MOV` to CR8: the class goes into TPR bits 7:4, the sub-class is cleared.
    pub(super) fn set_task_priority_class(&mut self, class: u8) {
        self.tpr = u32::from(class & 0xF) << 4;
    }

    /// The requested vector the guest would be interrupted with now, if any:
    /// the highest one, when the APIC is enabled and its class is above the
    /// processor priority.
    pub(super) fn deliverable(&self) -> Option<u8> {
        let vector = highest(&self.irr)?;
        (self.enabled() && u32::from(vector >> 4) > self.processor_priority() >> 4)
            .then_some(vector)
    }

    /// `vector` is being delivered: it moves from requested to in service
    /// until the guest's EOI.
    pub(super) fn accept(&mut self, vector: u8) {
        clear(&mut self.irr, vector);
        self.isr[usize::from(vector >> 5)] |= 1 << (vector & 31);
    }

    /// `IA32_TSC_DEADLINE`: the armed deadline, 0 once it has fired.
    pub(super) fn deadline(&self) -> u64 {
        self.deadline
    }

    /// A write to `IA32_TSC_DEADLINE`: arms the timer (0 disarms it) in
    /// TSC-deadline mode and is ignored in the other modes.
    pub(super) fn set_deadline(&mut self, tsc: u64) {
        if self.lvt[0] & TIMER_MODE == TIMER_DEADLINE {
            self.deadline = tsc;
        }
    }

    /// Fire the timer if its deadline is at or before `now_tsc`: it disarms,
    /// and requests its vector unless the entry is masked.
    pub(super) fn tick(&mut self, now_tsc: u64) {
        if self.deadline != 0 && now_tsc >= self.deadline {
            self.deadline = 0;
            if self.lvt[0] & LVT_MASKED == 0 {
                self.raise(self.lvt[0] as u8);
            }
        }
    }
}

/// The highest vector set in a 256-bit register.
fn highest(register: &[u32; 8]) -> Option<u8> {
    register
        .iter()
        .enumerate()
        .rev()
        .find(|(_, word)| **word != 0)
        .map(|(index, word)| (index * 32) as u8 + 31 - word.leading_zeros() as u8)
}

fn clear(register: &mut [u32; 8], vector: u8) {
    register[usize::from(vector >> 5)] &= !(1 << (vector & 31));
}

#[cfg(test)]
mod tests {
    use super::*;

    const LVT_DEADLINE_VECTOR_EC: u32 = TIMER_DEADLINE | 0xEC;

    fn enabled() -> Lapic {
        let mut apic = Lapic::new(0);
        apic.write(SVR, SVR_ENABLE | 0xFF);
        apic
    }

    #[test]
    fn the_deadline_timer_fires_once_at_its_deadline() {
        let mut apic = enabled();
        apic.write(LVT_TIMER, LVT_DEADLINE_VECTOR_EC);
        apic.set_deadline(1_000);
        apic.tick(999);
        assert_eq!(apic.deliverable(), None);
        assert_eq!(apic.deadline(), 1_000);

        apic.tick(1_000);
        assert_eq!(apic.deliverable(), Some(0xEC));
        assert_eq!(apic.deadline(), 0, "one shot: disarmed when it fires");
        apic.accept(0xEC);
        apic.tick(5_000);
        assert_eq!(apic.deliverable(), None);
    }

    #[test]
    fn a_deadline_is_ignored_outside_deadline_mode_and_lost_on_a_mode_switch() {
        let mut apic = enabled();
        apic.write(LVT_TIMER, 0xEC); // one-shot count mode
        apic.set_deadline(1_000);
        assert_eq!(apic.deadline(), 0);

        apic.write(LVT_TIMER, LVT_DEADLINE_VECTOR_EC);
        apic.set_deadline(1_000);
        apic.write(LVT_TIMER, 0xEC);
        apic.tick(2_000);
        assert_eq!(apic.deliverable(), None);
    }

    #[test]
    fn a_masked_timer_disarms_without_an_interrupt() {
        let mut apic = enabled();
        apic.write(LVT_TIMER, LVT_DEADLINE_VECTOR_EC | LVT_MASKED);
        apic.set_deadline(10);
        apic.tick(10);
        assert_eq!(apic.deliverable(), None);
        assert_eq!(apic.deadline(), 0);
    }

    #[test]
    fn an_interrupt_in_service_holds_back_its_class_until_eoi() {
        let mut apic = enabled();
        apic.write(ICR_LOW, SHORTHAND_SELF | 0x41);
        apic.write(ICR_LOW, SHORTHAND_SELF | 0x4F);
        apic.write(ICR_LOW, SHORTHAND_SELF | 0x51);
        assert_eq!(apic.deliverable(), Some(0x51), "highest vector first");
        apic.accept(0x51);
        assert_eq!(apic.read(PPR), 0x50);
        assert_eq!(apic.deliverable(), None, "class 4 is below class 5");

        apic.write(EOI, 0);
        assert_eq!(apic.deliverable(), Some(0x4F));
        apic.accept(0x4F);
        assert_eq!(apic.deliverable(), None, "0x41 shares the class in service");
        apic.write(EOI, 0);
        assert_eq!(apic.deliverable(), Some(0x41));
    }

    #[test]
    fn the_task_priority_holds_back_its_class_and_below() {
        let mut apic = enabled();
        apic.write(ICR_LOW, SHORTHAND_SELF | 0x35);
        apic.set_task_priority_class(3);
        assert_eq!(apic.read(TPR), 0x30);
        assert_eq!(apic.deliverable(), None);
        apic.write(TPR, 0x2F);
        assert_eq!(apic.task_priority_class(), 2);
        assert_eq!(apic.deliverable(), Some(0x35));
    }

    #[test]
    fn a_software_disabled_apic_delivers_nothing_and_keeps_its_sources_masked() {
        let mut apic = Lapic::new(0);
        apic.write(LVT_TIMER, LVT_DEADLINE_VECTOR_EC);
        assert_ne!(apic.read(LVT_TIMER) & LVT_MASKED, 0);
        apic.write(ICR_LOW, SHORTHAND_SELF | 0x40);
        assert_eq!(apic.deliverable(), None);

        apic.write(SVR, SVR_ENABLE | 0xFF);
        assert_eq!(apic.deliverable(), Some(0x40), "the request was kept");
        apic.write(SVR, 0xFF);
        assert_eq!(apic.deliverable(), None);
    }

    #[test]
    fn an_ipi_reaches_this_processor_only_when_addressed_to_it() {
        let mut apic = enabled();
        apic.write(ICR_LOW, 0b101 << 8 | SHORTHAND_SELF | 0x60); // INIT, not fixed
        assert_eq!(apic.deliverable(), None);
        apic.write(ICR_LOW, 0x0F | SHORTHAND_SELF); // not an interrupt vector
        assert_eq!(apic.deliverable(), None);

        apic.write(ICR_HIGH, 0);
        assert_eq!(apic.write(ICR_LOW, 0x60), None, "destination 0 is this one");
        assert_eq!(apic.deliverable(), Some(0x60));
    }

    /// Linux brings an application processor up with INIT (assert), INIT
    /// (de-assert) and two start-up IPIs, physical destination.
    #[test]
    fn commands_for_other_processors_leave_with_their_destination() {
        let mut apic = enabled();
        let to = |target, kind| Some(Ipi { target, kind });
        apic.write(ICR_HIGH, 2 << 24);
        assert_eq!(
            apic.write(ICR_LOW, ICR_ASSERT | DELIVERY_INIT | 1 << 15),
            to(IpiTarget::Physical(2), IpiKind::Init)
        );
        assert_eq!(
            apic.write(ICR_LOW, DELIVERY_INIT | 1 << 15),
            None,
            "de-assert"
        );
        assert_eq!(
            apic.write(ICR_LOW, DELIVERY_START_UP | 0x9A),
            to(IpiTarget::Physical(2), IpiKind::StartUp(0x9A))
        );
        assert_eq!(
            apic.write(ICR_LOW, 0xFD),
            to(IpiTarget::Physical(2), IpiKind::Fixed(0xFD))
        );
        assert_eq!(apic.deliverable(), None, "nothing for this one");

        assert_eq!(
            apic.write(ICR_LOW, SHORTHAND_ALL_BUT_SELF | 0xFB),
            to(IpiTarget::AllButSelf, IpiKind::Fixed(0xFB))
        );
        assert_eq!(apic.deliverable(), None);
        assert_eq!(
            apic.write(ICR_LOW, SHORTHAND_ALL | 0xFC),
            to(IpiTarget::AllButSelf, IpiKind::Fixed(0xFC))
        );
        assert_eq!(apic.deliverable(), Some(0xFC), "all includes this one");

        apic.write(LDR, 1 << 24);
        apic.write(ICR_HIGH, 0b11 << 24);
        assert_eq!(
            apic.write(ICR_LOW, ICR_LOGICAL | 0x50),
            to(IpiTarget::Logical(0b11), IpiKind::Fixed(0x50))
        );
        apic.accept(0xFC);
        apic.write(EOI, 0);
        assert_eq!(apic.deliverable(), Some(0x50), "logical bit 0 is this one");
    }

    #[test]
    fn init_resets_everything_but_the_apic_id() {
        let mut apic = Lapic::new(3);
        apic.write(SVR, SVR_ENABLE | 0xFF);
        apic.write(LDR, 1 << 27);
        apic.receive(0x40);
        apic.reset();
        assert_eq!(apic.read(ID), 3 << 24);
        assert_eq!((apic.read(LDR), apic.read(SVR)), (0, 0xFF));
        assert_eq!(apic.read(IRR + 0x20), 0, "requests are gone");
    }

    #[test]
    fn registers_keep_only_their_writable_bits() {
        let mut apic = enabled();
        assert_eq!(apic.read(ID), 0);
        assert_eq!(apic.read(VERSION), 0x0005_0014);
        apic.write(VERSION, !0);
        assert_eq!(apic.read(VERSION), 0x0005_0014);
        apic.write(0x350, !0); // LINT0
        assert_eq!(apic.read(0x350), 0x0001_A7FF);
        apic.write(LDR, !0);
        assert_eq!(apic.read(LDR), 0xFF00_0000);
        apic.write(DFR, 0);
        assert_eq!(apic.read(DFR), 0x0FFF_FFFF);
        apic.write(0x324, !0); // between two registers
        assert_eq!(apic.read(0x324), 0);
        assert_eq!(apic.read(LVT_TIMER), LVT_MASKED);
    }

    #[test]
    fn cpuid_describes_one_package_of_single_threaded_cores() {
        let host = [0x0009_06A3, 0x0B10_0800, 0x7FFA_FBFF, 0xBFEB_FBFF];
        let [_, ebx, ecx, edx] = cpuid(1, 0, host, true, 2, 4);
        assert_eq!(ebx >> 24, 2, "APIC ID");
        assert_eq!((ebx >> 16) & 0xFF, 4, "logical processors in the package");
        assert_ne!(edx & 1 << 28, 0, "HTT");
        assert_ne!(ecx & 1 << 24, 0, "TSC deadline");
        assert_eq!(cpuid(0xB, 0, [9; 4], true, 2, 4), [0, 1, 1 << 8, 2]);
        assert_eq!(cpuid(0xB, 1, [9; 4], true, 2, 4), [2, 4, 2 << 8 | 1, 2]);
        assert_eq!(cpuid(0xB, 2, [9; 4], true, 2, 4), [0, 0, 2, 2]);
        assert_eq!(
            cpuid(0xB, 1, [9; 4], true, 0, 3)[0],
            2,
            "3 cores need 2 bits"
        );
        assert_eq!(cpuid(0x1F, 0, [9; 4], true, 2, 4), [0; 4]);
        let [_, ebx, _, edx] = cpuid(1, 0, host, false, 0, 1);
        assert_eq!(((ebx >> 16) & 0xFF, edx & 1 << 28), (1, 0));
    }
}
