use core::arch::global_asm;

global_asm!(
    r#"
    .section .text.x86_idt_cpl3_user,"ax",@progbits
    .global x86_idt_cpl3_user_start
    .global x86_idt_cpl3_user_a
    .global x86_idt_cpl3_user_b
    .global x86_idt_cpl3_user_b_return
    .global x86_idt_cpl3_user_end
    .balign 16
x86_idt_cpl3_user_start:
x86_idt_cpl3_user_b:
    movq %rsp,%r14
    leaq x86_idt_cpl3_user_b_return(%rip),%r12
    movabsq $0x1122334455667788,%rdx
    movabsq $0x91,%rax
    syscall
x86_idt_cpl3_user_b_return:
    movq %rdx,%r13
    xorl %ecx,%ecx
    rdpkru
    movl %eax,%r15d
    movq %rsp,%r14
    movabsq $0xb110b110b110b110,%rax
    int $0x80
    ud2

    .balign 16
x86_idt_cpl3_user_a:
    xorl %ecx,%ecx
    rdpkru
    movl %eax,%r15d
    movq %rsp,%r14
    movabsq $0xa110a110a110a110,%rax
    int $0x80
    xorl %ecx,%ecx
    rdpkru
    movl %eax,%r13d
    movq $0x51a1,%rax
.Lx86_idt_a_spin:
    pause
    cmpq $0x1d7c0de,%rax
    jne .Lx86_idt_a_spin
    xorl %ecx,%ecx
    rdpkru
    movl %eax,%r15d
    movq %rsp,%r14
    movabsq $0xa440a440a440a440,%rax
    int $0x80
    ud2
x86_idt_cpl3_user_end:
"#,
    options(att_syntax)
);
