.section .text
.global __trap_entry32
.align 2

# ViTrapFrame32 layout (144 bytes):
#   regs[0..31]: x0..x31 at offsets  0, 4, 8, ..., 124  (4 bytes each)
#   sstatus:  offset 128
#   sepc:     offset 132
#   stval:    offset 136
#   scause:   offset 140
#
# Save strategy: t0/x5 cannot be saved until after the frame is allocated
# (we use t0 to compute the original sp value). Save t0's original value
# in sscratch first, restore it after the frame is set up.
#
# All traps in Phase-31 Nano arrive from S-mode (SATP=0, no U-mode ring).
# sscratch == 0 before every trap (set in init(); restored at end of handler).

__trap_entry32:
    # 1. Stash t0 in sscratch so we can use t0 freely.
    csrw sscratch, t0

    # 2. Allocate 144-byte trap frame.
    addi sp, sp, -144

    # 3. Save all general-purpose registers.
    sw   x0,   0(sp)          # x0  = always 0 (slot kept for array indexing)
    sw   x1,   4(sp)          # ra
    # x2/sp: save the pre-trap sp (= current sp + 144)
    addi t0, sp, 144
    sw   t0,   8(sp)          # original sp
    sw   x3,  12(sp)          # gp
    sw   x4,  16(sp)          # tp
    # x5/t0: recover original value from sscratch
    csrr t0, sscratch
    sw   t0,  20(sp)          # original t0/x5
    sw   x6,  24(sp)          # t1
    sw   x7,  28(sp)          # t2
    sw   x8,  32(sp)          # s0/fp
    sw   x9,  36(sp)          # s1
    sw  x10,  40(sp)          # a0
    sw  x11,  44(sp)          # a1
    sw  x12,  48(sp)          # a2
    sw  x13,  52(sp)          # a3
    sw  x14,  56(sp)          # a4
    sw  x15,  60(sp)          # a5
    sw  x16,  64(sp)          # a6
    sw  x17,  68(sp)          # a7 (syscall number)
    sw  x18,  72(sp)          # s2
    sw  x19,  76(sp)          # s3
    sw  x20,  80(sp)          # s4
    sw  x21,  84(sp)          # s5
    sw  x22,  88(sp)          # s6
    sw  x23,  92(sp)          # s7
    sw  x24,  96(sp)          # s8
    sw  x25, 100(sp)          # s9
    sw  x26, 104(sp)          # s10
    sw  x27, 108(sp)          # s11
    sw  x28, 112(sp)          # t3
    sw  x29, 116(sp)          # t4
    sw  x30, 120(sp)          # t5
    sw  x31, 124(sp)          # t6

    # 4. Save CSRs.
    csrr t0, sstatus
    sw   t0, 128(sp)
    csrr t0, sepc
    sw   t0, 132(sp)
    csrr t0, stval
    sw   t0, 136(sp)
    csrr t0, scause
    sw   t0, 140(sp)

    # 5. Clear sscratch (re-assert S-mode invariant for nested traps).
    csrw sscratch, zero

    # 6. Call Rust trap handler with frame pointer as argument.
    mv   a0, sp
    call vi_trap_handler32

    # 7. Restore CSRs (sepc / sstatus may have been modified by handler, e.g. sepc += 4 for syscall).
    # __trap_exit is the entry point for newly-spawned tasks: the kernel sets
    # context.ra = &__trap_exit so the first context switch "returns" here,
    # restoring the initial trap frame and doing sret into the task entry.
    .global __trap_exit
__trap_exit:
    lw   t0, 128(sp)
    csrw sstatus, t0
    lw   t0, 132(sp)
    csrw sepc, t0

    # 8. Restore general-purpose registers (x2/sp restored last).
    lw   x1,   4(sp)
    lw   x3,  12(sp)
    lw   x4,  16(sp)
    lw   x5,  20(sp)
    lw   x6,  24(sp)
    lw   x7,  28(sp)
    lw   x8,  32(sp)
    lw   x9,  36(sp)
    lw  x10,  40(sp)
    lw  x11,  44(sp)
    lw  x12,  48(sp)
    lw  x13,  52(sp)
    lw  x14,  56(sp)
    lw  x15,  60(sp)
    lw  x16,  64(sp)
    lw  x17,  68(sp)
    lw  x18,  72(sp)
    lw  x19,  76(sp)
    lw  x20,  80(sp)
    lw  x21,  84(sp)
    lw  x22,  88(sp)
    lw  x23,  92(sp)
    lw  x24,  96(sp)
    lw  x25, 100(sp)
    lw  x26, 104(sp)
    lw  x27, 108(sp)
    lw  x28, 112(sp)
    lw  x29, 116(sp)
    lw  x30, 120(sp)
    lw  x31, 124(sp)
    lw   sp,   8(sp)          # restore original sp last

    sret
