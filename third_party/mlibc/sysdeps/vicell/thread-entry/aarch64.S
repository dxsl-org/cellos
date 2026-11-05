// SPDX-License-Identifier: MIT
// Spawn entry for mlibc threads. x0 = start block (generic/thread.cpp):
//   [0] entry  [1] user_arg  [2] Tcb*  [3] 0 = switch sp to the block, 1 = stay
	.text
	.globl __vicell_start_thread
	.type __vicell_start_thread, %function
__vicell_start_thread:
	.cfi_startproc
	.cfi_undefined x30
	hint #34                // BTI c: Cells are built with +bti
	ldr x9, [x0, #24]
	cbnz x9, 1f
	mov sp, x0
1:
	mov x9, sp
	and x9, x9, #~15
	mov sp, x9
	ldr x2, [x0, #16]
	ldr x1, [x0, #8]
	ldr x0, [x0]
	bl __mlibc_enter_thread
	brk #0
	.cfi_endproc
	.size __vicell_start_thread, . - __vicell_start_thread

	.section .note.GNU-stack, "", %progbits
