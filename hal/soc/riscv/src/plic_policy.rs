/// Checked mapping from physical hart id to S-mode PLIC context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlicContextPolicy {
    first_s_mode_hart: usize,
    first_s_mode_context: usize,
    context_stride: usize,
}

impl PlicContextPolicy {
    pub const fn new(
        first_s_mode_hart: usize,
        first_s_mode_context: usize,
        context_stride: usize,
    ) -> Self {
        Self {
            first_s_mode_hart,
            first_s_mode_context,
            context_stride,
        }
    }

    /// Layout with one M-mode context followed by one S-mode context per hart.
    pub const fn machine_then_supervisor() -> Self {
        Self::new(0, 1, 2)
    }

    /// JH7110 omits the S-mode context for its physical hart 0 monitor core.
    pub const fn jh7110() -> Self {
        Self::new(1, 2, 2)
    }

    /// Returns the S-mode context for a physical hart, or `None` when absent.
    pub const fn s_mode_context_for_physical_hart(self, physical_hart: usize) -> Option<usize> {
        let relative_hart = match physical_hart.checked_sub(self.first_s_mode_hart) {
            Some(relative_hart) => relative_hart,
            None => return None,
        };
        let offset = match relative_hart.checked_mul(self.context_stride) {
            Some(offset) => offset,
            None => return None,
        };
        self.first_s_mode_context.checked_add(offset)
    }
}
