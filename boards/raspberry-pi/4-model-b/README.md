# Raspberry Pi 4 Model B

VideoCore must provide the live DTB. The fallback map deliberately covers only
the low 1 GiB-safe window; the descriptor does not claim hardware boot proof.

```sh
cargo build -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi4
```
