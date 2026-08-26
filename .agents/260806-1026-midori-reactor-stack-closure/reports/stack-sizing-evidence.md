# Phase 07 Stack Sizing Evidence

Status: authoritative input captured after the parked executor and dual-guard stack landed.

## Selection Rule

- Page size: 4,096 bytes.
- Observed peak: maximum of the kernel-stack and user-stack watermarks for the path.
- Required pages: `ceil(2 * observed_peak / 4096)`.
- Conservative floor: 16 usable pages (65,536 bytes).
- Selected pages: `max(required_pages, 16)`.
- Every stack also reserves the two Phase 06 guard pages; guards are not counted as usable.
- Unmeasured names remain on the 64-page default.

## Results

| Path | Kernel bytes | User bytes | Peak pages | 2x pages | Selected pages | QEMU log |
|---|---:|---:|---:|---:|---:|---|
| `init` | 13,688 | 2,136 | 4 | 7 | 16 | [raw transcript](#raw-qemu-transcript) |
| `shell` | 4,992 | 15,592 | 4 | 8 | 16 | [raw transcript](#raw-qemu-transcript) |
| `vfs` | 5,808 | 31,744 | 8 | 16 | 16 | [raw transcript](#raw-qemu-transcript) |
| `vfs-test` | 3,736 | 5,672 | 2 | 3 | 16 | [raw transcript](#raw-qemu-transcript) |
| `net` | 4,400 | 15,524 | 4 | 8 | 16 | [raw transcript](#raw-qemu-transcript) |
| `virtio-net` | 3,960 | 10,840 | 3 | 6 | 16 | [raw transcript](#raw-qemu-transcript) |

The final capture was taken after applying the 16-page table. Every measured path therefore completed its representative workload inside the selected allocation, not only inside the old 64-page baseline.

## Validation

- Capture command: `CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --manifest-path tests/integration/Cargo.toml --test boot stack_sizing_paths_emit_kernel_and_user_watermarks -- --exact --nocapture --test-threads=1`.
- Workload before the 15-second sample: service boot, DHCP/NIC activity, VFS integration suite, and shell `help`, `ls`, `ps`.
- Unknown fallback proof: `stack-sizing policy self-test PASS (measured=16, unknown=64)`.
- RV64 production: shell burst, DHCP, TCP send/receive, and VFS redirect tests PASS.
- Production boot: RV64 PASS; AArch64 PASS; x86_64 PASS after the PCIe-only VirtIO-MMIO enumeration bug was fixed.
- No manifest/public ABI field was added.

## Raw QEMU Transcript

```text
[stack-baseline] name=vfs-test phase=exit kind=kernel used_bytes=3736 used_pages=1 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=vfs-test phase=exit kind=user used_bytes=5672 used_pages=2 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=init phase=boot kind=kernel used_bytes=13688 used_pages=4 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=init phase=boot kind=user used_bytes=2136 used_pages=1 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=vfs phase=boot kind=kernel used_bytes=5808 used_pages=2 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=vfs phase=boot kind=user used_bytes=31744 used_pages=8 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=virtio-net phase=boot kind=kernel used_bytes=3960 used_pages=1 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=virtio-net phase=boot kind=user used_bytes=10840 used_pages=3 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=net phase=boot kind=kernel used_bytes=4400 used_pages=2 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=net phase=boot kind=user used_bytes=15524 used_pages=4 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=shell phase=boot kind=kernel used_bytes=4992 used_pages=2 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
[stack-baseline] name=shell phase=boot kind=user used_bytes=15592 used_pages=4 alloc_bytes=73728 usable_bytes=65536 baseline=authoritative-input
```
