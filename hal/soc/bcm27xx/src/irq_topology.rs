/// Immutable interrupt routing facts for one BCM27xx SoC generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bcm27xxIrqTopology {
    pub system_timer_c1: u32,
    pub aux: u32,
    pub gpio_bank0: u32,
    pub gpio_bank1: u32,
    pub local_timer_ns_mask: u32,
    pub local_timer_hp_mask: u32,
    pub local_gpu_mask: u32,
}

impl Bcm27xxIrqTopology {
    /// Returns whether legacy IRQs and local-source masks form a valid topology.
    pub const fn is_valid(self) -> bool {
        let legacy_in_range = self.system_timer_c1 < 64
            && self.aux < 64
            && self.gpio_bank0 < 64
            && self.gpio_bank1 < 64;
        let legacy_unique = self.system_timer_c1 != self.aux
            && self.system_timer_c1 != self.gpio_bank0
            && self.system_timer_c1 != self.gpio_bank1
            && self.aux != self.gpio_bank0
            && self.aux != self.gpio_bank1
            && self.gpio_bank0 != self.gpio_bank1;
        let local_one_hot = is_one_hot(self.local_timer_ns_mask)
            && is_one_hot(self.local_timer_hp_mask)
            && is_one_hot(self.local_gpu_mask);
        let local_disjoint = self.local_timer_ns_mask & self.local_timer_hp_mask == 0
            && self.local_timer_ns_mask & self.local_gpu_mask == 0
            && self.local_timer_hp_mask & self.local_gpu_mask == 0;

        legacy_in_range && legacy_unique && local_one_hot && local_disjoint
    }
}

const fn is_one_hot(mask: u32) -> bool {
    mask != 0 && mask & (mask - 1) == 0
}
