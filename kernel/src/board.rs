#[cfg(target_arch = "riscv64")]
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

#[cfg(target_arch = "riscv64")]
fn invalid_descriptor(error: ValidationError) -> ! {
    panic!("[board] invalid riscv64 descriptor: {:?}", error)
}
