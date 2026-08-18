# Milk-V Pioneer

The SG2042 firmware DTB is required. Console access is SBI DBCN because the
DesignWare UART physical address is outside the current Sv39 kernel window.

```sh
RUSTFLAGS="-C relocation-model=pic" cargo build -p cellos-kernel --release --target riscv64gc-unknown-none-elf --features board-pioneer
```
