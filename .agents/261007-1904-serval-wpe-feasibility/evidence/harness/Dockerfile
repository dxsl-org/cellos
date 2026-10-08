FROM debian:trixie

# Non-interactive apt
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential clang cmake ninja-build gperf ruby perl python3 python3-pip \
    libglib2.0-dev libsoup-3.0-dev libicu-dev libharfbuzz-dev libfreetype6-dev \
    libfontconfig1-dev libxml2-dev libsqlite3-dev libjpeg-dev libpng-dev libwebp-dev \
    libepoxy-dev libegl1-mesa-dev libxkbcommon-dev libinput-dev libdrm-dev \
    libsystemd-dev libgcrypt20-dev libtasn1-6-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev liblcms2-dev libxslt1-dev libwoff-dev \
    strace ltrace libatk1.0-dev libatk-bridge2.0-dev gobject-introspection libgirepository1.0-dev unifdef curl git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
