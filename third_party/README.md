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

## monty

**Upstream**: https://github.com/pydantic/monty  
**Pin**: `5f4a3dfc493538bb9dc319974393391ba9e7cf14` (tag `v1.7.0-beta.1`, 2026-11-20), set in `scripts/fetch-monty.sh`  
**License**: MIT  
**Purpose**: Python sandbox interpreter for Tier-2 `UNTRUSTED` std cells (witness: `cells/demos/monty-smoke`)

Not vendored. `scripts/fetch-monty.sh` fetches the pin into the gitignored `third_party/monty-src/`
and applies `third_party/monty/patches/*.patch` in order. Cells depend on
`third_party/monty-src/crates/{monty,monty-types}` by path. Python language features go
upstream; this patch set carries platform fixes only.

| Patch | Purpose | Upstreamable |
|---|---|---|
| `0001-gate-ruff-stacker-off-cellos.patch` | Enables `ruff_python_parser/stacker` everywhere except `target_os = "cellos"`; `stacker`/`psm` need `mmap`/`mprotect` and per-OS asm. Hosts on cellos must compile with `source_scan_threshold = 0`, so Monty's nesting pre-scan bounds parser recursion; the parser then recurses on the cell's native stack, which is why the kernel gives Monty paths a 2 MiB user stack (`task::LARGE_STACK_PAGES`). | Yes. Upstream carries a TODO to drop stacker for recursion limits (`Cargo.toml:81-83`). |
| `jiter/0001-gate-x86_64-simd-on-sse2.patch` | jiter 0.19.0 (Monty's JSON parser, crates.io tarball checked against the cell's `Cargo.lock` checksum) uses SSE2 intrinsics on every x86_64 target; the cellos x86_64 spec is soft-float with SSE disabled, where LLVM cannot lower them. The patch requires `target_feature = "sse2"` for those paths, so cellos takes jiter's scalar fallback. Wired in by `cells/demos/monty-smoke` `[patch.crates-io]`. | Yes, to jiter: SSE2 is not part of every x86_64 target. |

The witness cell also selects `sha1`/`sha2`'s portable backends (`--cfg sha{1,2}_backend="soft"`) for the same
reason, and builds at `opt-level = "s"`: the kernel copies a signed ELF into its heap to verify it, and a 12.9 MB
`-O3` image did not fit (8.2 MB does).

Moving the pin: edit `MONTY_PIN`, run the script (each patch must `git apply` cleanly), then rerun
the witness `scripts/x86/qemu-monty-smoke.sh` and `--link-only` for riscv64/aarch64.

```sh
bash scripts/fetch-monty.sh
```
