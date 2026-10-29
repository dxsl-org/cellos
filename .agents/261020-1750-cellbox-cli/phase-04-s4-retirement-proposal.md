# Phase 04 slice S4 — final retirement (CP1 proposal)

Status: **CP1 APPROVED by the user on 2026-10-22 (“xoá applets, sửa exit status của cat, làm cả S4a + S4b”): D-S4-1 delete, D-S4-2 fix in S4, D-S4-3 S4a + S4b.** Parent: [Phase 04](phase-04-multicall-cutover.md) steps 5–6 and the [entry dossier](phase-04-entry-dossier.md). Evidence: read-only retirement census of 2026-10-22.

## 1. Census result (facts)
| Item | State | Disposition |
|---|---|---|
| Old shell crate `cells/tools/shell` / `app-shell` | Already gone; S1 was a `git mv`; workspace lists only `app-cellbox` (`Cargo.toml:114-124`). Boot, init, policy, launch profiles and Terminal all use `/bin/cellbox ["sh"]`. | Done; nothing to retire |
| GUI Terminal private matcher | Already removed by S3 (`cells/apps/terminal`). | Done |
| `applets` multicall feature | Non-default, and **unreachable**: `main()` always runs `run_shell` and nothing calls `dispatch_applet` (`cells/tools/cellbox/src/main.rs:87-121,183-235`). `applets/sys.rs::cmd_cat` and the text helpers duplicate live builtins (`executor.rs:1721-1805`). | **Retire** (plan step 6: no duplicate sources/wrappers) |
| Stale live recipes | `scripts/gen-disk-ci.sh:453` still asserts `LFN 'shell'` although the script packages `/bin/cellbox`; `scripts/make-hypervisor-fs.sh:112-115` probes `app-shell`/`service-shell`. | **Fix** (step 5) |
| Stale code doc | `state_transfer.rs` documents `hotswap shell /bin/shell-v2`. | **Fix** |
| False current docs | `docs/getting-started.md:149-150`, `docs/project-overview-pdr.md:213`, `docs/project-roadmap-legacy.md:868`, `docs/guides/tier1b-lua.md:56` present `/bin/shell` as current. | **Fix**; historical evidence and changelog entries stay as history |
| Stock sys-tools/net-tools ELFs (`ls cat echo ps kill env uname date free shutdown hotswap`, `ping curl nc wget httpd mqtt`) | Ledger `RetainedWorker`; still packaged by RV64/AArch64 recipes and used as stock externals by the runners. S2-N/S2-TW now bound their rights at the edge. | **Keep** (plan: "retain unrelated tools and standalone server modes") |
| Rollout containment (packager/signer/ISO guards, quarantined hashes) | `reports/rollout-containment.md` release conditions are unmet (Phases 05–10 incomplete). | **Keep**; S4 does not lift it |

## 2. Proposed slices
**S4a — source/recipe/doc retirement** (no canonical image bytes, no signing):
1. Delete `cells/tools/cellbox/src/{dispatch.rs,applets/**}`, the `applets` feature in `cells/tools/cellbox/Cargo.toml`, and their `cfg` hooks in `main.rs`. Also fix dead-code warnings that this exposes (only items made dead by the removal). Update `ledger.rs` and catalog wording that names the multicall form.
2. `scripts/gen-disk-ci.sh`: replace the `shell` FAT assertion with `cellbox`. `scripts/make-hypervisor-fs.sh`: replace the `shell`/`app-shell`/`service-shell` probe with `cellbox`/`app-cellbox`. No other recipe change.
3. Correct the `state_transfer.rs` doc and the four docs above to `/bin/cellbox ["sh"]`.
4. *(D-S4-2)* Fix `cat` of a missing file returning status 0 (`commands.rs` cmd_cat "No such file" branch returns `Ok`), and restore the status-based `sandbox-no-file` check in the Phase04 driver.

**S4b — CP2 dossiers** for the deltas implemented since the last CP2: [OOM local sink](phase-04-oom-local-sink-proposal.md), [S2-K](phase-04-s2-rights-proposal.md), [S2-N](phase-04-s2n-net-annex.md), [S2-T/W](phase-04-s2tw-annex.md). Each dossier records the exact diff, caller census, evidence and SHA-256 anchors, prepared from saved evidence (hashing only), in the format of the existing [S3 GUI CP2 dossier](phase-04-s3-gui-cp2-dossier.md). The user then approves each CP2 separately.

**Not in S4:** removing stock ELFs, editing canonical images or ISO, lifting quarantine, release signing, rollout.

## 3. Phase 04 criteria after S4 (honest forecast)
| Criterion | After S4 |
|---|---|
| 1 Catalog/callers migrated or retained | Closable: ledger plus census, with no shell remnants |
| 2 Shell state + byte pipelines on the new identity | Closable on private evidence (S1/P03/S3 runners) |
| 3 Ungranted rights denied; all affected CP2 | Closable once S4b's four CP2s are approved |
| 4 One suite executable; no fallback to obsolete artifact | Closable for source/recipes; **canonical image packaging stays blocked** by containment, so "packaged" holds only on private images |
| 5 Headless boot without viUI; desktop backend ops have CLI paths | **Not closable by S4**: the desktop-operation ↔ CLI coverage demonstration (step 7) has not been run |
| 6 GUI Terminal runs the real shell | Already checked (private fixtures) |

Phase 04 therefore stays `in_progress` after S4 unless criterion 5 gets its own slice.

## 4. Verification
- Host: `cargo test --offline -p app-cellbox --target x86_64-unknown-linux-gnu` (without `applets`, which no longer exists); `cargo check --release --target x86_64-unknown-none -p app-cellbox`; the ceiling diff is unchanged (G-S1a).
- Recipes: `bash -n` on both scripts. `gen-disk-ci.sh` FAT verification is exercised against a scratch image built with `tools/mkfat32.py` + `tools/inspect_fat.py` (the same method S2-TW used), **not** by producing a canonical RV64 image: the containment signer refuses canonical cellbox signing.
- Private image: S3 terminal runner (353; with D-S4-2 the restored status check), Phase03 (60), Phase02.
- Docs: link/path check of the edited files only.

## 5. Decisions requested (recommendations first)
- **D-S4-1 `applets`:** *delete* (recommended: unreachable duplicate code; a future multicall would need its own ceiling delta for `StateRestore` anyway), or keep disabled.
- **D-S4-2 `cat` missing-file status:** *fix in S4* (recommended; it is a migrated-behavior defect in a retained builtin, found by S2-TW), or leave it for a separate fix.
- **D-S4-3 Scope:** *S4a + S4b* (recommended), or S4a only.

## 6. Rollback
Revert the S4a files. S4b is documents only. No persisted state; containment is unchanged throughout.

## Results — S4a (2026-10-22)
- **Done:**
  - Deleted `cells/tools/cellbox/src/{dispatch.rs,applets/}` (untracked files, removed from disk) and the `applets` feature; updated the `main.rs` hooks/docs, the ledger tests and catalog §3.1 wording.
  - `gen-disk-ci.sh` FAT assertion now requires `cellbox`. `make-hypervisor-fs.sh` probes `cellbox`, with an explicit `$BIN_DIR/cellbox` case.
  - Corrected the `state_transfer.rs` doc and four docs (`getting-started`, `project-overview-pdr`, `project-roadmap-legacy`, `tier1b-lua`).
  - `cat` now continues past missing or denied operands and exits 1 (POSIX), and the driver's `sandbox-no-file` check is status-based again.
  - An implementer comment edit in `kernel/src/loader/launch_profile/profiles.rs` was reverted, so the pinned digest `3140c61b…` stays valid.
- **Verified:**
  - Host `app-cellbox` 89/89; `x86_64-unknown-none` check (plus `shell_test`); ceiling diff G-S1a PASS; `bash -n` on both scripts.
  - The FAT assertion passes on a scratch image containing `/bin/cellbox` and fails with `/bin/shell` or with no cellbox.
  - Private terminal runner exit 0, **353/353**, `sandbox-no-file` status-based; Phase03 60/60; Phase02 exit 0. Logs: `reports/s4-final-evidence/`.
- **Not exercised:** canonical RV64/AArch64 image production (blocked by containment); `cat` read-error-midway status (still 0, out of scope).
