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
