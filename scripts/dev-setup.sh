#!/usr/bin/env bash
# dev-setup.sh — One-command Cellos development environment setup.
#
# Supports: Ubuntu 22.04 / 24.04 (incl. WSL2), Debian 12.
# Idempotent: safe to re-run; never clobbers an existing .cargo/config.toml.
#
# The package and toolchain list mirrors .github/workflows/ci.yml — that workflow
# is the authority on what a working environment needs, because it is the only
# environment every merge is actually validated in.
#
# Usage:
#   ./scripts/dev-setup.sh          # install + configure + verify
#   ./scripts/dev-setup.sh --check  # verify only, install nothing
#   ./scripts/dev-setup.sh --help
#
# Not supported on Windows. The image-assembly scripts are POSIX-only: under Git
# Bash, MSYS rewrites their `/bin/...` arguments into Windows paths and the VIFS1
# image comes out with no /bin at all. Use WSL2.

set -euo pipefail

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[setup]${NC} $*"; }
warn()  { echo -e "${YELLOW}[warn] ${NC} $*"; }
die()   { echo -e "${RED}[error]${NC} $*" >&2; exit 1; }
step()  { echo -e "\n${BOLD}=== $* ===${NC}"; }

CHECK_ONLY=false
for arg in "$@"; do
  case $arg in
    --check) CHECK_ONLY=true ;;
    --help|-h)
      echo "Usage: $0 [--check] [--help]"
      echo "  --check   Verify the environment without installing anything."
      exit 0 ;;
  esac
done

OS=$(uname -s)
[[ "$OS" == "Linux" ]] || die "Linux only (WSL2 counts). Detected: $OS"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

step "Cellos developer setup"
info "OS:   $OS $(uname -m)"
info "Repo: $REPO_ROOT"

# Building on a Windows drive through WSL's drvfs translation layer is both slow
# (target/ reaches tens of GB of small-file I/O) and semantically lossy — the same
# path rewriting that corrupts VIFS1 images lives there. Warn loudly rather than
# let it be discovered as a mystery failure later.
case "$REPO_ROOT" in
  /mnt/*) warn "Repo is on a Windows drive ($REPO_ROOT) via drvfs."
          warn "Clone into the Linux filesystem instead, e.g. ~/cellos — builds are"
          warn "far faster there and the image scripts behave correctly." ;;
esac

TOOLCHAIN=$(grep 'channel' rust-toolchain.toml 2>/dev/null | cut -d'"' -f2 || echo "nightly")
info "Pinned toolchain: $TOOLCHAIN"

# ── 1. Rust ───────────────────────────────────────────────────────────────────
step "1/6 Rust toolchain"
if ! command -v rustup &>/dev/null; then
  $CHECK_ONLY && die "rustup not found — run without --check to install it"
  info "Installing rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi
info "$(rustup --version 2>/dev/null | head -1)"

if ! $CHECK_ONLY; then
  rustup toolchain install "$TOOLCHAIN" --allow-downgrade
  rustup component add rust-src rustfmt clippy llvm-tools-preview
  # -Z build-std compiles core/alloc from source, so a missing target is not fatal
  # — but installing them keeps plain `cargo build` and rust-analyzer happy.
  rustup target add \
    riscv64gc-unknown-none-elf \
    aarch64-unknown-none-softfloat \
    x86_64-unknown-none
fi

# ── 2. System packages ────────────────────────────────────────────────────────
step "2/6 System packages"
# Union of every apt-get install in ci.yml. The two easy mistakes:
#   * gcc-riscv64-unknown-elf is the BARE-METAL cross compiler. gcc-riscv64-linux-gnu
#     targets hosted Linux and cannot link freestanding — littlefs2-sys needs the
#     former (CI sets CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc).
#   * libclang-dev is not optional: littlefs2-sys runs bindgen for the LFS struct
#     layout, independently of the CC_* override.
APT_PKGS=(
  qemu-system-misc          # riscv64
  qemu-system-arm           # aarch64 + hypervisor lanes
  qemu-system-x86           # x86_64
  xorriso                   # Limine ISO for the x86_64 boot test
  gcc-riscv64-unknown-elf   # bare-metal riscv64 (littlefs2-sys C core)
  gcc-aarch64-linux-gnu     # supplies /usr/aarch64-linux-gnu, the bindgen sysroot
  clang                     # aarch64 cells cross-compile the C core with clang
  libclang-dev              # bindgen
  python3 mtools dosfstools make curl git
)

if $CHECK_ONLY; then
  for cmd in qemu-system-riscv64 qemu-system-aarch64 qemu-system-x86_64 \
             riscv64-unknown-elf-gcc clang python3 make; do
    command -v "$cmd" &>/dev/null && info "$cmd ✓" || warn "$cmd NOT FOUND"
  done
else
  command -v apt-get &>/dev/null || die "apt-get not found — install manually: ${APT_PKGS[*]}"
  sudo apt-get update -q
  sudo apt-get install -y -q "${APT_PKGS[@]}"
fi

# pwsh is deliberately NOT installed here: it needs a third-party apt repository,
# which is a supply-chain decision to make consciously. gen_disk.ps1 (used by the
# boot-suite lane) runs under pwsh on Linux — install it yourself if you need that
# lane locally. Everything else is plain bash.
command -v pwsh &>/dev/null || warn "pwsh absent — gen_disk.ps1 lanes unavailable (optional)"

# ── 3. .cargo/config.toml ─────────────────────────────────────────────────────
step "3/6 Cargo configuration"
# Gitignored (machine-specific absolute paths), so it does not arrive with a clone.
# Without it every target gets default codegen flags and links wrong — silently.
CARGO_CFG=".cargo/config.toml"
if [[ -f "$CARGO_CFG" ]]; then
  info "$CARGO_CFG exists — left untouched"
  grep -q 'relocation-model' "$CARGO_CFG" \
    || warn "$CARGO_CFG has no relocation-model — compare against scripts/cargo-config-linux.toml"
elif $CHECK_ONLY; then
  warn "$CARGO_CFG MISSING — builds will use wrong codegen flags"
else
  mkdir -p .cargo
  sed "s|@REPO_ROOT@|$REPO_ROOT|g" scripts/cargo-config-linux.toml > "$CARGO_CFG"
  info "wrote $CARGO_CFG from scripts/cargo-config-linux.toml"
fi

# ── 4. Host unit tests ────────────────────────────────────────────────────────
step "4/6 Host unit tests"
# The three crates that define no panic/alloc lang items and so can run on the
# host. This is the same command the `unit-tests` CI job runs, and it is the
# cheapest proof that the toolchain works at all.
if $CHECK_ONLY; then
  warn "skipped in --check mode"
else
  cargo test -p types -p api -p text-engine --target x86_64-unknown-linux-gnu \
    || die "host unit tests failed — the toolchain or config is wrong, stop here"
  info "host unit tests ✓"
fi

# ── 5. Bootable ramdisk + kernel ──────────────────────────────────────────────
step "5/6 Bootable image"
# kernel_fs.img is a gitignored build artifact. Without it build.rs embeds an EMPTY
# STUB: the kernel compiles, boots, mounts VIFS1 — and finds no cells. That failure
# looks like a code bug and is not one, so generate it as part of setup.
if $CHECK_ONLY; then
  [[ -s kernel/src/embedded/kernel_fs.img ]] \
    && info "kernel/src/embedded/kernel_fs.img present ✓" \
    || warn "kernel_fs.img MISSING or empty — kernel would boot with no cells"
else
  bash scripts/build-boot-ramdisk-ci.sh || die "ramdisk assembly failed"
  cargo build --release -p vicell-kernel \
    --target riscv64gc-unknown-none-elf -Z build-std=core,alloc \
    || die "kernel build failed"
  info "kernel + ramdisk built ✓"
fi

# ── 6. Boot verification ──────────────────────────────────────────────────────
step "6/6 QEMU boot check"
if $CHECK_ONLY; then
  warn "skipped in --check mode"
elif command -v qemu-system-riscv64 &>/dev/null; then
  bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel \
    || die "boot test failed — the environment builds but does not run"
  info "rv64 boot ✓"
else
  warn "qemu-system-riscv64 not in PATH — boot test skipped"
fi

step "Done"
echo ""
echo -e "${GREEN}${BOLD}Cellos development environment is ready.${NC}"
echo ""
echo "  Host tests:      cargo test -p types -p api -p text-engine --target x86_64-unknown-linux-gnu"
echo "  Lint (as CI):    cargo clippy --workspace --exclude app-mlibc-smoke --exclude doom \\"
echo "                     --exclude tetris-c --exclude lua --exclude tetris-lua \\"
echo "                     --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings"
echo "  Shell scenarios: bash scripts/build-shell-test-ci.sh && \\"
echo "                     (cd tests/integration && cargo test --test shell-utils)"
echo "  VFS suite:       bash scripts/build-test-hooks-ci.sh && \\"
echo "                     (cd tests/integration && cargo test --test vfs-quota)"
echo "  Re-verify env:   ./scripts/dev-setup.sh --check"
echo ""
echo "  Architecture:    docs/system-architecture.md"
echo "  Standards:       docs/code-standards.md"
echo "  Agent rules:     CLAUDE.md"
echo ""
