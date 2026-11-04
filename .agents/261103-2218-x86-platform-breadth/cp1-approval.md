# x86 platform breadth — ADR-0031, CP1 confirmation 1 and S1 lock handoff approval

Recorded 2026-11-04. The accountable maintainer/owner (the user) answered **"Duyệt tất cả"** ("approve all") at the phase 00 sign-off of the `/skill:hc-cook` run. The approved question:

> Approve all:
> - accept ADR-0031;
> - CP1 confirmation 1 per recommendations D-1…D-7;
> - the S1 lock handoff per §7;
> - confirm the withdrawal of journal v2.
>
> After that, the approval records are written with digests and Phase 01 starts.

Recorded values:

| ID | Decision |
|---|---|
| ADR | [ADR-0031](../../docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md) accepted; the ADR-0022 amendment of 2026-11-04 is in force |
| D-1 | `QueryPcieFunction = 434` returns one `ViPcieFunctionV1` (584 bytes) with inline capability bytes; no raw capability-read call |
| D-2 | `AllocPcieIrq = 435` return packing `first_irq \| count<<8 \| kind<<16`; no free syscall in v1 |
| D-3 | 434/435 share allowlist bit 50, gated by `PcieDriverCap` at dispatch |
| D-4 | `SystemPower = 436` gated by `SupervisorCap`, shares bit 49 |
| D-5 | 418/424 skip devices owned by another live task (wire unchanged) |
| D-6 | S1 lock handoff with the CP1 §7 file set |
| D-7 | 434/435/418-424 implemented in phase 04a; 436 in phase 06 behind its own handoff; CP2 per phase with exact ABI delta and evidence |
| J | Journal v2 / durable-charge review withdrawn: `cpu_accounting` is `#[cfg(cellos_g1_cpu)]` and in-memory; the G1 lane keeps HPET |

**Authorizes:**
- ADR-0031 direction and scope;
- Law-1 confirmation 1 for the 434/435/436 designs and the 418/424 semantics;
- the file-bounded exception for exactly the CP1 §7 paths, for phases 01-05 (slice S1) only.

**Does not authorize:**
- Law-1 confirmation 2 (required after phase 04a, and after phase 06 for 436);
- any file outside CP1 §7;
- implementing 436 or anything else in slices S2-S6;
- edits to the G1 private lane;
- hardware purchase, physical, remote or production claims;
- release of any CELL-CONTRACT-0024 whole-plan relation.

Per ADR-0013, the integrating session does not ratify anything itself. This record carries the owner's answer only.

**Source HEAD at binding:** `64ad34027bc093e0facf88f794a1f05c734b7018`. `git status --short` at binding shows no source file dirty, only these documents:
- `M` for `.agents/cell-contract-0024-locks.md`, `.agents/plan-portfolio.md`, `docs/abi-boundary-records.md`, `docs/decisions/0022-intel-x86-64-c2c-only-direction.md` and `docs/hardware-compatibility-list.md`;
- `??` for `.agents/261103-2218-x86-platform-breadth/` and `docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md`.

**Digest form:** SHA-256 of whole files including the final newline, with no normalization. No digest is stored inside the file it hashes.

## Bound documents

| Path | SHA-256 |
|---|---|
| `.agents/261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md` | `96ab40aa7cdb31641bcba68483f065b7055f592b03f7ac059817f58cba1b6237` |
| `docs/decisions/0031-x86-platform-breadth-kvm-vmware-intel.md` | `e911fced6ebc6c0c1f5570a892a15ff020167a4cfb2e0fcad4700c0d28be59a9` |
| `docs/decisions/0022-intel-x86-64-c2c-only-direction.md` | `916b63bb48a8f2b412383fa2a34c56dc05b6e1a790d42516b5b55bc4da1831bb` |
| `.agents/plan-portfolio.md` (post scheduling edit) | `b0fb2bba327bc672ce9e8110a7a7414fe997adfd278c9ff9e1e1e84fb8c099bf` |
| `.agents/cell-contract-0024-locks.md` (post scheduling edit) | `92c273ba4186433ac074a53d601c4ac640e0a8fdcf6dbf9673342a1951f95b0f` |
| `docs/abi-boundary-records.md` | `6e81db77a430cedb6db74645966493fd64e25642daa2791d0d8a19efd61cffc5` |
| `docs/hardware-compatibility-list.md` | `7a1f133d83cfcb4d6989eca163055c92537c993b5d87c059ae9d120a18e2cfaf` |
| `docs/code-standards.md` | `3be252a9ed9e9efb3075c48a0f1a2bf19bf426431306dd5b075bdfd302f9106b` |
