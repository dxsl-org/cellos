#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiscvMmioRegion {
    pub base: usize,
    pub size: usize,
    pub irq: Option<u32>,
}

impl RiscvMmioRegion {
    pub const fn is_valid(self) -> bool {
        self.base != 0 && self.size != 0 && self.base.checked_add(self.size).is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiscvFallbackMmio {
    pub uart: Option<RiscvMmioRegion>,
    pub plic: RiscvMmioRegion,
    pub clint: RiscvMmioRegion,
    pub rtc: Option<RiscvMmioRegion>,
    pub virtio: &'static [RiscvMmioRegion],
}

impl RiscvFallbackMmio {
    pub const fn is_valid(self) -> bool {
        if !self.plic.is_valid() || !self.clint.is_valid() {
            return false;
        }
        if let Some(uart) = self.uart {
            if !uart.is_valid() {
                return false;
            }
        }
        if let Some(rtc) = self.rtc {
            if !rtc.is_valid() {
                return false;
            }
        }
        if self.virtio.len() > 8 {
            return false;
        }
        let mut index = 0;
        while index < self.virtio.len() {
            if !self.virtio[index].is_valid() {
                return false;
            }
            if index > 0 && self.virtio[index].base <= self.virtio[index - 1].base {
                return false;
            }
            index += 1;
        }
        true
    }
}
