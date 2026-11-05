#!/usr/bin/env bash
# Host tests for libdrm-lite (runtime plan R6): builds the pinned libdrm subset,
# the Cellos enumeration backend, the fake provider and the vicell fd provider
# core for Linux with ASan/UBSan, and runs them through harness.c.
#
# Usage: bash libs/drm-cellos/tests/host/run.sh
# Env: CC (default gcc), OUT_DIR (default target/drm-cellos-host)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
CC="${CC:-gcc}"
OUT_DIR="${OUT_DIR:-$ROOT/target/drm-cellos-host}"
mkdir -p "$OUT_DIR"

# -U_FORTIFY_SOURCE: harness.c defines open(), which glibc's fortify headers
# would otherwise wrap inline.
"$CC" -std=gnu11 -O1 -g -Wall -Wextra -Werror -U_FORTIFY_SOURCE \
    -fsanitize=address,undefined -fno-sanitize-recover=all -pthread \
    -I"$ROOT/libs/drm-cellos/include" \
    -I"$ROOT/libs/drm-cellos/tests/host" \
    -I"$ROOT/third_party/libdrm" \
    -I"$ROOT/third_party/libdrm/include/drm" \
    -I"$ROOT/third_party/mlibc/sysdeps/vicell/include" \
    "$ROOT/third_party/libdrm/xf86drm.c" \
    "$ROOT/libs/drm-cellos/c/device.c" \
    "$ROOT/libs/drm-cellos/c/fake_provider.c" \
    "$HERE/harness.c" \
    "$HERE/test_drm_lite.c" \
    -o "$OUT_DIR/test_drm_lite"

"$OUT_DIR/test_drm_lite"
