// SPDX-License-Identifier: MPL-2.0
//! FADT (signature `FACP`) parser.

use super::bytes::TableBytes;
use super::records::Gas;
use super::AcpiInfo;

/// FADT facts later phases use: PM timer, reset, PM1 blocks, SCI and the
/// IA-PC boot-architecture flags. Addresses come from the 64-bit `X_` fields
/// when present, otherwise from the legacy 32-bit I/O port fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FadtInfo {
    /// SDT header revision.
    pub revision: u8,
    pub flags: u32,
    /// IA-PC boot architecture flags. The field is reserved before revision 3.
    pub iapc_boot_arch: u16,
    pub sci_int: u16,
    pub smi_cmd: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub pm1a_evt: Gas,
    pub pm1b_evt: Gas,
    pub pm1a_cnt: Gas,
    pub pm1b_cnt: Gas,
    pub pm_timer: Gas,
    pub reset_reg: Gas,
    pub reset_value: u8,
    pub dsdt: u64,
    pub sleep_control: Gas,
    pub sleep_status: Gas,
}

impl FadtInfo {
    /// The PM timer counter is 32 bits wide (otherwise 24).
    pub const TMR_VAL_EXT: u32 = 1 << 8;
    /// `reset_reg` is valid.
    pub const RESET_REG_SUP: u32 = 1 << 10;
    /// Hardware-reduced ACPI: no PM1, PM timer or SCI hardware.
    pub const HW_REDUCED_ACPI: u32 = 1 << 20;
    /// IA-PC boot arch: an 8042 keyboard controller is present.
    pub const BOOT_ARCH_8042: u16 = 1 << 1;
    /// IA-PC boot arch: MSI must not be enabled on this platform.
    pub const BOOT_ARCH_MSI_NOT_SUPPORTED: u16 = 1 << 3;

    pub fn pm_timer_32bit(&self) -> bool {
        self.flags & Self::TMR_VAL_EXT != 0
    }

    pub fn reset_supported(&self) -> bool {
        self.flags & Self::RESET_REG_SUP != 0 && self.reset_reg.is_present()
    }

    pub fn hw_reduced(&self) -> bool {
        self.flags & Self::HW_REDUCED_ACPI != 0
    }

    /// Set only when the table is new enough to carry the field.
    pub fn msi_forbidden(&self) -> bool {
        self.revision >= 3 && self.iapc_boot_arch & Self::BOOT_ARCH_MSI_NOT_SUPPORTED != 0
    }
}

/// Parse the FADT. Offsets follow ACPI 6.5 Table 5.9. ACPI 1.0 tables end at
/// 116, ACPI 2.0+ at 244 or more. Fields past the table length stay zero.
pub(super) fn parse_fadt(virt: usize, length: usize, info: &mut AcpiInfo) {
    if length < 116 {
        log::warn!("[acpi] FADT too short ({} bytes)", length);
        return;
    }
    // SAFETY: the caller validated and mapped `[virt, virt + length)`.
    let t = unsafe { TableBytes::new(virt, length) };
    let u8_at = |off| t.u8(off).unwrap_or(0);
    let u32_at = |off| t.u32(off).unwrap_or(0);

    // A non-zero 64-bit `X_` GAS wins; otherwise the 32-bit I/O port field
    // with a width of `len` bytes.
    let block = |x_off: usize, legacy_off: usize, legacy_len: u8| -> Gas {
        if let Some(gas) = t.gas(x_off).filter(Gas::is_present) {
            return gas;
        }
        Gas {
            space_id: Gas::SYSTEM_IO,
            bit_width: legacy_len.saturating_mul(8),
            bit_offset: 0,
            access_size: 0,
            address: u32_at(legacy_off) as u64,
        }
    };

    let revision = u8_at(8);
    let pm1_evt_len = u8_at(88);
    let pm1_cnt_len = u8_at(89);
    let x_dsdt = t.u64(140).unwrap_or(0);
    let fadt = FadtInfo {
        revision,
        flags: u32_at(112),
        iapc_boot_arch: if revision >= 3 {
            t.u16(109).unwrap_or(0)
        } else {
            0
        },
        sci_int: t.u16(46).unwrap_or(0),
        smi_cmd: u32_at(48),
        acpi_enable: u8_at(52),
        acpi_disable: u8_at(53),
        pm1a_evt: block(148, 56, pm1_evt_len),
        pm1b_evt: block(160, 60, pm1_evt_len),
        pm1a_cnt: block(172, 64, pm1_cnt_len),
        pm1b_cnt: block(184, 68, pm1_cnt_len),
        pm_timer: block(208, 76, u8_at(91)),
        reset_reg: t.gas(116).unwrap_or_default(),
        reset_value: u8_at(128),
        dsdt: if x_dsdt != 0 {
            x_dsdt
        } else {
            u32_at(40) as u64
        },
        sleep_control: t.gas(244).unwrap_or_default(),
        sleep_status: t.gas(256).unwrap_or_default(),
    };
    log::info!(
        "[acpi] FADT: rev={} pm_timer={:#x}{} reset={} hw_reduced={} msi_forbidden={} sci={}",
        fadt.revision,
        fadt.pm_timer.address,
        if fadt.pm_timer_32bit() { "/32" } else { "/24" },
        fadt.reset_supported(),
        fadt.hw_reduced(),
        fadt.msi_forbidden(),
        fadt.sci_int
    );
    info.fadt = Some(fadt);
}
