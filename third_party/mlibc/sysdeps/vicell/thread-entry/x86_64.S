// SPDX-License-Identifier: MIT
// Spawn entry for mlibc threads. rdi = start block (generic/thread.cpp):
//   [0] entry  [1] user_arg  [2] Tcb*  [3] 0 = switch rsp to the block, 1 = stay
	.text
	.globl __vicell_start_thread
	.type __vicell_start_thread, @function
__vicell_start_thread:
	.cfi_startproc
	.cfi_undefined rip
	endbr64
	cmpq $0, 24(%rdi)
	jne 1f
	movq %rdi, %rsp
1:
	andq $-16, %rsp
	movq 16(%rdi), %rdx
	movq 8(%rdi), %rsi
	movq (%rdi), %rdi
	call __mlibc_enter_thread
	ud2
	.cfi_endproc
	.size __vicell_start_thread, . - __vicell_start_thread

	.section .note.GNU-stack, "", @progbits
