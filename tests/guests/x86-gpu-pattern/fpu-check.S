# Guest user-space check that XMM and YMM registers survive VM exits.
#
# Fills xmm0..xmm15 with distinct values, spins long enough for many host
# timer interrupts (each one a VM exit back to the hypervisor), then compares
# every register: X86_FPU_OK or X86_FPU_CORRUPT:<n>. Then, where the guest has
# AVX (CPUID AVX and OSXSAVE, XCR0 enabling SSE and AVX), does the same with
# all 256 bits of ymm0..ymm15: X86_AVX_OK, X86_AVX_CORRUPT, or X86_AVX_ABSENT
# without AVX. Exit status 0 when nothing was corrupt.
# Build: as fpu-check.S -o fpu-check.o && ld -static fpu-check.o -o fpu-check

    .globl _start
    .text
_start:
    xor %r12, %r12               # exit status
    movabs $0x0101010101010101, %rbx
    movabs $0x5a5a00005a5a0000, %rax
    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    add %rbx, %rax
    movq %rax, %xmm\n
    .endr

    call spin

    movabs $0x5a5a00005a5a0000, %rax
    xor %r8, %r8
    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    add %rbx, %rax
    movq %xmm\n, %rdx
    cmp %rax, %rdx
    je 2f
    mov $\n, %r8
    inc %r8
    jmp xmm_corrupt
2:
    .endr

    lea ok(%rip), %rsi
    mov $ok_len, %rdx
    call print
    jmp avx

xmm_corrupt:
    # Register number (1-based) as two decimal digits.
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
    call print
    mov $1, %r12

avx:
    mov $1, %eax
    xor %ecx, %ecx
    cpuid
    and $0x18000000, %ecx        # OSXSAVE (27) and AVX (28)
    cmp $0x18000000, %ecx
    jne avx_absent
    xor %ecx, %ecx
    xgetbv
    and $6, %eax                 # SSE and AVX state enabled
    cmp $6, %eax
    jne avx_absent

    # pattern[i] = 7 * i + 3: every byte of every register differs.
    lea pattern(%rip), %rdi
    xor %ecx, %ecx
3:  lea 3(%rcx,%rcx,2), %eax
    lea (%rax,%rcx,4), %eax
    mov %al, (%rdi,%rcx)
    inc %ecx
    cmp $512, %ecx
    jne 3b

    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    vmovdqu pattern+\n*32(%rip), %ymm\n
    .endr

    call spin

    .irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15
    vmovdqu %ymm\n, readback+\n*32(%rip)
    .endr
    vzeroupper
    lea pattern(%rip), %rsi
    lea readback(%rip), %rdi
    mov $512, %rcx
    repe cmpsb
    jne avx_corrupt
    lea avx_ok(%rip), %rsi
    mov $avx_ok_len, %rdx
    call print
    jmp done

avx_corrupt:
    lea avx_bad(%rip), %rsi
    mov $avx_bad_len, %rdx
    call print
    mov $1, %r12
    jmp done

avx_absent:
    lea avx_none(%rip), %rsi
    mov $avx_none_len, %rdx
    call print

done:
    mov $60, %rax                # exit(status)
    mov %r12, %rdi
    syscall

# Spin long enough for many host timer interrupts. Clobbers rcx only.
spin:
    mov $2000000000, %rcx
1:  dec %rcx
    jnz 1b
    ret

# write(1, rsi, rdx). Clobbers rax, rdi, rcx, r11.
print:
    mov $1, %rax
    mov $1, %rdi
    syscall
    ret

    .data
ok:     .ascii "X86_FPU_OK\n"
        .set ok_len, . - ok
bad:    .ascii "X86_FPU_CORRUPT:"
bad_digits: .ascii "00\n"
        .set bad_len, . - bad
avx_ok: .ascii "X86_AVX_OK\n"
        .set avx_ok_len, . - avx_ok
avx_bad: .ascii "X86_AVX_CORRUPT\n"
        .set avx_bad_len, . - avx_bad
avx_none: .ascii "X86_AVX_ABSENT\n"
        .set avx_none_len, . - avx_none

    .bss
    .balign 32
pattern:  .skip 512
readback: .skip 512
