# Third-party vendored crates

Vendored forks of external crates adapted for Cellos's no_std/no_main bare-metal target.
These are NOT git submodules — full source snapshots, owned by this repo.

## redoxfs

**Upstream**: https://github.com/redox-os/redoxfs  
**Version**: 0.9.0  
**License**: MIT  
**Purpose**: RedoxFS CoW filesystem for VFS `/srv` mount point (see `docs/specs/09b-vfs-native-fs-adr.md`)

### Patch summary (diff from upstream 0.9.0)

`Cargo.toml` changes only — zero source code changes:

1. `libc = "0.2"` → `libc = { version = "0.2", optional = true }` (used only in std/FUSE modules)
2. `redox_syscall = "0.7.5"` → `{ version = "0.7.5", default-features = false }` (needed for `Disk` trait error types; bare-metal compatible)
3. All other deps already had `default-features = false`; added it to `bitflags`, `base64ct`, `endian-num`
4. `features.default = []` (was `["std", "log", "fuse"]`); `std` feature now gates `libc` + `redox_syscall/std`

### Building (no_std)

```sh
cargo check --no-default-features --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
```

### Building (std, for mkfs tooling)

```sh
cargo build --features std --bin redoxfs-mkfs --release
```

### Creating a `/srv` disk image for testing

```sh
# 64 MB blank image
dd if=/dev/zero of=srv.img bs=1M count=64
# Format with RedoxFS
./target/release/redoxfs-mkfs srv.img
# Seed with test data via FUSE (Linux)
mkdir /tmp/srv-mnt
./target/release/redoxfs srv.img /tmp/srv-mnt
echo "Cellos RedoxFS" > /tmp/srv-mnt/hello.txt
fusermount -u /tmp/srv-mnt
```

## mlibc

**Upstream**: https://github.com/managarm/mlibc  
**Pin**: `ae1092eb211e42e80d2d1c73881c8c25fde80fe6` (2026-03-26), set in `scripts/build-mlibc.sh` and `scripts/setup-mlibc.ps1`  
**License**: MIT  
**Purpose**: Tier-B C library for opt-in Tier-2 Cells (ADR-0018; x86_64 per the x86 Tier-2 runtime plan)

Not vendored. The build scripts fetch the pin into the gitignored `third_party/mlibc-src/`,
copy in the Cellos port `third_party/mlibc/sysdeps/vicell/` (upstream `Sysdeps<Tag>` API),
add a `vicell` branch to mlibc's `meson.build`, and build with the `posix` option set on.
Each row writes `libc.a`, the building compiler's `libgcc.a` and mlibc's installed headers
(`sysroot/include`) to `third_party/mlibc/build{,-aarch64,-x86_64}/` (gitignored).

The pin is the newest commit GCC 13 builds; the next upstream commit needs GCC 14
(C++23 explicit object parameters). Ubuntu's clang 18 cannot build this mlibc either
(C++20 alias-template deduction). Every `sysdeps/vicell/include/abi-bits/` header is a
copy of upstream `abis/linux/` at the pin (copies, not symlinks, so Windows checkouts
work): the `O_*`, `SEEK_*`, `PROT_*` and `MAP_*` values the sysdeps pass to the kernel
already equal Linux's, and `struct stat` is filled in C++, never shared with Rust.
Moving the pin means re-copying them.

```sh
bash scripts/build-mlibc.sh                      # Linux/WSL2: every row whose compiler exists
cargo build -p app-mlibc-smoke --release --target riscv64gc-unknown-none-elf
RUSTFLAGS="-C relocation-model=pic" \
  cargo build -p app-mlibc-smoke --release --target x86_64-unknown-none
```
