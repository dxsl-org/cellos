# Guest user-space check that XMM registers survive VM exits.
#
# Fills xmm0..xmm15 with distinct values, spins long enough for many host
# timer interrupts (each one a VM exit back to the hypervisor Cell), then
# compares every register. Prints X86_FPU_OK or X86_FPU_CORRUPT:<n> and exits.
# Build: as fpu-check.S -o fpu-check.o && ld -static fpu-check.o -o fpu-check

    .globl _start
    .text
_start:
    movabs $0x0101010101010101, %rbx
    movabs $0x5a5a00005a5a0000, %rax
    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    add %rbx, %rax
    movq %rax, %xmm\n
    .endr

    mov $2000000000, %rcx
1:  dec %rcx
    jnz 1b

    movabs $0x5a5a00005a5a0000, %rax
    xor %r8, %r8
    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    add %rbx, %rax
    movq %xmm\n, %rdx
    cmp %rax, %rdx
    je 2f
    mov $\n, %r8
    inc %r8
    jmp corrupt
2:
    .endr

    lea ok(%rip), %rsi
    mov $ok_len, %rdx
    xor %r12, %r12
    jmp report

corrupt:
    # Register number (1-based) as one or two decimal digits.
    lea bad_digits(%rip), %rdi
    mov %r8, %rax
    mov $10, %rcx
    xor %rdx, %rdx
    div %rcx
    add $'0', %al
    add $'0', %dl
    mov %al, (%rdi)
    mov %dl, 1(%rdi)
    lea bad(%rip), %rsi
    mov $bad_len, %rdx
    mov $1, %r12

report:
    mov $1, %rax          # write(1, msg, len)
    mov $1, %rdi
    syscall
    mov $60, %rax         # exit(status)
    mov %r12, %rdi
    syscall

    .data
ok:     .ascii "X86_FPU_OK\n"
        .set ok_len, . - ok
bad:    .ascii "X86_FPU_CORRUPT:"
bad_digits: .ascii "00\n"
        .set bad_len, . - bad
