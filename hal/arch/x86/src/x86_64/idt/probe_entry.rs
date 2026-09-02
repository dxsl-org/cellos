use core::arch::global_asm;

global_asm!(
    r#"
    .section .text.x86_idt_probe,"ax",@progbits
    .global x86_64_idt_test_dispatch_shim
    .type x86_64_idt_test_dispatch_shim,@function
x86_64_idt_test_dispatch_shim:
    .byte 0xf3,0x0f,0x1e,0xfa
    movq %rsp,X86_IDT_SHIM_RSP(%rip)
    jmp x86_64_idt_dispatch
    .size x86_64_idt_test_dispatch_shim,.-x86_64_idt_test_dispatch_shim

    .macro save_caller
    pushq %rbx
    pushq %rbp
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    subq $8,%rsp
    .endm

    .macro load_sentinels
    movabsq $0x1111111111111111,%rax
    movabsq $0x2222222222222222,%rbx
    movabsq $0x3333333333333333,%rcx
    movabsq $0x4444444444444444,%rdx
    movabsq $0x5555555555555555,%rbp
    movabsq $0x6666666666666666,%rsi
    movabsq $0x7777777777777777,%rdi
    movabsq $0x8888888888888888,%r8
    movabsq $0x9999999999999999,%r9
    movabsq $0xaaaaaaaaaaaaaaaa,%r10
    movabsq $0xbbbbbbbbbbbbbbbb,%r11
    movabsq $0xcccccccccccccccc,%r12
    movabsq $0xdddddddddddddddd,%r13
    movabsq $0xeeeeeeeeeeeeeeee,%r14
    movabsq $0xffffffffffffffff,%r15
    .endm

    .macro capture target
    movq %rax,\target+0(%rip)
    movq %rbx,\target+8(%rip)
    movq %rcx,\target+16(%rip)
    movq %rdx,\target+24(%rip)
    movq %rbp,\target+32(%rip)
    movq %rsi,\target+40(%rip)
    movq %rdi,\target+48(%rip)
    movq %r8,\target+56(%rip)
    movq %r9,\target+64(%rip)
    movq %r10,\target+72(%rip)
    movq %r11,\target+80(%rip)
    movq %r12,\target+88(%rip)
    movq %r13,\target+96(%rip)
    movq %r14,\target+104(%rip)
    movq %r15,\target+112(%rip)
    pushfq
    popq %rax
    movq %rax,\target+120(%rip)
    cld
    .endm

    .macro restore_caller
    addq $8,%rsp
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %rbp
    popq %rbx
    retq
    .endm

    .global x86_idt_probe_bp
    .type x86_idt_probe_bp,@function
x86_idt_probe_bp:
    .byte 0xf3,0x0f,0x1e,0xfa
    movq %rsp,X86_IDT_BP_CALLER_RSP(%rip)
    save_caller
    leaq .Lbp_return(%rip),%rdi
    callq x86_idt_probe_arm_bp
    addq $8,%rsp
    load_sentinels
    std
    int3
.Lbp_return:
    capture X86_IDT_BP_CAPTURE
    subq $8,%rsp
    restore_caller
    .size x86_idt_probe_bp,.-x86_idt_probe_bp

    .global x86_idt_probe_gp
    .type x86_idt_probe_gp,@function
x86_idt_probe_gp:
    .byte 0xf3,0x0f,0x1e,0xfa
    movq %rsp,X86_IDT_GP_CALLER_RSP(%rip)
    save_caller
    leaq .Lgp_fault(%rip),%rdi
    leaq .Lgp_recover(%rip),%rsi
    callq x86_idt_probe_arm_gp
    addq $8,%rsp
    load_sentinels
    std
.Lgp_fault:
    movw .Lbad_selector(%rip),%ds
.Lgp_recover:
    capture X86_IDT_GP_CAPTURE
    subq $8,%rsp
    restore_caller
    .size x86_idt_probe_gp,.-x86_idt_probe_gp

    .section .rodata.x86_idt_probe,"a",@progbits
    .balign 2
.Lbad_selector:
    .word 0xffff
"#,
    options(att_syntax)
);
