.section .text
.global __switch

# fn __switch(old: *mut Context, new: *const Context);
# a0: old context ptr
# a1: new context ptr

__switch:
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
    sd s0, 14*8(a0) # sepc (placeholder, needs manual read) -> wait, switch isn't an interrupt.
                    # For thread switching, we need to save the current execution point (ra) which is already saved.
                    # BUT, "sepc" and "sstatus" in Context implies we want to restore them?
                    # Actually, for kernel threads `switch`, `ra` IS the return address.
                    # `sepc` is usually for returning from a TRAP.
                    # User complaint: "4 thanh ghi cuối (mepc, mstatus, gp, tp) mày định nghĩa cho vui à?"
                    # So I MUST save them.
                    # For `switch`, `sepc` isn't relevant? Unless we are switching to a task that was pre-empted?
                    # No, context switch happens in kernel mode.
                    # If I am voluntarily switching, `ra` is where I return.
                    # The user seems to imply that `Context` holds the state.
                    # Standard `__switch` in xv6/others often only saves Callee-saved registers (s0-s11, ra, sp).
                    # But the user explicitly DEMANDS syncing the struct (18 fields) with asm.
                    # "mepc, mstatus, gp, tp... 4 thanh ghi cuối... dữ liệu sẽ bị rác... mày định giết Task của tao à?"
                    # So I MUST save them.

    # We need to read CSRs to save them if they are part of the context?
    # Usually `sstatus` needs to be saved to preserve interrupt enable state.
    # `sepc`: usage depends on if this context is used for trap return. But `switch` is usually cooperative or called from trap handler (which saves trap frame).
    # If `switch` is called, `sepc` might not be active.
    # HOWEVER, to satisfy the user, I will save `gp` and `tp`.
    # `sstatus`: YES.
    # `sepc`: Maybe just save pseudo value or 0? Or `ra`?
    # Actually, if we use `Context` for initial task entry, `sepc` is the entry point.
    # So when *restoring*, `ret` might not be enough if we want to jump to `sepc`.
    # BUT `switch` ends with `ret`. `ret` uses `ra`.
    # So `sepc` in Context is likely for the *first* status when creating the thread (where we set `ra = forkret` or similar, and that jumps to `sepc`?).
    # If I just save/restore `gp`, `tp`, `sstatus`, that's good. `sepc` matches user demand.

    csrr t0, sepc
    sd t0, 14*8(a0)
    csrr t0, sstatus
    sd t0, 15*8(a0)
    sd gp, 16*8(a0)
    sd tp, 17*8(a0)
    # sscratch is intentionally NOT saved/restored (bug #7 fix): the nested-safe
    # protocol keeps sscratch == 0 for the entire time a hart is in S-mode, and
    # context switches only ever happen in S-mode. The trap-exit path loads the
    # task's kernel-stack top into sscratch right before `sret`. Restoring a
    # stale per-task value here re-armed the nested-trap stack-spray.
    # Context field 18 (sscratch) stays in the struct for ABI layout but is unused.

    # Load new context
    ld ra,  0*8(a1)
    ld sp,  1*8(a1)
    ld s0,  2*8(a1)
    ld s1,  3*8(a1)
    ld s2,  4*8(a1)
    ld s3,  5*8(a1)
    ld s4,  6*8(a1)
    ld s5,  7*8(a1)
    ld s6,  8*8(a1)
    ld s7,  9*8(a1)
    ld s8, 10*8(a1)
    ld s9, 11*8(a1)
    ld s10,12*8(a1)
    ld s11,13*8(a1)
    ld t0, 14*8(a1)
    csrw sepc, t0
    ld t0, 15*8(a1)
    csrw sstatus, t0
    ld gp, 16*8(a1)
    ld tp, 17*8(a1)
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
