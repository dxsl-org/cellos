# GuC firmware pin (G0) — 2026-11-04

Source: linux-firmware `main`, fetched 2026-11-04 from git.kernel.org. Hashes are of the files as downloaded; the binaries are **not** committed to this repository. They ship unmodified with the `LICENSE.xe` notice (ADR-0033 §5), and nobody disassembles them.

| File | Use | Version (WHENCE) | Size | SHA-256 |
|---|---|---|---|---|
| `xe/ptl_guc_70.bin` | Panther Lake; Wildcat Lake `[inference: WCL IDs bind `ptl_desc` in `xe_pci.c:538`]` | GuC API 70.72.1 | 365,376 B | `ac42861c62b69f19923d9a69d971fb849921585a01924e14c0e8ac679d7cde81` |
| `xe/lnl_guc_70.bin` | Lunar Lake (only if that platform is chosen) | GuC API 70.72.1 | 348,992 B | `0583117763d2721b1b3a96c84d610229c18e644dce3a624b4e0ccc32952f5eaa` |
| `LICENSES/LICENSE.xe` | licence text | — | 2,041 B | `26b3c946f1f378ff25bb3bb7c365d108a2bc0bf08619e5e08bc0e4f9aaa6ee1e` |

`xe` (Linux master, `xe_uc_fw.c:119,121`) requires GuC major 70 with a minimum of 70.54.0 (PTL) and 70.53.0 (LNL). The pinned 70.72.1 satisfies both.

HuC and GSC are not pinned. Compute is expected to need only GuC (plan assumption A1). G2 confirms this from the `xe` init order and adds pins only if needed.
