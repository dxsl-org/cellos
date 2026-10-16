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

    /// Advance the enforcement fence monotonically; rejects regressions.
    pub fn advance(&mut self, next: u64) -> Result<(), ()> {
        if next <= self.current {
            return Err(());
        }
        self.current = next;
        Ok(())
    }
}
