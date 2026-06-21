// SPDX-License-Identifier: MIT
// ViCell riscv64 CRT stub.
//
// ostd (libs/ostd) provides the real _start entry point and calls main().
// This file is intentionally empty — it exists so mlibc's build system can
// produce a crt1.o without error.  Linking it alongside ostd's entry is
// harmless because this object exports no symbols.
//
// Do NOT add a _start or main here. Duplicate entry symbols will cause a
// linker error or silently override ostd's startup sequence.
