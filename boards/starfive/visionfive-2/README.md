# StarFive VisionFive 2

The firmware DTB is mandatory and authoritative for live peripheral discovery;
the checked fallback map preserves the existing JH7110 emergency boot contract.

```sh
RUSTFLAGS="-C relocation-model=pic" cargo build -p cellos-kernel --release --target riscv64imac-unknown-none-elf --features board-vf2
```
