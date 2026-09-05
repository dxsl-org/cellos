//! Bare-metal PCIe BAR-window validation for `test-hooks` kernels.

use super::valid_pcie_bar_window;

pub(super) fn run() -> bool {
    let cases = [
        (
            "accepts aligned 128 KiB BAR",
            valid_pcie_bar_window(0xF000_0000, 0x20_000),
        ),
        (
            "accepts aligned maximum 1 GiB BAR",
            valid_pcie_bar_window(0x8000_0000, 1 << 30),
        ),
        ("rejects zero base", !valid_pcie_bar_window(0, 0x4000)),
        (
            "rejects zero length",
            !valid_pcie_bar_window(0xF000_0000, 0),
        ),
        (
            "rejects base misalignment",
            !valid_pcie_bar_window(0xF000_1000, 0x4000),
        ),
        (
            "rejects non-power-of-two length",
            !valid_pcie_bar_window(0xF000_0000, 0x3000),
        ),
        (
            "rejects otherwise-valid BAR larger than 1 GiB",
            !valid_pcie_bar_window(0x8000_0000, 1usize << 31),
        ),
        (
            "rejects address overflow",
            !valid_pcie_bar_window(usize::MAX - 0xFFF, 0x1000),
        ),
    ];

    let mut ok = true;
    for (case, passed) in cases {
        if !passed {
            log::error!("[selftest] PCIE-BAR-WINDOW case failed: {}", case);
            ok = false;
        }
    }
    ok
}
