# D39 — Set the active-plan WIP limit

**Status:** approved/applied 2026-08-01. Documentation/portfolio only.

## Finding

Midori remains the convergence program, but its header is stale: phase 06, 09, and 11
are complete; phase 02 is code-complete but runtime-open; 04, 07, and 08 are partial.
P-TRUST is no longer an untouched blocker (`721e1f6f` landed), so supervisory migration
is technically unblocked but should stay queued to avoid reopening the same kernel/VFS
surfaces during convergence.

A literal "sole active plan" must allow emergency security/CI repairs and small
verification closures; otherwise the WIP rule would block the work needed to make the
active plan safe.

## Recommended ruling [FINAL]

**Approve A with a narrow exception policy.**

1. Midori is the sole active implementation program until runtime closure of 02 and
   completion of 04/07/08.
2. Allowed side work: P0 security fixes, broken-build/CI repair, and verification-only
   closure that does not open a new feature program.
3. Queue supervisory migration, package distribution, trust/identity remainder, and other
   product programs; do not label them active.
4. Correct Midori's header/phase summary and the stale P-TRUST dependency note.
