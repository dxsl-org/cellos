// SPDX-License-Identifier: MIT
// Spawn entry for mlibc threads. a0 = start block (generic/thread.cpp):
//   [0] entry  [1] user_arg  [2] Tcb*  [3] 0 = switch sp to the block, 1 = stay
	.text
	.globl __vicell_start_thread
	.type __vicell_start_thread, @function
__vicell_start_thread:
	.cfi_startproc
	.cfi_undefined ra
	ld t0, 24(a0)
	bnez t0, 1f
	mv sp, a0
1:
	andi sp, sp, -16
	ld a2, 16(a0)
	ld a1, 8(a0)
	ld a0, 0(a0)
	call __mlibc_enter_thread
	unimp
	.cfi_endproc
	.size __vicell_start_thread, . - __vicell_start_thread

	.section .note.GNU-stack, "", @progbits
