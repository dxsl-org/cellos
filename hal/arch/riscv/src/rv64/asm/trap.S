# S-mode Trap Entry for RISC-V 64-bit (ViOS)
# This is the entry point for all traps (interrupts, exceptions, syscalls)
# Uses sscratch for kernel stack swap per RISC-V convention

.section .text
.global __trap_entry
.global __trap_entry_hart0
.global __trap_entry_hart1
.global vi_set_sscratch
.align 4

# Trap Frame Layout (matches ViTrapFrame struct):
# regs[32] = 32 * 8 = 256 bytes
# sstatus  = 8 bytes
# sepc     = 8 bytes
# stval    = 8 bytes
# scause   = 8 bytes
# Total    = 288 bytes

.equ TRAP_FRAME_SIZE, 288

.macro HART_TRAP_ENTRY pointer_offset
    # Swap user tp with the kernel stack pointer without clobbering either.
    # From S-mode sscratch is zero, so the second swap restores kernel tp.
    csrrw tp, sscratch, tp
    beqz tp, .Lfrom_supervisor\@

    addi tp, tp, -TRAP_FRAME_SIZE
    sd sp, 2*8(tp)
    csrr sp, sscratch
    sd sp, 4*8(tp)
    csrw sscratch, zero
    mv sp, tp

    .option push
    .option norelax
    lla tp, HART_LOCAL_TP_ADDRS
    .option pop
    ld tp, \pointer_offset(tp)
    j .continue_save

.Lfrom_supervisor\@:
    csrrw tp, sscratch, tp
    addi sp, sp, -TRAP_FRAME_SIZE
    j .continue_save
.endm

__trap_entry:
__trap_entry_hart0:
    HART_TRAP_ENTRY 0

__trap_entry_hart1:
    HART_TRAP_ENTRY 8

.continue_save:
    sd x1,   1*8(sp)    # ra
    sd x3,   3*8(sp)    # gp
    sd x5,   5*8(sp)    # t0 (save before using it to inspect SPP)
    # x2 and x4 are already saved by the U-mode prologue. S-mode overwrites
    # x2 below and keeps the kernel tp already resident in x4.
    csrr t0, sstatus
    andi t0, t0, 0x100
    beqz t0, .tp_saved
    sd x4, 4*8(sp)
.tp_saved:
    sd x6,   6*8(sp)    # t1
    sd x7,   7*8(sp)    # t2
    sd x8,   8*8(sp)    # s0/fp
    sd x9,   9*8(sp)    # s1
    sd x10, 10*8(sp)    # a0
    sd x11, 11*8(sp)    # a1
    sd x12, 12*8(sp)    # a2
    sd x13, 13*8(sp)    # a3
    sd x14, 14*8(sp)    # a4
    sd x15, 15*8(sp)    # a5
    sd x16, 16*8(sp)    # a6
    sd x17, 17*8(sp)    # a7
    sd x18, 18*8(sp)    # s2
    sd x19, 19*8(sp)    # s3
    sd x20, 20*8(sp)    # s4
    sd x21, 21*8(sp)    # s5
    sd x22, 22*8(sp)    # s6
    sd x23, 23*8(sp)    # s7
    sd x24, 24*8(sp)    # s8
    sd x25, 25*8(sp)    # s9
    sd x26, 26*8(sp)    # s10
    sd x27, 27*8(sp)    # s11
    sd x28, 28*8(sp)    # t3
    sd x29, 29*8(sp)    # t4
    sd x30, 30*8(sp)    # t5
    sd x31, 31*8(sp)    # t6
    
    # === STEP 4: Save CSRs ===
    csrr t0, sstatus
    sd t0, 32*8(sp)     # sstatus
    csrr t0, sepc
    sd t0, 33*8(sp)     # sepc
    csrr t0, stval
    sd t0, 34*8(sp)     # stval
    csrr t0, scause
    sd t0, 35*8(sp)     # scause
    
    # === STEP 5: Save interrupted sp ===
    # From U-mode the user sp was already parked into the x2 slot (and
    # sscratch zeroed) right after the early saves. From S-mode the
    # interrupted sp is simply frame base + frame size.
    csrr t0, sstatus
    andi t0, t0, 0x100          # Check SPP bit (bit 8)
    beqz t0, .call_handler      # from U-mode: x2 slot already correct

    # From S-mode: calculate original sp
    addi t0, sp, TRAP_FRAME_SIZE
    sd t0, 2*8(sp)

.call_handler:
    # === STEP 6: Call Rust trap handler ===
    mv a0, sp               # a0 = &ViTrapFrame (borrowed, not owned)
    call vi_trap_handler    # Rust function with Vi prefix
    
    # === STEP 7: Restore CSRs ===
    .global __trap_exit
__trap_exit:
    ld t0, 32*8(sp)
    csrw sstatus, t0
    ld t0, 33*8(sp)
    csrw sepc, t0
    
    # === STEP 8: Check if returning to U-mode ===
    # If SPP=0, restore sscratch with kernel stack for next trap
    csrr t0, sstatus
    andi t0, t0, 0x100
    bnez t0, .restore_smode
    
    # Returning to U-mode: set sscratch = kernel stack top
    addi t0, sp, TRAP_FRAME_SIZE
    csrw sscratch, t0
    
.restore_smode:
    # === STEP 9: Restore general purpose registers ===
    ld x1,   1*8(sp)    # ra
    # Skip x2 (sp) - restore last
    ld x3,   3*8(sp)    # gp
    ld x4,   4*8(sp)    # tp
    ld x5,   5*8(sp)    # t0
    ld x6,   6*8(sp)    # t1
    ld x7,   7*8(sp)    # t2
    ld x8,   8*8(sp)    # s0/fp
    ld x9,   9*8(sp)    # s1
    ld x10, 10*8(sp)    # a0
    ld x11, 11*8(sp)    # a1
    ld x12, 12*8(sp)    # a2
    ld x13, 13*8(sp)    # a3
    ld x14, 14*8(sp)    # a4
    ld x15, 15*8(sp)    # a5
    ld x16, 16*8(sp)    # a6
    ld x17, 17*8(sp)    # a7
    ld x18, 18*8(sp)    # s2
    ld x19, 19*8(sp)    # s3
    ld x20, 20*8(sp)    # s4
    ld x21, 21*8(sp)    # s5
    ld x22, 22*8(sp)    # s6
    ld x23, 23*8(sp)    # s7
    ld x24, 24*8(sp)    # s8
    ld x25, 25*8(sp)    # s9
    ld x26, 26*8(sp)    # s10
    ld x27, 27*8(sp)    # s11
    ld x28, 28*8(sp)    # t3
    ld x29, 29*8(sp)    # t4
    ld x30, 30*8(sp)    # t5
    ld x31, 31*8(sp)    # t6
    
    # === STEP 10: Restore sp and return ===
    ld sp,   2*8(sp)    # Restore user/kernel sp
    
    sret

# Helper function to set sscratch (called from Rust before entering userspace)
# fn vi_set_sscratch(kernel_stack_top: usize)
vi_set_sscratch:
    csrw sscratch, a0
    ret
