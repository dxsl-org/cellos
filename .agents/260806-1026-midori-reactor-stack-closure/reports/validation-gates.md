# Validation Gates

Run commands from `/home/dmin/cellos` in native WSL/Linux unless noted.

## Static Gates

```bash
cargo fmt --all --check
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api --target x86_64-unknown-linux-gnu
```

## RV64 Full Image + Boot

```bash
export CARGO_BUILD_TARGET=riscv64gc-unknown-none-elf
export CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export OBJCOPY=riscv64-unknown-elf-objcopy
set -o pipefail
pwsh ./gen_disk.ps1 2>&1 | tee gen_disk.log
! grep -i "FATAL" gen_disk.log
BOOT_WINDOW=120 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel disk_v3.img
```

If `gen_disk.ps1` output is not tee'd to `gen_disk.log`, inspect the terminal output for `FATAL`; the script may exit 0 after an inner failure.

## RV64 Test-Hooks

```bash
bash scripts/build-test-hooks-ci.sh
cd tests/integration
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test vfs-quota
```

## AArch64 Build + Boot

```bash
export CC_aarch64_unknown_none_softfloat=clang
export CFLAGS_aarch64_unknown_none_softfloat="--target=aarch64-unknown-none-elf -ffreestanding -mgeneral-regs-only -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_none_softfloat="--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu"
cargo build --release --target aarch64-unknown-none-softfloat -Z build-std=core,alloc \
  -p app-init -p app-shell -p service-vfs -p service-config \
  -p service-net -p service-input -p service-compositor \
  -p periph-demo -p input-test -p app-sys-tools
REL=target/aarch64-unknown-none-softfloat/release
python3 tools/mkfat32.py kernel/src/embedded-aarch64/kernel_fs.img \
  "$REL/app-shell" /bin/shell "$REL/service-vfs" /bin/vfs \
  "$REL/service-config" /bin/config "$REL/service-input" /bin/input \
  "$REL/input-test" /bin/input-test "$REL/periph-demo" /bin/periph-demo \
  "$REL/ls" /bin/ls "$REL/cat" /bin/cat "$REL/echo" /bin/echo \
  "$REL/ps" /bin/ps "$REL/kill" /bin/kill
cp "$REL/app-init" kernel/src/embedded-aarch64/init
cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc
bash scripts/format-disk-arm.sh disk_arm_virt.img
BOOT_WINDOW=90 bash scripts/qemu-aarch64-test.sh
```

## x86_64 Build + Boot

```bash
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none=cc
export CFLAGS_x86_64_unknown_none="-ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="--target=x86_64-linux-gnu"
cargo build --release --target x86_64-unknown-none -Z build-std=core,alloc \
  -p app-init -p app-shell -p service-vfs -p service-config \
  -p service-platform -p driver-nvme -p driver-e1000 -p app-sys-tools
REL=target/x86_64-unknown-none/release
python3 tools/mkfat32.py kernel/src/embedded-x86_64/kernel_fs.img \
  "$REL/app-shell" /bin/shell "$REL/service-vfs" /bin/vfs \
  "$REL/service-config" /bin/config "$REL/platform" /bin/platform \
  "$REL/driver-nvme" /bin/nvme "$REL/driver-e1000" /bin/e1000 \
  "$REL/ls" /bin/ls "$REL/cat" /bin/cat "$REL/echo" /bin/echo "$REL/ps" /bin/ps
cp "$REL/app-init" kernel/src/embedded-x86_64/init
cargo build --release -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc
bash scripts/x86/make-iso-ci.sh build/vicell-x86.iso
BOOT_WINDOW=90 bash scripts/qemu-x86_64-test.sh build/vicell-x86.iso
```

## Feature Evidence Markers

- Phase01: `[net-rx-producer] irq->completion PASS`
- Phase05: `[executor] parked PASS`
- Phase06: `[stack-overflow] guard/probe PASS`
- Phase07: `[stack-baseline] ... baseline=authoritative-input`
