# QEMU Virt AArch64

This package records the immutable QEMU `virt` identity, boot contract, safe
256 MiB fallback map, and shared-driver selection. Early boot still sizes the
kernel span from the linker and accepts firmware DTB RAM as authoritative.
The checked DTS is intentionally memory/console-only; it is not a substitute
for firmware discovery of RTC, VirtIO, or PCIe. Their immutable fallback layout
lives in the QEMU ARM virt SoC profile rather than being duplicated here.

Rebuild the checked configuration with:

```sh
cargo build -p cellos-kernel --target aarch64-unknown-none-softfloat
```
