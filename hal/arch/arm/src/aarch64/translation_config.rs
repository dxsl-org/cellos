// SPDX-License-Identifier: MPL-2.0
//! AArch64 translation configuration and physical address bounds (Phase 02b).
//!
//! Configures VA/PA width, walk levels (3 or 4), and TCR_EL1/EL2 IPS field based on
//! hardware PARange and maximum physical/MMIO address.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TranslationConfig {
    /// TCR T0SZ field (bits [5:0]): VA size is (64 - t0sz).
    /// 25 for 39-bit VA (512 GiB, 3 levels L1..L3).
    /// 16 for 48-bit VA (256 TiB, 4 levels L0..L3).
    pub t0sz: u8,
    /// Number of translation table walk levels: 3 (L1..L3) or 4 (L0..L3).
    pub levels: u8,
    /// Physical address width in bits (32..=48).
    pub pa_bits: u8,
    /// TCR IPS / PS field (bits [34:32]): intermediate physical address size code.
    /// 0=32b, 1=36b, 2=40b, 3=42b, 4=44b, 5=48b.
    pub ips: u8,
}

impl TranslationConfig {
    /// Physical address width encoded by `ID_AA64MMFR0_EL1.PARange`.
    pub const fn parange_to_bits(code: u8) -> u8 {
        match code & 0xF {
            0b0000 => 32,
            0b0001 => 36,
            0b0010 => 40,
            0b0011 => 42,
            0b0100 => 44,
            0b0101 => 48,
            0b0110 => 52,
            _ => 40,
        }
    }

    /// 39-bit VA, 3-level walk, 40-bit IPS: the configuration before any probe.
    pub const fn default_39bit() -> Self {
        Self {
            t0sz: 25,
            levels: 3,
            pa_bits: 40,
            ips: 2,
        }
    }

    /// Derive the regime for a CPU with `parange_code` whose identity map must
    /// reach `max_phys_addr`.
    ///
    /// The kernel maps VA = PA, so the walk depth follows the highest address
    /// it must map, not the CPU's PA width: a 4-level walk is used only when
    /// that address needs more than 39 VA bits. Every other consumer of the
    /// shared root (the Pi EL2 monitor's `EL2_TCR`, its `identity_leaf` check)
    /// walks 3 levels.
    pub fn derive(parange_code: u8, max_phys_addr: u64) -> Self {
        let max_pa_bits = Self::parange_to_bits(parange_code);
        let ips = (parange_code & 0x7).min(5);

        if max_phys_addr > (1u64 << 39) {
            Self {
                t0sz: 16,
                levels: 4,
                pa_bits: max_pa_bits.min(48),
                ips,
            }
        } else {
            Self {
                t0sz: 25,
                levels: 3,
                pa_bits: 40,
                ips: ips.min(2),
            }
        }
    }

    /// Pack into the `u32` layout `ACTIVE_CONFIG` stores.
    pub const fn pack(&self) -> u32 {
        (self.t0sz as u32)
            | ((self.levels as u32) << 8)
            | ((self.pa_bits as u32) << 16)
            | ((self.ips as u32) << 24)
    }

    /// Inverse of [`pack`](Self::pack).
    pub const fn unpack(val: u32) -> Self {
        Self {
            t0sz: (val & 0xFF) as u8,
            levels: ((val >> 8) & 0xFF) as u8,
            pa_bits: ((val >> 16) & 0xFF) as u8,
            ips: ((val >> 24) & 0xFF) as u8,
        }
    }

    /// `TCR_EL1` (and VHE `TCR_EL2`, same layout): T0SZ, IPS, WB-WA inner-
    /// shareable TTBR0 walks, TTBR1 walks disabled. TG0 = 0b00 is 4 KiB; TG1 =
    /// 0b10 is 4 KiB, since 0b00 is a reserved TG1 encoding even with EPD1 set.
    pub const fn tcr_el1_value(&self) -> u64 {
        (self.t0sz as u64)
            | ((self.ips as u64) << 32)
            | (1 << 8) // IRGN0 = WB-WA
            | (1 << 10) // ORGN0 = WB-WA
            | (3 << 12) // SH0 = Inner-shareable
            | (1 << 23) // EPD1: no TTBR1 walks
            | (2_u64 << 30) // TG1 = 4 KiB
    }

    /// VHE `TCR_EL2` (E2H = 1 gives it the `TCR_EL1` layout).
    pub const fn vhe_tcr_value(&self) -> u64 {
        self.tcr_el1_value()
    }
}

static ACTIVE_CONFIG: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0x02_28_03_19); // default 39-bit: ips=2, pa_bits=40, levels=3, t0sz=25

/// Publish the regime the kernel root is built for; read by every walker.
pub fn set_translation_config(config: TranslationConfig) {
    ACTIVE_CONFIG.store(config.pack(), core::sync::atomic::Ordering::Release);
}

/// The regime published by [`set_translation_config`] (39-bit default before).
pub fn current_translation_config() -> TranslationConfig {
    TranslationConfig::unpack(ACTIVE_CONFIG.load(core::sync::atomic::Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_config_derive_rules() {
        // Standard QEMU virt / 40-bit PA
        let c1 = TranslationConfig::derive(0b0010, 0x4000_0000);
        assert_eq!(c1.t0sz, 25);
        assert_eq!(c1.levels, 3);
        assert_eq!(c1.pa_bits, 40);
        assert_eq!(c1.ips, 2);
        assert_eq!(c1.tcr_el1_value() & 0x3F, 25);
        assert_eq!((c1.tcr_el1_value() >> 32) & 0x7, 2);

        // Wide PA / High MMIO > 512 GiB (e.g. 1 TiB t600x)
        let c2 = TranslationConfig::derive(0b0101, 0x100_0000_0000);
        assert_eq!(c2.t0sz, 16);
        assert_eq!(c2.levels, 4);
        assert_eq!(c2.pa_bits, 48);
        assert_eq!(c2.ips, 5);
        assert_eq!(c2.tcr_el1_value() & 0x3F, 16);
        assert_eq!((c2.tcr_el1_value() >> 32) & 0x7, 5);

        // Pack and unpack round-trip
        assert_eq!(TranslationConfig::unpack(c1.pack()), c1);
        assert_eq!(TranslationConfig::unpack(c2.pack()), c2);

        // A wide-PA CPU whose RAM and MMIO sit below 512 GiB keeps the 3-level walk
        // (QEMU `-cpu max`, Cortex-A72, Apple M1).
        let c3 = TranslationConfig::derive(0b0101, 0x1_0000_0000);
        assert_eq!(c3.t0sz, 25);
        assert_eq!(c3.levels, 3);
        assert_eq!(c3.ips, 2);
        assert_eq!((c3.tcr_el1_value() >> 30) & 0x3, 0b10);
    }
}
