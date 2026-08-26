# 2026-08-26 — Production root kill gate Phase 6

## What happened
Executed the no-code product kill gate before Phase 4. Official-source research and active refutation found no reviewed product that jointly satisfies exact procurement/support, protected typed CSR/TLS reconstruction, immutable AP boot authorization, atomic rollback state, authenticated time, and qualified board/provisioning requirements.

## Decisions
- Accepted ADR-0006: select no production root product on current evidence; production Phases 7–8 are `BLOCKED_BY_ADR_0006`.
- Keep Phase 4 product-independent: it may proceed only after real protected persistence, authenticated time, and a distinct reviewed pending-key binding under frozen KMS opcodes 9–14.
- Keep Phase 5 `DEV_REFERENCE`; QEMU, FPGA, development bundles, generic TPM/secure-element signing, Pluton, and Caliptra do not receive production credit.
- Reopen only with one vendor-signed package binding exact MPN/revision, firmware/provisioning/support, content-enforcing protocol, AP/board, protected state/time, and per-device qualification; require fresh reviews and a superseding GO ADR.

## Lessons
- “Mass production” is not procurement evidence: lowRISC states the Nuvoton Earl Grey devices are not open-market parts, and the public family masks the exact part.
- Missing public capability evidence must remain `UNVERIFIED`, not be promoted to proof of absence.
- Product selection and software mTLS integration are separate gates; coupling Phase 4 to hardware selection hid the real persistence/time/pending-binding prerequisites.
- Historical generic `sign_prehash` guidance is dangerous even when current code is typed; mark obsolete reports explicitly non-normative.

## Next steps
- Resume Phase 4 only after its three software entry gates receive an explicit design and approval.
- Keep Phases 7–8 blocked until vendor evidence and a superseding GO ADR exist; perform no procurement, OTP, board, manufacturing, or hardware-adapter work before then.
