# SPDX-License-Identifier: MIT
# ViCell mlibc sysdeps — Meson build fragment.
# Included by third_party/mlibc-src/meson.build when host_machine.system() == 'vicell'.
#
# Build:
#   Windows: pwsh scripts/setup-mlibc.ps1
#   WSL2:    bash scripts/build-mlibc.sh

# ─── Supported options ─────────────────────────────────────────────────────────
# posix: on since runtime plan R3 (threads, futex, time, sysconf, environment,
# dl_iterate_phdr). Sysdeps vicell does not implement return ENOSYS through
# mlibc::sysdep_or_enosys. linux/glibc/bsd stay off: Cellos is not Linux.
sysdep_supported_options = {
    'posix': true,
    'linux': false,
    'glibc': false,
    'bsd':   false,
}

# ─── Compile flags ────────────────────────────────────────────────────────────
# -fno-exceptions/-fno-rtti: required; mlibc internals (frg) are exception-free.
# -fno-stack-protector: no __stack_chk_fail in freestanding mode.
# -nostdlib: we are the stdlib; do not pull in another one.
#
# Bare-metal fast-int override (riscv-none-elf-gcc vs mlibc Linux ABI):
#   riscv-none-elf-gcc defines __INT_FAST{8,16,32}_TYPE__ = int (32-bit).
#   mlibc's bits/types.h RISC-V section defines int_fast8 = int8_t (8-bit)
#   and int_fast{16,32} = int64_t (64-bit) — matching the Linux/glibc ABI.
#   __MLIBC_CHECK_TYPE runs for GCC and fails on bare-metal targets.
#   We override the predefined macros to match mlibc's expectations.
add_project_arguments(
    [
        '-fno-exceptions', '-fno-rtti', '-fno-stack-protector', '-nostdlib', '-Os',
        '-D__INT_FAST8_TYPE__=__INT8_TYPE__',
        '-D__UINT_FAST8_TYPE__=__UINT8_TYPE__',
        '-D__INT_FAST16_TYPE__=__INT64_TYPE__',
        '-D__UINT_FAST16_TYPE__=__UINT64_TYPE__',
        '-D__INT_FAST32_TYPE__=__INT64_TYPE__',
        '-D__UINT_FAST32_TYPE__=__UINT64_TYPE__',
    ],
    language: ['c', 'cpp'],
    native: false,
)

# mlibc's generic C++ (locale, nl_item) relies on the GNU names that a Linux
# g++ predefines through _GNU_SOURCE; bare-metal elf toolchains do not.
add_project_arguments('-D_GNU_SOURCE', language: 'cpp', native: false)

# ─── Include paths ─────────────────────────────────────────────────────────────
libc_include_dirs += include_directories('include')
# mlibc builds its rtld objects (ld.a) for static programs too; they include
# <abi-bits/*> and call sysdeps.
rtld_include_dirs += include_directories('include')
# The sysdep sources go to the shared ld.so only (as in upstream's Linux
# sysdeps): a static libc.a already merges ld.a's objects with libc's own, so
# listing them in rtld_sources would put every sysdep in libc.a twice.
rtld_dso_sources += files('generic/generic.cpp', 'generic/thread.cpp')

# ─── Sources ──────────────────────────────────────────────────────────────────
libc_sources += files(
    'generic/generic.cpp',
    'generic/thread.cpp',
    'generic/posix.cpp',
    'generic/entry.cpp',
    'generic/startup.cpp',
    'thread-entry' / host_machine.cpu_family() + '.S',
)

# ─── CRT object (empty stub — ostd provides the real _start) ─────────────────
if not headers_only
    if host_machine.cpu_family() == 'riscv64'
        crt_asm = 'crt-riscv64/crt1.S'
    elif host_machine.cpu_family() == 'aarch64'
        crt_asm = 'crt-aarch64/crt1.S'
    elif host_machine.cpu_family() == 'x86_64'
        crt_asm = 'crt-x86_64/crt1.S'
    else
        error('vicell sysdeps: unsupported CPU family: ' + host_machine.cpu_family())
    endif

    crt = custom_target('crt1',
        build_by_default: true,
        command: c_compiler.cmd_array() + ['-c', '-o', '@OUTPUT@', '@INPUT@'],
        input: crt_asm,
        output: 'crt1.o',
        install: false,
    )
endif

# ─── Installed headers (scripts/build-mlibc.sh → <row>/sysroot/include) ───────
if not no_headers
    install_subdir('include/abi-bits', install_dir: get_option('includedir'))
endif
