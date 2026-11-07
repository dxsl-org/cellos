FROM ubuntu:24.04

# Install dependencies
RUN apt update
RUN apt install -y build-essential curl git wget
RUN apt install -y qemu-system-aarch64 dosfstools mtools

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup.sh && \
    sh /tmp/rustup.sh -y && \
    rm /tmp/rustup.sh
ENV PATH="/root/.cargo/bin:${PATH}"

# Copy just ./scripts/
RUN mkdir /moss
WORKDIR /moss
RUN mkdir /moss/scripts
COPY ./scripts /moss/scripts
RUN ./scripts/download-arm-toolchain.sh /tmp/arm-toolchain.tar.xz
RUN mkdir -p /opt/arm-toolchain
RUN tar -xf /tmp/arm-toolchain.tar.xz -C /opt/arm-toolchain --strip-components=1

# Copy the current directory contents into the container at /moss
COPY . /moss

# Install ARM toolchain
ENV PATH="/opt/arm-toolchain/bin:${PATH}"

# Install dependencies
RUN ./scripts/build-deps.sh
RUN ./scripts/create-image.sh
RUN cargo build --release
