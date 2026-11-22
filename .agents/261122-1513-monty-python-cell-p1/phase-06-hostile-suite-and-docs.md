---
phase: 6
title: "Hostile suite, CI lane, guide"
status: pending
priority: P1
effort: "1.5d"
dependencies: [4, 5]
tier: thinking
checkpoint: true
---

# Phase 6: Hostile suite, CI lane, guide

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

This phase adversarially tests the python cell as untrusted code would and puts it in CI. Every case must end in a contained,
reported outcome: a Python exception, a cell exit with its domain released, or a refusal. None may end in a kernel fault, a leak, or an effect
outside the sandbox root.

## Requirements

- Functional: a hostile corpus `tests/python-hostile/*.py`, each case with an expected outcome file:
  - memory bomb (soft → `MemoryError`; bypass attempts → hard-limit end);
  - CPU loop (timeout);
  - deep recursion (`RecursionError`);
  - nesting at 200 / 10 000;
  - path escapes (`..`, absolute, symlink planted in the root, NUL, very long path);
  - write outside the root;
  - import from outside the root;
  - `os.environ` probe;
  - `cellos_net` under `/bin/python`;
  - `max_suspensions` flood;
  - huge `print`;
  - `eval`/`exec` of the above.
- Functional: the runner `scripts/x86/qemu-python-hostile.sh` runs every case in one boot. It asserts the expected outcome, one domain release
  per run, no `[fault] Cell` except where the case expects a contained fault, and no `KERNEL PANIC`. It also checks the VFS tree afterwards for
  unexpected writes.
- Functional: CI adds the build, smoke and hostile lanes to `rust-std-lane` (self-hosted/heavy). `lanes.toml` gets the rows.
- Functional: docs: `docs/guides/tier2-python.md` (usage, Python subset, limits, sandbox root, python vs python-net, what is
  not supported), linked from `docs/app-development-guide.md` (decision tree "untrusted scripting") and the changelogs.

## Architecture

The runner reuses `qemu-monty-smoke.sh`'s FIFO/marker pattern. Cases are copied into the image's VFS fixture tree, and the expected
outcomes live next to them.

## Assumptions

- **Claim:** the VFS tree can be inspected after the run from the shell (`ls`, `cat`) inside the same boot.
  **Confidence:** high (`app-sys-tools` is in the image).

## Related Files

- Create: `tests/python-hostile/`, `scripts/x86/qemu-python-hostile.sh`, `docs/guides/tier2-python.md`
- Modify: `.github/workflows/ci.yml`, `scripts/lanes.toml`, `docs/app-development-guide.md`, `docs/project-changelog.md`, `CHANGELOG.md`,
  `third_party/README.md`

## Implementation Steps

1. Write the corpus and expectations; run the cases locally against a host build of the runtime modules where possible (confine, limits).
2. Write the runner; iterate until every case meets its expectation in QEMU TCG.
3. Independent security review (reviewer agent) of `confine.rs`, `oscall.rs` and `net.rs` against the corpus. Fix the findings and re-run.
4. CI wiring and docs. Checkpoint: `lint`, `security`, `rust-std-lane`, `qemu-x86_64-boot`, `x86-c2c`, `boot-suite`.

## Success Criteria

- [ ] Hostile runner PASS: every case meets its expected outcome, one domain release per run, no unexpected writes.
- [ ] Independent security review: no unresolved blocking or major findings.
- [ ] Checkpoint ci-local jobs pass; guide published and linked.

## Security Considerations

This phase is the evidence that the cell is safe to run untrusted code. Every finding from the review is fixed or explicitly
accepted by the owner and recorded in the Deviation Log.

## Risk Notes

- Hard-limit and stack-overflow cases end the cell by design. The runner must tell "contained end" from a regression by marker,
  not by exit code alone.

## Deviation Log

None.
