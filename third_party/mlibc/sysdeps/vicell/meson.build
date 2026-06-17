# SPDX-License-Identifier: MIT
# ViCell mlibc sysdeps — Meson build fragment.
# Included by third_party/mlibc/meson.build when host_machine.system() == 'vicell'.
# Run from WSL2: see scripts/build-mlibc.sh for the full command sequence.

# ─── Compile flags ────────────────────────────────────────────────────────────
# -fno-exceptions/-fno-rtti: required; mlibc internals (frg) are exception-free.
# -fno-stack-protector: no __stack_chk_fail in freestanding mode.
# -nostdlib: we are the stdlib; do not pull in another one.
mlibc_c_flags += [
    '-fno-exceptions',
    '-fno-rtti',
    '-fno-stack-protector',
    '-nostdlib',
    '-Os',
]

# Architecture-specific march/mabi flags are supplied by the cross file
# ([properties] section) so we do not hard-code them here.

# ─── Include paths ─────────────────────────────────────────────────────────────
mlibc_include_dirs += include_directories('include')

# ─── Sources ──────────────────────────────────────────────────────────────────
mlibc_sources += files(
    'generic/generic.cpp',
    'generic/entry.cpp',
)

# ─── CRT objects (empty stubs — ostd provides the real _start) ───────────────
# The CPU-arch selection below prevents shipping the wrong arch's crt1.o.
if host_machine.cpu_family() == 'riscv64'
    mlibc_crt_sources = files('crt-riscv64/crt1.S')
elif host_machine.cpu_family() == 'aarch64'
    mlibc_crt_sources = files('crt-aarch64/crt1.S')
endif

# ─── Disable unused mlibc subsystems ─────────────────────────────────────────
# These are configured at the top-level meson setup:
#   -Dposix_option=disabled   (no pthreads, no socket wrappers)
#   -Dlinux_option=disabled   (no Linux-specific syscalls)
#   -Ddefault_library=static  (produce libc.a, not libc.so)
