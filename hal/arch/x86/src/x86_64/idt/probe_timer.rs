use core::sync::atomic::{AtomicU8, Ordering};

const IDLE: u8 = 0;
const FIRST_EOI: u8 = 1;
const FIRST_DONE: u8 = 2;
const SECOND_EOI: u8 = 3;
const COMPLETE: u8 = 4;

static STATE: AtomicU8 = AtomicU8::new(IDLE);
static COUNT: AtomicU8 = AtomicU8::new(0);

fn transition(from: u8, to: u8) {
    if STATE
        .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        super::probe::fail();
    }
}

pub(super) fn complete() -> bool {
    STATE.load(Ordering::Acquire) == COMPLETE && COUNT.load(Ordering::Acquire) == 2
}

pub(super) fn after_eoi(cpl0_ready: bool) {
    match (STATE.load(Ordering::Acquire), cpl0_ready) {
        (IDLE, true) => {
            if COUNT.fetch_add(1, Ordering::AcqRel) != 0 {
                super::probe::fail();
            }
            transition(IDLE, FIRST_EOI);
        }
        (FIRST_DONE, true) => {
            if COUNT.fetch_add(1, Ordering::AcqRel) != 1 {
                super::probe::fail();
            }
            transition(FIRST_DONE, SECOND_EOI);
        }
        (COMPLETE, _) => super::cpl3_probe::timer_after_eoi(),
        _ => super::probe::fail(),
    }
}

pub(super) fn after_callback() {
    match STATE.load(Ordering::Acquire) {
        FIRST_EOI => transition(FIRST_EOI, FIRST_DONE),
        SECOND_EOI => {
            unsafe { super::super::apic::start_oneshot(0) };
            transition(SECOND_EOI, COMPLETE);
            super::super::uart_16550::puts(
                "\nX86-IDT-SELFTEST: PASS bp=3 gp=13/ec=fffc gprs=15 df=ok align=ok timer=32\n",
            );
        }
        COMPLETE => super::cpl3_probe::timer_after_callback(),
        _ => super::probe::fail(),
    }
}
