# Driver Evidence Ledger Template

Use one ledger row per driver slice. Never infer `physical` from `compile`, `qemu`, or `synthetic`.

## Required fields

| Field | Allowed values | Rule |
|---|---|---|
| `driver_id` | exact `DriverId` or crate slug | One controller/path per row. |
| `stage` | `G1` \| `G2` \| `G3` | Record the intended product lane, not “all stages”. |
| `board_or_machine` | descriptor slug or QEMU machine | Examples: `raspberry-pi-3-model-b`, `qemu-q35-x86_64`. |
| `controller_path` | MMIO/PCI/device name | Example: `BCM2837 GPIO`, `Intel 82540EM`. |
| `compile` | `PASS` \| `FAIL` \| `BLOCKED` | Build only; no runtime claim. |
| `qemu` | `PASS` \| `FAIL` \| `BLOCKED` | Emulator/runtime evidence only. |
| `synthetic_or_fallback` | `PASS` \| `FAIL` \| `BLOCKED` \| `N/A` | Bit-bang, loopback, simulation, or fallback driver proof. |
| `physical_controller` | `PASS` \| `FAIL` \| `BLOCKED` \| `N/A` | Real controller on real hardware, even if whole board flow is incomplete. |
| `physical_board` | `PASS` \| `FAIL` \| `BLOCKED` \| `N/A` | End-to-end board proof; requires boot + controller + post-action witness. |
| `evidence_refs` | path:line list | Point to scripts, logs, commits, or report files. |
| `notes` | free text | Record blockers or scope limits; no marketing summary. |

## Promotion rule

| Outcome | Minimum proof |
|---|---|
| `prototype` | compile plus at most `synthetic_or_fallback=PASS` or partial `qemu=PASS`. |
| `present` | code exists with at least one non-physical proof, but promotion gate still unmet. |
| `promoted` | the stage’s required lane is proven in its own column set; if the stage is hardware-gated, `physical_board=PASS` is mandatory. |

## Lane-specific guardrails

| Stage | Guardrail |
|---|---|
| G1 | Bit-bang/sim proof does not promote a real BCM/DW controller. |
| G2 | q35/QEMU proof does not promote Pioneer or any physical x86/RV64 server. |
| G3 | G2 substrate proof does not promote an accelerator/NPU lane. |

## Example row

| driver_id | stage | board_or_machine | controller_path | compile | qemu | synthetic_or_fallback | physical_controller | physical_board | evidence_refs | notes |
|---|---|---|---|---|---|---|---|---|---|---|
| `SdhciArasan` | `G1` | `raspberry-pi-3-model-b` | `BCM2837 Arasan SDHCI` | `PASS` | `BLOCKED` | `N/A` | `PASS` | `PASS` | `boards/raspberry-pi/3-model-b/board.rs:6-12`; `docs/project-changelog.md:230-234`; `kernel/src/task/drivers/mmc/sdhci.rs:1-40` | Physical MMC lane is proven; QEMU remains non-authoritative for this board path. |

## Ledger hygiene

- Store raw PASS/FAIL/BLOCKED facts first; derive summary status afterward.
- If one board passes and another fails, use separate rows.
- If evidence comes from memory or a prior report, mark it as prior and do not upgrade current status without fresh verification.
- When a path is intentionally out of scope, use `BLOCKED` plus the governing spec/report reference.
