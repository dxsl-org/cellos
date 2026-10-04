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
