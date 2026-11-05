// SPDX-License-Identifier: MIT
// mlibc static start-up for Cellos Cells (owner decision B, 2026-11-04;
// .agents/261104-0802-x86-tier2-gpu-runtime/cp1-cell-initial-stack-auxv.md).
//
// Sequence contract: ostd's `_start` calls __cellos_libc_start(entry_sp) instead
// of walking .init_array, then calls `main` as before. Nothing mlibc-owned has
// run yet. On return mlibc has set up the Tcb, the static TLS block from
// PT_TLS, the thread pointer, the stack guard and argv/environ, and has run
// .preinit_array and .init_array exactly once.
//
// The kernel's initial stack is Linux-shaped (argc, argv, envp, auxv) with
// AT_PHDR pointing at a copy of the program headers and AT_BASE holding the
// Cell's load base. Before handing it to mlibc (`__dlapi_enter`), AT_PHDR and
// AT_PHNUM are redirected to a filtered copy:
//   - PT_DYNAMIC and PT_INTERP are dropped: the loader already applied every
//     relocation, and mlibc would process .rela.dyn again (into RELRO pages);
//   - a synthetic PT_PHDR leads the copy, with p_vaddr = copy − load base,
//     because that is how mlibc derives a PIE's base address.

#include <abi-bits/auxv.h>
#include <bits/ensure.h>
#include <elf.h>
#include <stddef.h>
#include <stdint.h>

extern "C" void __dlapi_enter(uintptr_t *entry_stack);

namespace {

constexpr size_t max_phdrs = 16;
Elf64_Phdr phdr_copy[max_phdrs + 1];
bool started;

} // namespace

// `libs/mlibc-shim` references this symbol so the linker extracts this object
// from libc.a: ostd already supplies a weak `__cellos_libc_start`, and a weak
// definition never makes an archive member load. Once loaded, the strong
// definition below replaces ostd's default.
extern "C" void __cellos_libc_start_anchor() {}

extern "C" void __cellos_libc_start(uintptr_t *entry_sp) {
    __ensure(!__atomic_exchange_n(&started, true, __ATOMIC_ACQ_REL));

    uintptr_t *aux = entry_sp + 1 + entry_sp[0] + 1; // skip argc, argv, NULL
    while (*aux)
        aux++;                                        // skip envp
    aux++;

    uintptr_t *phdr_slot = nullptr, *phnum_slot = nullptr;
    const Elf64_Phdr *phdrs = nullptr;
    size_t phnum = 0;
    uintptr_t base = 0;
    bool have_base = false;
    for (uintptr_t *a = aux; a[0] != AT_NULL; a += 2) {
        switch (a[0]) {
        case AT_PHDR: phdr_slot = &a[1]; phdrs = reinterpret_cast<const Elf64_Phdr *>(a[1]); break;
        case AT_PHNUM: phnum_slot = &a[1]; phnum = a[1]; break;
        case AT_PHENT: __ensure(a[1] == sizeof(Elf64_Phdr)); break;
        case AT_BASE: base = a[1]; have_base = true; break;
        }
    }
    __ensure(phdr_slot && phnum_slot && have_base && phnum <= max_phdrs);

    size_t n = 1;
    for (size_t i = 0; i < phnum; i++) {
        uint32_t type = phdrs[i].p_type;
        if (type != PT_DYNAMIC && type != PT_INTERP && type != PT_PHDR)
            phdr_copy[n++] = phdrs[i];
    }
    Elf64_Phdr &self = phdr_copy[0];
    self = {};
    self.p_type = PT_PHDR;
    self.p_flags = PF_R;
    self.p_vaddr = reinterpret_cast<uintptr_t>(phdr_copy) - base;
    self.p_memsz = self.p_filesz = n * sizeof(Elf64_Phdr);
    self.p_align = 8;

    *phdr_slot = reinterpret_cast<uintptr_t>(phdr_copy);
    *phnum_slot = n;
    __dlapi_enter(entry_sp);
}
