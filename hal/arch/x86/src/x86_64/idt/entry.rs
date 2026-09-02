use core::arch::global_asm;

#[repr(C)]
pub(super) struct EntryFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
}

impl EntryFrame {
    #[inline]
    fn has_privilege_stack(&self) -> bool {
        self.cs & 3 != 0
    }

    pub(super) fn old_rsp(&self) -> Option<u64> {
        self.has_privilege_stack()
            .then(|| unsafe { core::ptr::read((self as *const Self).cast::<u64>().add(20)) })
    }

    #[allow(dead_code)]
    pub(super) fn old_ss(&self) -> Option<u64> {
        self.has_privilege_stack()
            .then(|| unsafe { core::ptr::read((self as *const Self).cast::<u64>().add(21)) })
    }

    pub(super) fn interrupted_rsp(&self) -> u64 {
        self.old_rsp()
            .unwrap_or((self as *const Self as usize + core::mem::size_of::<Self>()) as u64)
    }
}

const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<EntryFrame>() == 160);
    assert!(offset_of!(EntryFrame, r15) == 0);
    assert!(offset_of!(EntryFrame, r14) == 8);
    assert!(offset_of!(EntryFrame, r13) == 16);
    assert!(offset_of!(EntryFrame, r12) == 24);
    assert!(offset_of!(EntryFrame, r11) == 32);
    assert!(offset_of!(EntryFrame, r10) == 40);
    assert!(offset_of!(EntryFrame, r9) == 48);
    assert!(offset_of!(EntryFrame, r8) == 56);
    assert!(offset_of!(EntryFrame, rdi) == 64);
    assert!(offset_of!(EntryFrame, rsi) == 72);
    assert!(offset_of!(EntryFrame, rbp) == 80);
    assert!(offset_of!(EntryFrame, rdx) == 88);
    assert!(offset_of!(EntryFrame, rcx) == 96);
    assert!(offset_of!(EntryFrame, rbx) == 104);
    assert!(offset_of!(EntryFrame, rax) == 112);
    assert!(offset_of!(EntryFrame, vector) == 120);
    assert!(offset_of!(EntryFrame, error) == 128);
    assert!(offset_of!(EntryFrame, rip) == 136);
    assert!(offset_of!(EntryFrame, cs) == 144);
    assert!(offset_of!(EntryFrame, rflags) == 152);
};

#[cfg(not(feature = "x86-idt-cpl3-test"))]
use super::dispatch::x86_64_idt_dispatch as dispatch_target;

#[cfg(feature = "x86-idt-cpl3-test")]
unsafe extern "C" {
    fn x86_64_idt_test_dispatch_shim(frame: &mut EntryFrame);
}
#[cfg(feature = "x86-idt-cpl3-test")]
use x86_64_idt_test_dispatch_shim as dispatch_target;

global_asm!(
    include_str!(concat!(env!("OUT_DIR"), "/x86_idt_stubs.S")),
    options(att_syntax)
);

global_asm!(
    r#"
    .section .text.x86_idt_common,"ax",@progbits
    .global x86_64_idt_common
    .type x86_64_idt_common,@function
    .balign 16
x86_64_idt_common:
    .byte 0xf3,0x0f,0x1e,0xfa
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rbp
    pushq %rsi
    pushq %rdi
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    # Hardware does not switch GS or PKRU on an IDT privilege transition.
    # The saved GPRs make the WRPKRU clobbers harmless.
    testb $3,144(%rsp)
    jz .Lidt_kernel_entry
    swapgs
    cmpb $0,ViCell_pku_active(%rip)
    je .Lidt_kernel_entry
    xorl %eax,%eax
    xorl %ecx,%ecx
    xorl %edx,%edx
    wrpkru
.Lidt_kernel_entry:
    cld
    movq %rsp,%r12
    andq $-16,%rsp
    movq %r12,%rdi
    callq {dispatch}
    movq %r12,%rsp
    # yield_cpu resumes with IF set. Keep the complete user descent atomic.
    cli
    testb $3,144(%rsp)
    jz .Lidt_restore_gprs
    cmpb $0,ViCell_pku_active(%rip)
    je .Lidt_restore_gprs
    movl %gs:16,%eax
    xorl %ecx,%ecx
    xorl %edx,%edx
    wrpkru
.Lidt_restore_gprs:
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rdi
    popq %rsi
    popq %rbp
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax
    # After the pops: vector,error,rip,cs,rflags[,old_rsp,old_ss].
    # SWAPGS as late as possible and only for a CPL3 destination.
    testb $3,24(%rsp)
    jz .Lidt_iret
    swapgs
.Lidt_iret:
    addq $16,%rsp
    iretq
    .size x86_64_idt_common,.-x86_64_idt_common
"#,
    dispatch = sym dispatch_target,
    options(att_syntax)
);
