#!/usr/bin/env bash
# dev-setup.sh — One-command ViOS development environment setup.
#
# Supports: Ubuntu 22.04 / 24.04, Debian 12, macOS 14+ (Homebrew required)
# Idempotent: safe to run multiple times.
#
# Usage:
#   ./scripts/dev-setup.sh          # install everything
#   ./scripts/dev-setup.sh --check  # verify existing install without installing
#   ./scripts/dev-setup.sh --help   # show this help

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
ARCH=$(uname -m)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

step "ViOS Developer Setup"
info "OS: $OS $ARCH"
info "Repo: $REPO_ROOT"
cd "$REPO_ROOT"

# ── Read pinned toolchain version ──────────────────────────────────────────────
TOOLCHAIN=$(grep 'channel' rust-toolchain.toml 2>/dev/null | cut -d'"' -f2 || echo "nightly")
info "Pinned Rust toolchain: $TOOLCHAIN"

# ── 1. Rustup ─────────────────────────────────────────────────────────────────
step "1/5 Rust toolchain"
if ! command -v rustup &>/dev/null; then
  if $CHECK_ONLY; then die "rustup not found — run without --check to install"; fi
  info "Installing rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi
info "rustup $(rustup --version 2>/dev/null | head -1)"

if ! $CHECK_ONLY; then
  rustup toolchain install "$TOOLCHAIN" --allow-downgrade
  rustup component add rust-src rustfmt clippy llvm-tools-preview
  rustup target add \
    riscv64gc-unknown-none-elf \
    aarch64-unknown-none \
    x86_64-unknown-none
fi

# Verify
rustup show active-toolchain 2>/dev/null | grep -q "nightly" \
  || warn "Expected nightly toolchain active — check rust-toolchain.toml"

# ── 2. System packages ─────────────────────────────────────────────────────────
step "2/5 System packages"
if $CHECK_ONLY; then
  for cmd in qemu-system-riscv64 python3 make; do
    command -v "$cmd" &>/dev/null && info "$cmd ✓" || warn "$cmd not found"
  done
else
  case "$OS" in
    Linux)
      # Detect package manager
      if command -v apt-get &>/dev/null; then
        info "Using apt-get..."
        sudo apt-get update -q
        sudo apt-get install -y -q \
          qemu-system-misc \
          gcc-riscv64-linux-gnu \
          gcc-aarch64-linux-gnu \
          mtools \
          dosfstools \
          python3 \
          make \
          curl \
          git
      elif command -v dnf &>/dev/null; then
        info "Using dnf..."
        sudo dnf install -y \
          qemu-system-riscv \
          gcc-riscv64-linux-gnu \
          mtools \
          python3 \
          make \
          curl \
          git
      else
        warn "Unknown Linux package manager — install manually:"
        echo "  qemu-system-riscv64, mtools, python3, make"
      fi
      ;;
    Darwin)
      if ! command -v brew &>/dev/null; then
        die "Homebrew required on macOS — see https://brew.sh"
      fi
      info "Using Homebrew..."
      brew install qemu mtools riscv-gnu-toolchain python3 make || true
      ;;
    *)
      warn "Unsupported OS: $OS — install qemu-system-riscv64 manually"
      ;;
  esac
fi

# ── 3. Optional: QEMU version check ───────────────────────────────────────────
step "3/5 QEMU"
if command -v qemu-system-riscv64 &>/dev/null; then
  QVER=$(qemu-system-riscv64 --version | head -1)
  info "$QVER ✓"
else
  warn "qemu-system-riscv64 not in PATH — QEMU boot tests will be skipped"
fi

# ── 4. Cargo check (smoke build) ──────────────────────────────────────────────
step "4/5 Cargo workspace check"
if cargo check --workspace 2>&1 | tail -3; then
  info "cargo check ✓"
else
  die "cargo check failed — see output above"
fi

# ── 5. Summary ────────────────────────────────────────────────────────────────
step "5/5 Done!"
echo ""
echo -e "${GREEN}${BOLD}ViOS development environment is ready.${NC}"
echo ""
echo "  Build kernel:   cargo build --release"
echo "  Generate disk:  ./gen_disk.ps1  (or adapt for Linux)"
echo "  Run in QEMU:    ./run.sh        (or ./run.ps1 on Windows)"
echo "  Smoke checks:   ./scripts/check-baseline.sh"
echo ""
echo "  First time?  Read docs/ONBOARDING.md"
echo "  Questions?   Open a GitHub Discussion"
