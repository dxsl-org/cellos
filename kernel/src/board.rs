#[cfg(any(
    target_arch = "riscv64",
    all(target_arch = "aarch64", feature = "board-rpi3")
))]
use cellos_boards::{Architecture, BoardDescriptor, ValidationError};

#[cfg(target_arch = "riscv64")]
const DEFAULT_RISCV64_BOARD: &BoardDescriptor =
    &cellos_boards::qemu_virt_riscv64::QEMU_VIRT_RISCV64;

#[cfg(all(target_arch = "riscv64", not(feature = "board-vf2")))]
/// Returns the compiled-in QEMU RV64 descriptor used for audited fallback data.
pub(crate) const fn default_riscv64_board() -> &'static BoardDescriptor {
    DEFAULT_RISCV64_BOARD
}

#[cfg(target_arch = "riscv64")]
/// Returns the validated descriptor before early boot consumes MMIO or RAM ranges.
pub(crate) fn selected() -> &'static BoardDescriptor {
    match DEFAULT_RISCV64_BOARD.validate_for(Architecture::Riscv64) {
        Ok(()) => DEFAULT_RISCV64_BOARD,
        Err(error) => invalid_descriptor(error),
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
