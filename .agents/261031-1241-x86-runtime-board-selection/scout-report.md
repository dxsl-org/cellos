# Scout report — x86 runtime selection

Full recon: `local://x86-recon.md` (session artifact). Decision-bearing facts:

- `board-x86-pc` uses: `kernel/src/board.rs:57-63`, `kernel/Cargo.toml:130-134`, `scripts/check-board-configs.sh:132-154`, `.github/workflows/ci.yml:1000-1018`, `boards/pc/x86_64-pc/README.md:17-19`, `docs/hardware-compatibility-list.md:25-30`, `docs/guides/ocel-viewer.md:89-95`, `docs/roadmap/hardware-tracks.md:47-50`, `docs/roadmap/current-focus.md:640-644`. Changelog and roadmap entries are historical; leave them.
- Pre-ACPI consumers: `main.rs:170-218` (COM ports), `main.rs:390-417` (firmware windows). Both profiles are byte-identical here (`hal/soc/x86/src/lib.rs:191-237`).
- Post-ACPI consumers: `main.rs:448-470` (has_driver gates), `main.rs:817-832`, `task/syscall.rs:1993-1997`, `task/drivers/iommu.rs:146-174` (`dma_isolation`), `task/drivers/iommu_x86.rs:438-457` (Q35 VT-d base fallback keyed on `soc`).
- Identity today: the RSDP `oem_id` is parsed but dropped (`acpi.rs:81-99`); the SDT header OEM fields are not retained; FADT is not dispatched; there is no SMBIOS support and no Limine command-line request.
- Persistence: the cell-store is read-only; snapshot is skipped on x86; writable storage exists only via VFS after the driver Cells start.
- Tests: `boards/src/catalog_tests.rs:45-115`; CI builds a separate PC ISO for `iommu-dmar-x86`; QEMU runner is q35-only (`scripts/qemu-x86_64-test.sh`).
