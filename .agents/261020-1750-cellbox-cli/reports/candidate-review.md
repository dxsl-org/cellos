# Cellbox candidate contract review and disposition report

**Review Date:** 2026-10-20  
**Status:** Formal review FAILED (four findings); candidate revisions applied, independent re-review unavailable after provider failure; implementation incomplete and rollout BLOCKED.
**Scope:** `spec.md`, `abi-session-launch.md`, `abi-vfs-telemetry.md`, `command-catalog.md`, `command-contracts.md`, `dependency-review.md`, `runtime-fixtures.md`

---

## 1. Summary of findings and dispositions

The formal candidate review failed with four findings. The parent assistant applied the candidate-document revisions below and wrote this disposition report; it is not a successful reviewer report. The attempted independent re-review failed at the provider and produced no approval. These are proposed contract changes, not proof of implementation or independent closure:

### Finding 1: Define an owner-issued confirmation lease operation
- **Issue:** D03 required unknown-host acceptance to use an active D02 trusted confirmation lease, but `SessionV1` defined no operation to issue or transfer such a lease to the VFS-hosted trust worker without undeclared foreground authority.
- **Candidate revision:** Added proposed typed operations in `abi-session-launch.md` §6:
  - `RegisterTrustChallenge` (VFS trust worker registers challenge with session; returns `ConfirmationRef`, not keyboard rights).
  - `BeginTrustConfirmation` (Current foreground caller initiates exclusive owner-run prompt).
  - `CollectTrustDecision` (VFS trust worker collects owner-attested terminal decision binding exact challenge, epoch, caller/worker principals).
  - `RevokeTrustConfirmation` (Worker or controller cancels pending confirmation).
- **Proposed enforcement:** The candidate design assigns `service-input` the trusted confirmation role, with a fixed owner-origin prompt bypassing untrusted child stdout/input. Implementation and enforcement are not established by this report.

### Finding 2: Split host-trust authority from KMS credential authority
- **Issue:** The caller/right matrix combined SSH private-key credentials and host-trust storage under a single KMS owner, conflicting with D03's placement of trust-store storage in the VFS service.
- **Candidate revision:** Updated `command-catalog.md`:
  - Split into **`A`** (KMS credential/private-key signing authority) and **`Vt`** (VFS-hosted trust-worker authority).
  - Both rights are independently required for `ssh`; neither confers the other.

### Finding 3: Freeze the `rg`, `edit`, and `ssh` command contracts
- **Issue:** New product commands had names and aggregate rights in `command-catalog.md`, but lacked argv grammar, options, modes, status rules, bounds, output schemas, and individual acceptance scenarios.
- **Candidate revision:** Added `command-contracts.md` as the candidate product annex:
  - `cellbox rg`: full options (`-e`, `-F`, `-i`, `-w`, `-x`, `-n`, `-q`, `-A/-B/-C`, `-g`, `--hidden`, `--no-ignore`, `-a`, `--encoding none`), traversal/ignore precedence, binary scan policy, `cellbox.search.line` schema, 0/1/2 statuses.
  - `cellbox edit`: terminal UI keys (arrows, Home/End, PgUp/PgDn, Backspace/Delete, Ctrl-Q/S/O/W/F/Z/Y/A/C/X/V, Alt-G/B), 1000-entry/16 MiB undo, syntax highlighting, conditional-save transaction outcomes, clean dirty-buffer retention.
  - `cellbox ssh`: exclusive password/key auth, PTY vs raw byte modes, explicit `--command` string boundary, 30s connection / 1h session timeouts, automatic rekeying at 1 GiB / 1 hour, untrusted host rejection.

### Finding 4: Define the `cellos-vt1` input encoding
- **Issue:** The output-control profile was fixed, but raw child input was left to an undefined “frontend profile”, allowing GUI, serial, and SSH frontends to produce incompatible byte streams.
- **Candidate revision:** Added §4.1 in `spec.md` with the logical key namespace (`Character`, `Enter`, `Backspace`, `Tab`, `Escape`, `Up/Down/Left/Right`, `Home/End`, `Insert/Delete`, `PageUp/PageDown`, `F1..F12`) and candidate byte mappings:
  - Enter = `0x0D`, Backspace = `0x7F`, Escape = `0x1B`.
  - CSI parameter calculation `m = 1 + Shift + 2*Alt + 4*Control`.
  - Bracketed paste framing (`\x1b[200~` ... `\x1b[201~`) with payload integrity.

---

## 2. Additional candidate-document revisions

1. **Snapshot scope mismatch:** The candidate revision changed the payload from `scope:PrincipalV1` to `grant:ControlGrantRef, request_id:u64`, with System-scoped `SnapshotTarget` only, to reflect the global RAM serializer inspected during preparation. This is not runtime authorization proof.
2. **Missing `uniq` row in catalog:** The candidate revision restored a builtin catalog row based on the source declaration inspected during preparation.
3. **Duplicate `--text` option in search annex:** The candidate revision removed the duplicate binary-eligibility alias to distinguish it from the renderer selector. This is not independent proof that the search contract is complete.

---

## 3. Chronology and retained gates

At candidate-preparation time, Phase 01 remained blocked: this parent-authored
disposition did not close the failed formal review or supply an independent
re-review. The saved document revisions and five historical digest observations
are preparation evidence only.

The maintainer subsequently explicitly approved continuing the candidate design.
That actual design consent is preserved; the earlier wording that implied no
design approval at any point is superseded. It does not establish numeric
opcode/syscall/IPC allocations, controlling-owner handoffs, runtime acceptance
or CP2.

Later implementation, host checks, target builds, signing and packaging occurred,
but the parent's later implementation review found the work incomplete and
passed no CBX row. A signed image is not a qualification result.
All ten phases remain incomplete/blocked, and all 29 CBX rows are pending or blocked.
Runtime integration, upstream dependency adaptation and affected authority/ABI/
ownership gates cannot be inferred from this report.

---

## 4. Corrected disposition

- **Formal review:** Failed with four findings; candidate revisions recorded, not independently verified as resolved. The attempted independent re-review failed at the provider.
- **Candidate package:** Not attested complete, self-consistent, immutable or finally approved by this parent-written report. Historical document hashes establish only the recorded byte identities.
- **Design consent:** The maintainer's actual approval to continue is retained without inferring allocation, handoff or runtime qualification.
- **Current acceptance:** No CBX passes and no CP2; implementation incomplete and artifact rollout **BLOCKED**. See the authoritative [rollout containment and withdrawal record](rollout-containment.md).
