# Cleanup items 1-4 approval

Date: 2026-10-30. Approver: user ("làm hết theo thứ tự"; then "Duyệt theo khuyến nghị" i1/i2/i4, "Duyệt job CI + B0" i3).
Proposals (SHA-256 prefix): f1-gate 83d2cce6cf11cdc5;ceiling-diff 0171fcdfae809222;ci.yml fdbc8e1040a3db52;completion-queue c4593ae552617476;
- Item 1 f1-gate.md: forbid attrs x5, allowlist entries (rg/tests.rs; new service-input session_zeroize_tests.rs, approver dmin), runner phase09 edit. 
- Item 2 ceiling-diff.md: pin default baseline in scripts/cellbox-ceiling-diff.py (+4/-1).
- Item 3 ci.yml.md: new CI job kms-ssh-dev-posture; marker hand-off B0 (no edit of scripts/check-production-relay-image.py; Phase 02 owns it).
- Item 4 completion-queue.md: kernel/src/task/completion_selftest.rs only (3 hunks), baseline-repair slice; verification includes cargo check and ONE private x86_64 boot; no completion.rs edit, no host test addition.
- Each item: file closure exactly as its proposal; any other path stops. CHANGELOG by main agent. No push/commit by agents. Private CARGO_TARGET_DIR.
