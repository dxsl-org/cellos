# Cellos build environment: the packages and pinned toolchain of
# scripts/dev-setup.sh, which mirrors .github/workflows/ci.yml. The image build
# assembles the boot ramdisk and builds the riscv64 kernel as a smoke check.
#
#   docker build -t cellos-dev .
#   docker run --rm -it -v "$PWD":/cellos cellos-dev bash
FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl git make python3 python3-cryptography \
      mtools dosfstools xorriso \
      qemu-system-misc qemu-system-arm qemu-system-x86 \
      gcc-riscv64-unknown-elf gcc-aarch64-linux-gnu clang libclang-dev \
 && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain none
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /cellos
# Install the pinned toolchain, components and targets before copying the tree,
# so a source change does not invalidate this layer.
COPY rust-toolchain.toml ./
RUN rustup toolchain install && cargo --version

COPY . .
# .cargo/config.toml is gitignored; generate it as scripts/dev-setup.sh does.
RUN mkdir -p .cargo \
 && sed "s|@REPO_ROOT@|/cellos|g" scripts/cargo-config-linux.toml > .cargo/config.toml
RUN bash scripts/build-boot-ramdisk-ci.sh \
 && cargo build --release -p cellos-kernel \
      --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
