//! Non-regressing authority enforcement fence.

pub struct EnforcementFence {
    current: u64,
}

impl EnforcementFence {
    pub const fn new(floor: u64) -> Self {
        Self { current: floor }
    }

    pub fn get(&self) -> u64 {
        self.current
    }

    /// Advance the enforcement fence monotonically; returns `false` (and leaves the fence
    /// unchanged) on regression.
    #[must_use]
    pub fn advance(&mut self, next: u64) -> bool {
        if next <= self.current {
            return false;
        }
        self.current = next;
        true
    }
}
