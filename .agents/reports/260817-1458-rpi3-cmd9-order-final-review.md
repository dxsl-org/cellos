**VERDICT:** PASS — CLEAR; the CMD9 ordering fix keeps SD in Standby for CSD reads while preserving the eMMC select-then-EXT_CSD sequence.

[POSITIVE] kernel/src/task/drivers/mmc/core.rs:96 — the shared initialization still performs CMD2 then CMD3 before card-specific capacity handling, avoiding duplicated RCA/CID setup logic.
[POSITIVE] kernel/src/task/drivers/mmc/core.rs:110 — `match card_type` cleanly owns the card-specific sequence without losing `card_type`; the final `CardInfo` construction still returns the detected type.
[POSITIVE] kernel/src/task/drivers/mmc/core.rs:111 — eMMC still runs CMD7 before switching to 25 MHz and reading EXT_CSD, preserving the Transfer-state requirement for CMD8/EXT_CSD data transfer.
[POSITIVE] kernel/src/task/drivers/mmc/core.rs:116 — SD now reads CSD with CMD9 before CMD7, so CSD is requested while the card remains in Standby.
[POSITIVE] kernel/src/task/drivers/mmc/core.rs:117 — SD capacity errors propagate before selection or high-speed clocking, avoiding a partially advanced state on failed CSD decode.
[POSITIVE] kernel/src/task/drivers/mmc/core.rs:118 — SD selection and 25 MHz clock switch occur only after CSD succeeds, leaving the success path in Transfer state as the public contract states.
[POSITIVE] kernel/src/task/drivers/mmc/core.rs:124 — `CardInfo` still carries `card_type`, `rca`, `sector_count`, and `is_block_addressed` from the single detected negotiation path.
[POSITIVE] tools/rpi3-netboot/test-netboot-scripts.ps1:140 — static guard asserts `sd_read_csd(rca)?` remains before `cmd7_select(rca)?`, covering the confirmed CMD9-order root cause.

Verification: `git diff --check -- kernel/src/task/drivers/mmc/core.rs tools/rpi3-netboot/test-netboot-scripts.ps1` PASS; `cargo check -p cellos-kernel --features board-rpi3 --target aarch64-unknown-none-softfloat` PASS with pre-existing warnings; `powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\rpi3-netboot\test-netboot-scripts.ps1` PASS.
