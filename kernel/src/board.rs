#[cfg(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
))]
use cellos_boards::{Architecture, BoardDescriptor, ValidationError};

#[cfg(all(target_arch = "riscv64", feature = "board-pioneer"))]
const SELECTED_RISCV64_BOARD: &BoardDescriptor = &cellos_boards::milk_v_pioneer::MILK_V_PIONEER;
#[cfg(all(
    target_arch = "riscv64",
    not(feature = "board-pioneer"),
    feature = "board-vf2"
))]
const SELECTED_RISCV64_BOARD: &BoardDescriptor =
    &cellos_boards::starfive_visionfive_2::STARFIVE_VISIONFIVE_2;
#[cfg(all(
    target_arch = "riscv64",
    not(feature = "board-pioneer"),
    not(feature = "board-vf2")
))]
const SELECTED_RISCV64_BOARD: &BoardDescriptor =
    &cellos_boards::qemu_virt_riscv64::QEMU_VIRT_RISCV64;

#[cfg(target_arch = "riscv64")]
/// Returns the descriptor selected by the compatibility board feature.
pub(crate) const fn selected_riscv64_board() -> &'static BoardDescriptor {
    SELECTED_RISCV64_BOARD
}

#[cfg(target_arch = "riscv64")]
/// Returns the validated descriptor before early boot consumes MMIO or RAM ranges.
pub(crate) fn selected() -> &'static BoardDescriptor {
    match SELECTED_RISCV64_BOARD.validate_for(Architecture::Riscv64) {
        Ok(()) => SELECTED_RISCV64_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(target_arch = "riscv64")]
pub(crate) fn active() -> &'static BoardDescriptor {
    selected()
}

#[cfg(target_arch = "riscv64")]
/// Returns the SoC policy paired with the selected RISC-V board descriptor.
pub(crate) fn selected_riscv64_soc() -> &'static hal_soc_riscv::RiscvSocProfile {
    use cellos_boards::SocId;

    match selected().soc {
        SocId::GenericRiscvVirt => &hal_soc_riscv::GENERIC_VIRT,
        SocId::Jh7110 => &hal_soc_riscv::JH7110,
        SocId::Sg2042 => &hal_soc_riscv::SG2042,
        _ => panic!("[board] RISC-V descriptor has incompatible SoC identity"),
    }
}

#[cfg(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
))]
fn invalid_descriptor(error: ValidationError) -> ! {
    panic!("[board] invalid descriptor: {:?}", error)
}

#[cfg(all(target_arch = "x86_64", not(feature = "board-x86-pc")))]
const SELECTED_X86_64_BOARD: &BoardDescriptor = &cellos_boards::qemu_q35_x86_64::QEMU_Q35_X86_64;

/// Generic PC descriptor (phase 01 of the x86 PC lane). Selected by
/// `board-x86-pc`; the machine-level qualification list lives in
/// `docs/hardware-compatibility-list.md`.
#[cfg(all(target_arch = "x86_64", feature = "board-x86-pc"))]
const SELECTED_X86_64_BOARD: &BoardDescriptor = &cellos_boards::pc_x86_64::X86_64_PC;

#[cfg(target_arch = "x86_64")]
/// Validates the board contract before early boot consumes platform facts.
pub(crate) fn selected() -> &'static BoardDescriptor {
    match SELECTED_X86_64_BOARD.validate_for(Architecture::X86_64) {
        Ok(()) => SELECTED_X86_64_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(target_arch = "x86_64")]
/// Returns the validated x86 platform profile paired with the selected board.
pub(crate) fn selected_x86_64_soc() -> &'static hal_soc_x86::X86PlatformProfile {
    use cellos_boards::SocId;

    match selected().soc {
        SocId::QemuX86Q35 => validated_x86_profile(&hal_soc_x86::QEMU_Q35),
        SocId::GenericX86Pc => validated_x86_profile(&hal_soc_x86::GENERIC_X86_PC),
        _ => panic!("[board] x86 descriptor has incompatible SoC identity"),
    }
}

/// Validates an x86 platform profile before early boot consumes its facts.
#[cfg(target_arch = "x86_64")]
fn validated_x86_profile(
    profile: &'static hal_soc_x86::X86PlatformProfile,
) -> &'static hal_soc_x86::X86PlatformProfile {
    match profile.validate() {
        Ok(()) => profile,
        Err(error) => panic!("[board] invalid x86 SoC profile: {:?}", error),
    }
}

/// Machine identity facts detected at boot on x86_64.
///
/// Plain data: it selects nothing and grants nothing. Profile binding may use
/// it only to validate or reject a profile the operator named (ADR-0029 §2).
/// Every byte string in it is firmware-controlled.
#[cfg(target_arch = "x86_64")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlatformIdentity {
    /// OEM fields of the validated ACPI root table; `None` without one.
    pub(crate) acpi_oem: Option<crate::acpi::AcpiOem>,
    /// SMBIOS type 1; `None` when firmware publishes none or it is malformed.
    pub(crate) smbios: Option<crate::smbios::SmbiosSystem>,
    /// `CPUID.1:ECX[31]`: a hypervisor reports itself.
    pub(crate) hypervisor: bool,
}

/// One log line: `acpi_oem=… smbios=… hv=0|1`. Firmware strings are printed
/// quoted, with bytes outside printable ASCII, `"` and `\` escaped, so they
/// cannot break the line or forge another record.
#[cfg(target_arch = "x86_64")]
impl core::fmt::Display for PlatformIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.acpi_oem {
            Some(oem) => write!(
                f,
                "acpi_oem={} acpi_table={} acpi_rev={:#x}",
                Quoted(&oem.oem_id),
                Quoted(&oem.oem_table_id),
                oem.oem_revision
            )?,
            None => f.write_str("acpi_oem=none")?,
        }
        match &self.smbios {
            Some(system) => {
                write!(
                    f,
                    " smbios=manufacturer={} product={} uuid=",
                    Quoted(system.manufacturer.as_bytes()),
                    Quoted(system.product.as_bytes())
                )?;
                match system.uuid {
                    Some(uuid) => {
                        for (index, byte) in uuid.iter().enumerate() {
                            if matches!(index, 4 | 6 | 8 | 10) {
                                f.write_str("-")?;
                            }
                            write!(f, "{:02x}", byte)?;
                        }
                    }
                    None => f.write_str("none")?,
                }
            }
            None => f.write_str(" smbios=none")?,
        }
        write!(f, " hv={}", u8::from(self.hypervisor))
    }
}

/// Writes `[identity] <facts>` to the early console. Detection runs before the
/// logger is installed, and `log` output is dropped until then.
#[cfg(target_arch = "x86_64")]
pub(crate) fn report_identity(identity: &PlatformIdentity) {
    struct EarlyConsole;
    impl core::fmt::Write for EarlyConsole {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            crate::early_uart::text(text);
            Ok(())
        }
    }
    // EarlyConsole never reports an error, so the result carries nothing.
    let _ = core::fmt::Write::write_fmt(&mut EarlyConsole, format_args!("[identity] {identity}\n"));
}

/// Firmware bytes rendered as a quoted, escaped ASCII string.
#[cfg(target_arch = "x86_64")]
struct Quoted<'a>(&'a [u8]);

#[cfg(target_arch = "x86_64")]
impl core::fmt::Display for Quoted<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("\"")?;
        for &byte in self.0 {
            match byte {
                b'"' | b'\\' => write!(f, "\\{}", byte as char)?,
                0x20..=0x7E => write!(f, "{}", byte as char)?,
                _ => write!(f, "\\x{:02x}", byte)?,
            }
        }
        f.write_str("\"")
    }
}

#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
const DEFAULT_RPI3_BOARD: &BoardDescriptor =
    &cellos_boards::raspberry_pi_3_model_b::RASPBERRY_PI_3_MODEL_B;

#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
/// Returns the compiled-in RPi3 descriptor used by const fallback boot data.
pub(crate) const fn default_rpi3_board() -> &'static BoardDescriptor {
    DEFAULT_RPI3_BOARD
}

#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
/// Returns the validated RPi3 descriptor before platform drivers consume it.
pub(crate) fn selected_rpi3() -> &'static BoardDescriptor {
    match DEFAULT_RPI3_BOARD.validate_for(Architecture::Aarch64) {
        Ok(()) => DEFAULT_RPI3_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub(crate) fn active() -> &'static BoardDescriptor {
    selected_rpi3()
}

#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
const QEMU_ARM_VIRT_BOARD: &BoardDescriptor = &cellos_boards::qemu_virt_aarch64::QEMU_VIRT_AARCH64;

#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
pub(crate) const fn default_qemu_arm_virt_board() -> &'static BoardDescriptor {
    QEMU_ARM_VIRT_BOARD
}

#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
pub(crate) fn selected_qemu_arm_virt() -> &'static BoardDescriptor {
    match QEMU_ARM_VIRT_BOARD.validate_for(Architecture::Aarch64) {
        Ok(()) => QEMU_ARM_VIRT_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(all(
    target_arch = "aarch64",
    not(feature = "board-rpi3"),
    not(feature = "board-rpi4")
))]
pub(crate) fn active() -> &'static BoardDescriptor {
    selected_qemu_arm_virt()
}

#[cfg(all(
    target_arch = "aarch64",
    feature = "board-rpi4",
    not(feature = "board-rpi3")
))]
const RPI4_BOARD: &BoardDescriptor = &cellos_boards::raspberry_pi_4_model_b::RASPBERRY_PI_4_MODEL_B;

#[cfg(all(
    target_arch = "aarch64",
    feature = "board-rpi4",
    not(feature = "board-rpi3")
))]
pub(crate) const fn default_rpi4_board() -> &'static BoardDescriptor {
    RPI4_BOARD
}

#[cfg(all(
    target_arch = "aarch64",
    feature = "board-rpi4",
    not(feature = "board-rpi3")
))]
pub(crate) fn selected_rpi4() -> &'static BoardDescriptor {
    match RPI4_BOARD.validate_for(Architecture::Aarch64) {
        Ok(()) => RPI4_BOARD,
        Err(error) => invalid_descriptor(error),
    }
}

#[cfg(all(
    target_arch = "aarch64",
    feature = "board-rpi4",
    not(feature = "board-rpi3")
))]
pub(crate) fn active() -> &'static BoardDescriptor {
    selected_rpi4()
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::PlatformIdentity;
    use crate::acpi::AcpiOem;
    use crate::smbios::{SmbiosString, SmbiosSystem};
    use alloc::format;

    #[test]
    fn identity_line_names_every_fact() {
        let identity = PlatformIdentity {
            acpi_oem: Some(AcpiOem {
                oem_id: *b"BOCHS ",
                oem_table_id: *b"BXPC    ",
                oem_revision: 1,
            }),
            smbios: Some(SmbiosSystem {
                manufacturer: SmbiosString::new(b"QEMU"),
                product: SmbiosString::new(b"Standard PC (Q35 + ICH9, 2009)"),
                uuid: Some([
                    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC,
                    0xDD, 0xEE, 0xFF,
                ]),
            }),
            hypervisor: true,
        };
        assert_eq!(
            format!("{identity}"),
            "acpi_oem=\"BOCHS \" acpi_table=\"BXPC    \" acpi_rev=0x1 \
             smbios=manufacturer=\"QEMU\" product=\"Standard PC (Q35 + ICH9, 2009)\" \
             uuid=00112233-4455-6677-8899-aabbccddeeff hv=1"
        );
    }

    #[test]
    fn identity_line_escapes_firmware_bytes_and_names_absence() {
        let identity = PlatformIdentity {
            acpi_oem: None,
            smbios: Some(SmbiosSystem {
                manufacturer: SmbiosString::new(b"a\"b\\c\n[x] {}%s\xff"),
                product: SmbiosString::EMPTY,
                uuid: None,
            }),
            hypervisor: false,
        };
        assert_eq!(
            format!("{identity}"),
            "acpi_oem=none smbios=manufacturer=\"a\\\"b\\\\c\\x0a[x] {}%s\\xff\" product=\"\" \
             uuid=none hv=0"
        );
        let none = PlatformIdentity {
            acpi_oem: None,
            smbios: None,
            hypervisor: false,
        };
        assert_eq!(format!("{none}"), "acpi_oem=none smbios=none hv=0");
    }
}
