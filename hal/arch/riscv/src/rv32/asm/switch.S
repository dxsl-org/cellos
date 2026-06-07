.section .text
.global __switch32

# fn __switch32(old: *mut Rv32Context, new: *const Rv32Context)
# a0 = old context ptr, a1 = new context ptr
#
# Rv32Context layout (76 bytes, 4 bytes each):
#   0:  ra      4:  sp      8:  s0     12:  s1
#  16:  s2     20:  s3     24:  s4     28:  s5
#  32:  s6     36:  s7     40:  s8     44:  s9
#  48: s10     52: s11     56: sepc    60: sstatus
#  64:  gp     68:  tp     72: sscratch

__switch32:
    # Save callee-saved registers
    sw  ra,   0(a0)
    sw  sp,   4(a0)
    sw  s0,   8(a0)
    sw  s1,  12(a0)
    sw  s2,  16(a0)
    sw  s3,  20(a0)
    sw  s4,  24(a0)
    sw  s5,  28(a0)
    sw  s6,  32(a0)
    sw  s7,  36(a0)
    sw  s8,  40(a0)
    sw  s9,  44(a0)
    sw  s10, 48(a0)
    sw  s11, 52(a0)
    # Save CSRs (needed for tasks pre-empted by timer and resumed via sret)
    csrr t0, sepc
    sw   t0, 56(a0)
    csrr t0, sstatus
    sw   t0, 60(a0)
    sw   gp, 64(a0)
    sw   tp, 68(a0)
    csrr t0, sscratch
    sw   t0, 72(a0)

    # Restore new context
    lw  ra,   0(a1)
    lw  sp,   4(a1)
    lw  s0,   8(a1)
    lw  s1,  12(a1)
    lw  s2,  16(a1)
    lw  s3,  20(a1)
    lw  s4,  24(a1)
    lw  s5,  28(a1)
    lw  s6,  32(a1)
    lw  s7,  36(a1)
    lw  s8,  40(a1)
    lw  s9,  44(a1)
    lw  s10, 48(a1)
    lw  s11, 52(a1)
    lw  t0,  56(a1)
    csrw sepc, t0
    lw  t0,  60(a1)
    csrw sstatus, t0
    lw  gp,  64(a1)
    lw  tp,  68(a1)
    lw  t0,  72(a1)
    csrw sscratch, t0
    ret

.global thread_trampoline
.align 2
thread_trampoline:
    # Enable S-mode interrupts (SIE = bit 1 of sstatus), same as rv64.
    csrsi sstatus, 0x2
    mv    a0, s0   # argument passed from scheduler
    jr    s1       # jump to task entry point
