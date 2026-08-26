# 2026-08-26 — Contained development Silo Phase 2

## What happened
Replaced the public/general Silo prototype with a signed AArch64-QEMU-only `DEV_REFERENCE` provider behind KMS. Final evidence: 75 focused host tests, exact signed 12-cell image, virtualized QEMU PASS, and zero residual Critical/High/Medium findings.

## Decisions
- Remove `SiloHandle` and generic Init/Sign/ECDH/raw commands without compatibility shims; only typed KMS TLS `CertificateVerify` remains.
- Publish `service::SILO` only after admitted guest load, one-time entropy initialization, guest READY, and public-key validation through a test-hooks-only exact-path capability.
- Treat any guest/VMM/protocol fault or reset as permanent unavailability for that instance; never retry or fall back to an in-process key.
- Build the locked guest once, admit exact size/digest, load it after test-hook vCPU smoke injection, and require F1/F5-signed 12-cell FAT evidence.
- Keep Stage-2 evidence explicitly non-production; hardware custody and provenance remain blocked through Phases 6–8.

## Lessons
- Correct linker syntax exposed the real 32 KiB layout overflow; measured sections required a 64 KiB VM with the mailbox in the final page.
- Test-hook `create_vcpu` overwrites the entry page with x0=42/HVC smoke code; admitted guest bytes must load afterward.
- Repository-wide signing exposed a High grant lookup-to-lease TOCTOU; atomic table-lock validation and lease publication closed it without ABI changes.
- Diagnostics are a disclosure boundary: unknown guest HVC registers must be redacted, not logged for convenience.

## Next steps
- Obtain explicit approval before Phase 3 certificate activation and provisioning.
- Preserve `BLOCKED_PENDING_PHASE_6_7_8`; do not promote Silo or QEMU evidence to production qualification.
