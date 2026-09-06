# Pinned QEMU-TCG x86 Summary

## Facts

- Qualified emulator boundary is QEMU-TCG 10.2.0; Ubuntu 8.2.2 and upstream through the recorded boundary triple-fault with the same guest (`docs/roadmap/open-risk-register.md:125-136`). This is not evidence for a CellOS workaround.
- `scripts/install-qemu-x86-ci.sh:6-83` already pins the official 10.2.0 archive, SHA-256, versioned prefix, `x86_64-softmmu`, `--disable-download`, atomic install, and literal version line.
- CI already passes that explicit binary to the strict 1 GiB boot smoke and retains output artifacts (`.github/workflows/ci.yml:843-885,913-935`).
- Smoke reports but does not enforce its selected version. E2e and hostile use regex predicates that also admit suffixed 10.2.0 builds (`scripts/qemu-x86-virtio-e2e.sh:36-39`; `scripts/qemu-tier3-hostile-runner-x86.sh:47-49`).
- The diagnostic `b56617bbcb` backport run lacked the normal strict liveness result; it is not qualifying evidence.

## Reconciled Decision

Retain the installer unchanged. Export one run-unique, proven initially absent `.../qemu-10.2.0` as `QEMU_X86_PREFIX` (the installer's `PREFIX`), then capture/hash one fail-fast group containing download/checksum/configure/build/install output, selected binary SHA-256, and literal first version line. Export that exact `QEMU_X86_BIN` to every qualified runner. Inline literal equality in smoke, e2e, and hostile; reject suffixes/8.2.2 before launch while preserving every fatal, liveness, VT-d, persistence, reset, scenario, and hostile oracle.

Commit the runner-only source first, then run the clean-prefix installer and every oracle from a clean checkout of that exact commit/tree; only afterward may normal verification/changelog bind revision/tree, commands/results, and evidence hashes. Missing source/digest/toolchain/image/oracle or desire for distro/10.2+/backport halts Phase 06. Evidence remains QEMU-TCG-only, not KVM, physical, production, generic distro, or security-maintenance qualification.
