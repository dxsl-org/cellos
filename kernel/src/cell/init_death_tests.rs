//! Host tests for the init-death policy (code → action) and the A5 record line.

use super::*;
use alloc::format;

fn death(reason: Reason, pc: Option<usize>) -> InitDeath {
    InitDeath {
        tid: 2,
        cell_id: 7,
        generation: 3,
        reason,
        pc,
    }
}

#[test]
fn exit_reason_classification() {
    assert_eq!(Reason::from_exit_reason(usize::MAX), Reason::Fault);
    assert_eq!(Reason::from_exit_reason(0), Reason::Exit(0));
    assert_eq!(
        Reason::from_exit_reason(INIT_BOOT_STOP),
        Reason::Exit(INIT_BOOT_STOP)
    );
}

#[test]
fn only_the_boot_stop_exit_halts() {
    assert_eq!(Reason::Exit(INIT_BOOT_STOP).action(), Action::Halt);
    // The ostd startup return, the ostd panic exit, any other code, and a
    // fault or kernel kill all reset.
    for code in [0, 1, 2, 0xAAAA_AAAA, INIT_BOOT_STOP - 1, INIT_BOOT_STOP + 1] {
        assert_eq!(Reason::Exit(code).action(), Action::Reset, "code {code:#x}");
    }
    assert_eq!(Reason::Fault.action(), Action::Reset);
    assert_eq!(Reason::from_exit_reason(usize::MAX).action(), Action::Reset);
}

#[test]
fn boot_stop_code_is_reserved() {
    // Distinct from every exit init or the kernel already produces: ostd's
    // startup return (0), ostd's panic handler (1), hot-swap retirement and the
    // fault/kill sentinel.
    for taken in [0, 1, 0xAAAA_AAAA, usize::MAX] {
        assert_ne!(INIT_BOOT_STOP, taken);
    }
    // The audit `CellExit` record carries the code as u32.
    assert_eq!(INIT_BOOT_STOP as u32 as usize, INIT_BOOT_STOP);
}

#[test]
fn record_line_for_a_fault_resets() {
    let line = format!("{}", death(Reason::Fault, Some(0x40_1234)));
    assert_eq!(
        line,
        "[init-death] tid=2 cell=7 gen=3 reason=fault pc=0x401234 action=reset"
    );
}

#[test]
fn record_line_for_a_kill_without_pc() {
    let line = format!("{}", death(Reason::Fault, None));
    assert_eq!(
        line,
        "[init-death] tid=2 cell=7 gen=3 reason=fault pc=- action=reset"
    );
}

#[test]
fn record_line_for_boot_stop_halts() {
    let line = format!("{}", death(Reason::Exit(INIT_BOOT_STOP), None));
    assert_eq!(
        line,
        "[init-death] tid=2 cell=7 gen=3 reason=0xb0075709 pc=- action=halt"
    );
}

#[test]
fn record_line_for_a_plain_exit_resets() {
    let line = format!("{}", death(Reason::Exit(0), None));
    assert_eq!(
        line,
        "[init-death] tid=2 cell=7 gen=3 reason=0x0 pc=- action=reset"
    );
}

#[test]
fn record_is_fixed_size() {
    // No heap behind the record: it is `Copy` and a handful of words.
    fn is_copy<T: Copy>() {}
    is_copy::<InitDeath>();
    assert!(core::mem::size_of::<InitDeath>() <= 8 * core::mem::size_of::<usize>());
}
