use super::{
    entry::EntryFrame,
    fatal,
    policy::{classify, Eoi, Origin, Route},
};
use hal_arch_trait::{
    vi_handle_page_fault, vi_handle_uart_irq, vi_terminate_on_user_trap_fault, vi_timer_tick,
};

#[no_mangle]
pub(super) extern "C" fn x86_64_idt_dispatch(frame: &mut EntryFrame) {
    #[cfg(feature = "x86-idt-cpl3-test")]
    if super::probe::handle_entry(frame) {
        return;
    }

    let Ok(vector) = u8::try_from(frame.vector) else {
        fatal::halt(frame);
    };
    let selected = classify(vector, Origin::from_saved_cs(frame.cs));
    match selected.route {
        Route::PageFault => {
            debug_assert_eq!(selected.eoi, Eoi::None);
            let cr2: u64;
            unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };
            unsafe {
                vi_handle_page_fault(
                    cr2 as usize,
                    frame.error,
                    frame.rip,
                    frame.cs,
                    frame.interrupted_rsp(),
                );
            }
        }
        Route::TerminateUser => {
            debug_assert_eq!(selected.eoi, Eoi::None);
            unsafe {
                vi_terminate_on_user_trap_fault(frame.vector as usize, frame.rip as usize, 0);
            }
        }
        Route::Timer => {
            debug_assert_eq!(selected.eoi, Eoi::Before);
            super::super::apic::eoi();
            #[cfg(feature = "x86-idt-cpl3-test")]
            super::probe::timer_after_eoi();
            unsafe { vi_timer_tick() };
            #[cfg(feature = "x86-idt-cpl3-test")]
            super::probe::timer_after_callback();
        }
        Route::Uart => {
            debug_assert_eq!(selected.eoi, Eoi::After);
            unsafe { vi_handle_uart_irq() };
            super::super::apic::eoi();
        }
        Route::LegacyInt80 | Route::LapicSpurious => {
            debug_assert_eq!(selected.eoi, Eoi::None);
        }
        Route::FatalException | Route::FatalUnknown => fatal::halt(frame),
    }
}
