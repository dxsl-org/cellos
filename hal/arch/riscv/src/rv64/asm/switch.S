.section .text
.global __switch

# fn __switch(old, new, outgoing_sstatus, root_ppn, asid);
# a0: old context ptr; a1: new context ptr; a2: outgoing sstatus
# a3: destination root PPN (0 = no root write); a4: destination ASID

__switch:
    # SIE is already masked. The Rust yield path captured a2 before acquiring
    # SCHEDULER or publishing current/selected/deferred state, closing the
    # interruptible selection-to-assembly window. Saving a2 instead of reading
    # sstatus here preserves the outgoing context's original SIE state.

    # Save old context
    sd ra,  0*8(a0)
    sd sp,  1*8(a0)
    sd s0,  2*8(a0)
    sd s1,  3*8(a0)
    sd s2,  4*8(a0)
    sd s3,  5*8(a0)
    sd s4,  6*8(a0)
    sd s5,  7*8(a0)
    sd s6,  8*8(a0)
    sd s7,  9*8(a0)
    sd s8, 10*8(a0)
    sd s9, 11*8(a0)
    sd s10,12*8(a0)
    sd s11,13*8(a0)
    # The task context also carries the trap-return CSRs and kernel ABI globals.
    # `ra` resumes scheduler code; `sepc` remains for the trap-return path.

    sd a2, 15*8(a0)
    csrr t0, sepc
    sd t0, 14*8(a0)
    sd gp, 16*8(a0)
    sd tp, 17*8(a0)
    # Floating-point state. Cells run rv64gc/lp64d code and may be preempted
    # with live values in f0-f31; the kernel never uses FP, so these are still
    # the outgoing task's registers. Without this, another FP-using Cell (the
    # compositor's blending) overwrote them mid-computation. FS=Dirty first:
    # with FS=Off (boot/idle sstatus) the accesses below would trap.
    # The kernel's module asm is assembled without the D extension; the FP
    # instructions below are legal at run time (rv64gc hart, FS set first).
    .option push
    .option arch, +d
    li t0, 0x6000
    csrs sstatus, t0
    fsd f0, 19*8(a0)
    fsd f1, 20*8(a0)
    fsd f2, 21*8(a0)
    fsd f3, 22*8(a0)
    fsd f4, 23*8(a0)
    fsd f5, 24*8(a0)
    fsd f6, 25*8(a0)
    fsd f7, 26*8(a0)
    fsd f8, 27*8(a0)
    fsd f9, 28*8(a0)
    fsd f10, 29*8(a0)
    fsd f11, 30*8(a0)
    fsd f12, 31*8(a0)
    fsd f13, 32*8(a0)
    fsd f14, 33*8(a0)
    fsd f15, 34*8(a0)
    fsd f16, 35*8(a0)
    fsd f17, 36*8(a0)
    fsd f18, 37*8(a0)
    fsd f19, 38*8(a0)
    fsd f20, 39*8(a0)
    fsd f21, 40*8(a0)
    fsd f22, 41*8(a0)
    fsd f23, 42*8(a0)
    fsd f24, 43*8(a0)
    fsd f25, 44*8(a0)
    fsd f26, 45*8(a0)
    fsd f27, 46*8(a0)
    fsd f28, 47*8(a0)
    fsd f29, 48*8(a0)
    fsd f30, 49*8(a0)
    fsd f31, 50*8(a0)
    frcsr t0
    sd t0, 51*8(a0)
    .option pop
    # Root activation belongs after the outgoing save and before incoming
    # registers/stack restore. A zero PPN preserves the SAS/same-domain path.
    beqz a3, 1f
    li t0, 8
    slli t0, t0, 60
    li t1, 0xffff
    and t2, a4, t1
    slli t2, t2, 44
    or t0, t0, t2
    li t1, 0xfffffffffff
    and t2, a3, t1
    or t0, t0, t2
    fence rw, rw
    csrw satp, t0
    sfence.vma zero, a4
1:
    # sscratch is intentionally NOT saved/restored (bug #7 fix): the nested-safe
    # protocol keeps sscratch == 0 for the entire time a hart is in S-mode, and
    # context switches only ever happen in S-mode. The trap-exit path loads the
    # task's kernel-stack top into sscratch right before `sret`. Restoring a
    # stale per-task value here re-armed the nested-trap stack-spray.
    # Context field 18 (sscratch) stays in the struct for ABI layout but is unused.

    # The incoming stack and kernel globals must be active before publishing
    # switch completion. Keep the incoming Context pointer in a callee-saved
    # register because the Rust callback may clobber a0/a1; `ra` is loaded after
    # the call because `call` clobbers it.
    mv s11, a1
    ld sp,  1*8(s11)
    ld gp, 16*8(s11)
    ld tp, 17*8(s11)
    call vi_context_switch_complete

    # Load the rest of the incoming context after the completion callback.
    ld ra,  0*8(s11)
    ld s0,  2*8(s11)
    ld s1,  3*8(s11)
    ld s2,  4*8(s11)
    ld s3,  5*8(s11)
    ld s4,  6*8(s11)
    ld s5,  7*8(s11)
    ld s6,  8*8(s11)
    ld s7,  9*8(s11)
    ld s8, 10*8(s11)
    ld s9, 11*8(s11)
    ld s10,12*8(s11)
    ld t0, 14*8(s11)
    csrw sepc, t0
    # SIE must remain masked until every callee-saved GPR, including the
    # incoming s11 value, has been restored. Restoring sstatus last makes a
    # pending timer/IPI observe either the complete old context or this complete
    # new one, never the transient Context base in s11.
    # Incoming FP state (see the save above); FS=Dirty again in case the
    # completion callback changed sstatus. `csrw sstatus` below sets the
    # incoming context's own FS.
    # The kernel's module asm is assembled without the D extension; the FP
    # instructions below are legal at run time (rv64gc hart, FS set first).
    .option push
    .option arch, +d
    li t0, 0x6000
    csrs sstatus, t0
    ld t0, 51*8(s11)
    fscsr t0
    fld f0, 19*8(s11)
    fld f1, 20*8(s11)
    fld f2, 21*8(s11)
    fld f3, 22*8(s11)
    fld f4, 23*8(s11)
    fld f5, 24*8(s11)
    fld f6, 25*8(s11)
    fld f7, 26*8(s11)
    fld f8, 27*8(s11)
    fld f9, 28*8(s11)
    fld f10, 29*8(s11)
    fld f11, 30*8(s11)
    fld f12, 31*8(s11)
    fld f13, 32*8(s11)
    fld f14, 33*8(s11)
    fld f15, 34*8(s11)
    fld f16, 35*8(s11)
    fld f17, 36*8(s11)
    fld f18, 37*8(s11)
    fld f19, 38*8(s11)
    fld f20, 39*8(s11)
    fld f21, 40*8(s11)
    fld f22, 41*8(s11)
    fld f23, 42*8(s11)
    fld f24, 43*8(s11)
    fld f25, 44*8(s11)
    fld f26, 45*8(s11)
    fld f27, 46*8(s11)
    fld f28, 47*8(s11)
    fld f29, 48*8(s11)
    fld f30, 49*8(s11)
    fld f31, 50*8(s11)
    .option pop
    ld t0, 15*8(s11)
    ld s11,13*8(s11)
    csrw sstatus, t0
    # (sscratch deliberately untouched — see the invariant note above.)

    # Always return to caller (kernel scheduler or previous task loop)
    # The caller will handle eventual return to user mode via trap.S
    ret

.global thread_trampoline
.align 4
thread_trampoline:
    # S-mode interrupt enable (SIE = bit 1)
    csrsi sstatus, 0x2
    mv a0, s0 # arg
    jr s1     # entry
