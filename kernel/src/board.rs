#[cfg(any(
    target_arch = "riscv64",
    all(target_arch = "aarch64", feature = "board-rpi3")
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
    all(target_arch = "aarch64", feature = "board-rpi3")
))]
fn invalid_descriptor(error: ValidationError) -> ! {
    panic!("[board] invalid descriptor: {:?}", error)
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
