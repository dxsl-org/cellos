# 2026-08-17 — Cellos runtime and kernel name

## What happened

User-facing OS identity moved from ViCell to Cellos, and the Cargo package plus boot artifact moved from `vicell-kernel` to `cellos-kernel`. A real RPi3 TFTP boot verified the new prompt and architecture-aware system identity.

## Decisions

- Keep `Vi*`, `__ViCell_*`, syscall symbols, disk magic, and protocol constants stable; branding must not silently become an ABI migration.
- Report `Cellos cellos-kernel 0.2.1 <arch>` from shared no-std metadata; `target_arch` is compile-time truth.
- Treat Cellos as Unix-like in direction, with a future `target_os=cellos`, `target_family=unix`, `target_env=sas`; do not claim Linux or UNIX certification.

## Lessons

- Renaming a Cargo package creates a fresh OUT_DIR and exposed host `strip` deleting an embedded input on failure; strip only a temporary copy and keep a verified backup.
- Prompt changes require updating every integration wait gate in the same slice.

## Next steps

- Diagnose the RPi3 SDHCI CMD8 timeout with register/clock/power evidence before changing controller behavior.
- Keep the current bootstrap SD and TFTP workflow; no card rewrite is needed for kernel iterations.
