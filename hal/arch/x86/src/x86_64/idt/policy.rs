#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    Kernel,
    User,
}

impl Origin {
    pub(super) fn from_saved_cs(cs: u64) -> Self {
        if cs & 3 == 3 {
            Self::User
        } else {
            Self::Kernel
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    PageFault,
    TerminateUser,
    FatalException,
    Timer,
    Uart,
    LegacyInt80,
    LapicSpurious,
    FatalUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Eoi {
    None,
    Before,
    After,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub route: Route,
    pub eoi: Eoi,
}

const fn policy(route: Route, eoi: Eoi) -> Policy {
    Policy { route, eoi }
}

pub fn classify(vector: u8, origin: Origin) -> Policy {
    match vector {
        14 => policy(Route::PageFault, Eoi::None),
        2 | 8 | 18 => policy(Route::FatalException, Eoi::None),
        0..=31 if matches!(origin, Origin::User) => policy(Route::TerminateUser, Eoi::None),
        0..=31 => policy(Route::FatalException, Eoi::None),
        0x20 => policy(Route::Timer, Eoi::Before),
        0x24 => policy(Route::Uart, Eoi::After),
        0x80 => policy(Route::LegacyInt80, Eoi::None),
        0xff => policy(Route::LapicSpurious, Eoi::None),
        _ => policy(Route::FatalUnknown, Eoi::None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x86_64::idt::entry::EntryFrame;
    use crate::x86_64::idt::X86_IDT_ERROR_VECTORS;

    fn expect(vector: u8, origin: Origin, route: Route, eoi: Eoi) {
        assert_eq!(classify(vector, origin), Policy { route, eoi });
    }

    #[test]
    fn attributes_user_exceptions_but_not_machine_faults() {
        for vector in 0..=31 {
            let user_route = match vector {
                14 => Route::PageFault,
                2 | 8 | 18 => Route::FatalException,
                _ => Route::TerminateUser,
            };
            let kernel_route = if vector == 14 {
                Route::PageFault
            } else {
                Route::FatalException
            };
            expect(vector, Origin::User, user_route, Eoi::None);
            expect(vector, Origin::Kernel, kernel_route, Eoi::None);
        }
    }

    #[test]
    fn assigns_explicit_irq_and_software_routes() {
        expect(0x20, Origin::Kernel, Route::Timer, Eoi::Before);
        expect(0x24, Origin::Kernel, Route::Uart, Eoi::After);
        expect(0x80, Origin::User, Route::LegacyInt80, Eoi::None);
        expect(0xff, Origin::Kernel, Route::LapicSpurious, Eoi::None);
        expect(0x21, Origin::Kernel, Route::FatalUnknown, Eoi::None);
        expect(0xfe, Origin::Kernel, Route::FatalUnknown, Eoi::None);
    }

    #[test]
    fn generated_error_vector_set_is_architecturally_exact() {
        assert_eq!(
            X86_IDT_ERROR_VECTORS,
            [8, 10, 11, 12, 13, 14, 17, 21, 29, 30]
        );
        for vector in [3, 0x20, 0x24, 0x80, 0xff] {
            assert!(!X86_IDT_ERROR_VECTORS.contains(&vector));
        }
    }

    #[test]
    fn optional_privilege_stack_is_read_only_for_non_kernel_cs() {
        let mut words = [0u64; 22];
        words[18] = 0x08;
        {
            let frame = unsafe { &*words.as_ptr().cast::<EntryFrame>() };
            assert_eq!(frame.old_rsp(), None);
            assert_eq!(frame.old_ss(), None);
            assert_eq!(
                frame.interrupted_rsp(),
                frame as *const EntryFrame as u64 + 160
            );
        }

        words[18] = 0x23;
        words[20] = 0x1234_5678_9abc_def0;
        words[21] = 0x1b;
        let frame = unsafe { &*words.as_ptr().cast::<EntryFrame>() };
        assert_eq!(frame.old_rsp(), Some(words[20]));
        assert_eq!(frame.old_ss(), Some(words[21]));
        assert_eq!(frame.interrupted_rsp(), words[20]);
    }
}
