// SPDX-License-Identifier: MPL-2.0
//! `board`: the SoC policy of the active riscv64 board.

/// Returns the SoC policy paired with the active RISC-V board descriptor.
pub(crate) fn soc() -> &'static hal_soc_riscv::RiscvSocProfile {
    use cellos_boards::SocId;

    match crate::board::active().soc {
        SocId::GenericRiscvVirt => &hal_soc_riscv::GENERIC_VIRT,
        SocId::Jh7110 => &hal_soc_riscv::JH7110,
        SocId::Sg2042 => &hal_soc_riscv::SG2042,
        _ => panic!("[board] RISC-V descriptor has incompatible SoC identity"),
    }
}
