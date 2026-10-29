# Phase 01 source and verification baseline

Observed 2026-10-20 during the explicit `hc-cook --auto` Phase 01 preparation invocation. This is preparatory session evidence, not reproducible runtime qualification or ABI admission.

## Identity and ownership

- HEAD: `fabd6ab7e3f2d9ea90a931fe53b7d43e509888dd`.
- Entry status: 0 staged, 29 unstaged tracked files, 65 untracked paths ([saved raw output](reports/baseline-entry-status.txt); originally session-local `artifact://1`). Existing changes are user-owned; no resets or source edits were performed.
- Toolchain observed: rustc `1.97.0-nightly (f53b654a8 2026-04-30)`; cargo `1.97.0-nightly (eb9b60f1f 2026-04-24)`.
- Preparation producer: current coding-assistant session; accountable approval remains with the maintainer. A cross-surface source integrating owner and controlling-owner handoff are not established by this role.

## Executed baseline

`cargo test --offline --locked -p text-engine --target x86_64-unknown-linux-gnu`

Observed: **38 unit tests passed; 0 failed; 0 doctests**, exit success; no failing test names. [Saved raw output](reports/baseline-text-engine.txt) preserves the original session-local `artifact://5` result. This is the existing pure command-core host suite, not the full workspace, target compilation, image boot or new cellbox evidence. No full-workspace green claim is made. Offline/locked prohibits dependency acquisition and lockfile resolution changes.

No source implementation begins in this preparation. Before any later source slice, capture that slice’s full applicable baseline again. Kernel/Cell host-incompatible tests and QEMU scenarios require their actual admitted target/profile and fixtures; they were not silently treated as passing.

## Historical footprints, not approval

- `51d3e045f`: terminal session/grid/main, desktop launcher, workspace and desktop phase-09 ledger. Commit title claims streams; current consumer source must establish whether they are real.
- `bbd0f040a`: terminal main/session font/stride/focus changes.
- `0c33ed6be`: desktop main, qualification and ABI ledgers, current-focus and milestone reports.

Observed using `git log --grep` and `git show --stat`. Completion claims do not ratify new Cellbox contracts.

## Dirty tracked source/artifact identity

Hashes capture entry bytes, not reproducibility, signing validity or provenance of prebuilt images. Untracked path census is recorded above; untracked bytes have not been inventoried and must be captured before adopting them into a source slice.

| Path | Bytes | SHA-256 |
|---|---:|---|
| `.agents/failure-history.jsonl` | 5937 | `f2769c13b85552c060bd1f708eb8957c12aec9a1db431d099da46535d3ff30c7` |
| `CHANGELOG.md` | 276516 | `26a68c5edcdce3ce79c429c7f51b6f524574c57c95eca2cb30d12050090dae05` |
| `Cargo.lock` | 75817 | `d564f3fbbe15dd198c829337b9679aca638c9da02a990220b184e599ab66fe26` |
| `build/x86-iso-root/boot/kernel.elf` | 19602560 | `dbd862d6791c9f37e3ae3f066639975dc4c8f9e938212a99d0b95e97feb67731` |
| `cells/apps/chrome/src/main.rs` | 22701 | `e940e8f2210069ed8f6efc98fc811db3c115d834f7ab4160aedc3694bae43518` |
| `cells/apps/desktop/src/main.rs` | 15372 | `de787d62f6196772a3acb04d1f4183ea0f6e6ae4aee60bc4efb7d3ac33c02fcf` |
| `cells/apps/files/src/main.rs` | 23366 | `fcb5224c8bf79aecd29f03a88d0f30674aab88b7328f1d2eab3f4d19e7e78408` |
| `cells/apps/ocel/src/main.rs` | 82411 | `84d1a011158d7811e450379a8b017ac1ad14da9328a3e15b25e74460b648f203` |
| `cells/apps/terminal/src/main.rs` | 7857 | `79c9563df9c5ac8b8e98a646d1827b95a608be3b7f1cb80fc854dda1136db1a7` |
| `cells/services/input/src/dispatcher.rs` | 9191 | `75c5433b9065d2ebeab31f4e013a0af6f8abfb213de52fc99dffbe30fc661bfd` |
| `cells/services/input/src/main.rs` | 23793 | `3f6d44243f5caaf4d023f3b01e604666d2477889e4e6888d8e6eedb440e7016d` |
| `cells/services/net/src/dns.rs` | 5060 | `a752a1fd4841af69fe61d16575abde7788a04cf1cd9cd77612c82e04de299c8f` |
| `cells/services/net/src/dns/tests.rs` | 2151 | `1c97f4f8c2fc76cb86c59e927c62dde351b69552c3b08ef7c345463514470525` |
| `docs/decisions/0018-cell-native-portability-and-runtime-profiles.md` | 13430 | `5ef5f350d28ca2684d1bd0219ca8dacced3c5f48b133294539af0703b1320bcf` |
| `docs/decisions/0022-intel-x86-64-c2c-only-direction.md` | 10555 | `5b752029ba83e8a7e3e81d2b15162d7e7019a55cf95ad5d885e54ab53d80c9e9` |
| `docs/decisions/0024-cell-kernel-authority-contract.md` | 16421 | `275b068c1f7672a7c6381d3cb80abb7118dd10e983ad3a23941c4790554c32dc` |
| `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md` | 18451 | `36f6a2ef3d84f9c25ebe1951878d61cf4f1a55c862895787b35ce87368a1346c` |
| `docs/specs/11-shell.md` | 7013 | `ceef71020e509f35c5d1b558539a9ed9166d54bf38af7fb54d5ecba28f0eee75` |
| `docs/system-architecture.md` | 39762 | `be4b85dee61f1a922122057922d027240c6b5eb29c4f88e66448d92b10d8bc80` |
| `hal/arch/x86/src/x86_64.rs` | 13607 | `9cff5497464acdba6f6fb279dda0ad83aa8c1ea88225f9ca0dde9d7794084a33` |
| `hal/arch/x86/src/x86_64/paging.rs` | 31288 | `d640c328ec97e8b4d4e65d24c06255e16a1b7e463cbe33ec1ede16f2678d7e9f` |
| `hal/arch/x86/src/x86_64/vmx.rs` | 7014 | `f4a0301c6ba4710c90c752b562c4b8d8878709e031fbd134c51e44fcc7b92b2b` |
| `hal/arch/x86/src/x86_64/world_switch.rs` | 9092 | `65e09cab92d57b307fca1ffa1a89ae3275c4bd94420304107818903ad1c1686b` |
| `kernel/src/embedded-x86_64/kernel_fs.img` | 16777216 | `699e1df1757104bde3d37a930fa3ad93d92ee337861164f8164ae62c69c4621d` |
| `kernel/src/hypervisor/svm_registry.rs` | 25700 | `e887e6acfa4e50dc049db9124866a9bee7f0938686725d31a44f89ccf323a44c` |
| `kernel/src/main.rs` | 80532 | `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64` |
| `kernel/src/task/drivers/ps2_keyboard.rs` | 21702 | `6d9d661cb81ef6dab2f354dce9024dc44fbd1d815d4ef735e887fa39f95b67be` |
| `scripts/package-desktop-x86.py` | 3717 | `0a02f6f8c5391589ef1eb31ac2c52bd25d1b066fa207b0ce1269446c1093707c` |
| `scripts/unsafe-allowlist.toml` | 32882 | `6b0e4779938a009cde586f2a125f289c506b206145e07a89363227912dd84cc0` |

## Footprint and runtime ceiling

The prebuilt dirty kernel ELF and embedded filesystem sizes above are measured file sizes only. No matched signed split-tool image/policy/build provenance has been established; executable count/aggregate size, resident memory, startup, streaming and redraw baselines remain blocked pending a selected reproducible build/profile and disposable fixture. Do not compare those existing artifacts with a future build as if feature-equivalent.

No QEMU launch, signing, image packaging, download, key generation, network server or remote command was executed. `run-desktop.sh` is excluded from unattended validation because its existing image-user termination/inbound-forward behavior is unsafe for this preparation.

## Before source admission

1. Resolve exact per-delta CP1 specifications/digests and retained owner handoffs.
2. Select signed x86 image/policy/profile and controlled fixtures, private writable image copies and unique QMP sockets; do not replace canonical images.
3. Record artifact hashes and command/features/data equivalence before footprint comparisons.
4. Exercise each phase on target; host tests cannot qualify a Cellos port.
5. Bind CP2 to exact implemented delta, caller census and runtime proof before final caller/image retirement.


## Continuation capture — 2026-10-20 (candidate preparation)

The original 76-line baseline above is retained verbatim as historical entry evidence. This continuation supersedes its statement that untracked bytes and split-tool counts were not inventoried; it does **not** supersede its runtime/signing ceiling. Snapshot capture completed at 2026-10-20T12:38:54Z (read-only collection during concurrent preparation). Existing user source/artifacts were not reset, rebuilt, signed, packaged, converted or mutated. Preparation documents may continue to change after these hashes; this is a time-bounded byte snapshot, not an atomic worktree freeze or a final implementation digest.

### Commands actually executed in this continuation

- `git rev-parse HEAD` → `fabd6ab7e3f2d9ea90a931fe53b7d43e509888dd`; `git status --porcelain=v1 -uall` → **0 staged, 34 unstaged tracked, 88 untracked files**. Unlike the original directory-collapsing census, `-uall` enumerates individual paths. The extra preparation/portfolio files are not attributed to the user or treated as source permission.
- `rustc --version` → `rustc 1.97.0-nightly (f53b654a8 2026-04-30)`; `cargo --version` → `cargo 1.97.0-nightly (eb9b60f1f 2026-04-24)`; all exits 0.
- Streaming SHA-256 and `stat` of all 122 dirty/untracked files below; **1,420,590,412 logical bytes** collectively. No secret/key content was printed. Hashes of mutable preparation evidence, including this document, describe pre-append bytes only; they are intentionally not self-referential.
- `python3 tools/inspect_fat.py kernel/src/embedded-x86_64/kernel_fs.img` → exit 0. The existing `image_files()` reader from `scripts/generate-boot-config.py:165-272` was imported with Python bytecode writing disabled; its `rb`/read-only `mmap` route returned the complete payload inventory below. It did not execute that module's configuration-writing CLI.
- `readelf -h PATH` and `readelf -SW PATH` for each of the 22 release files in the release table → all exits 0. ELF class/machine independently read from headers. Section presence is **not** cryptographic signature verification or a pipeline attestation.
- `qemu-img info --output=json PATH` for each of the nine selected image files below → all exits 0; no backing filename reported. File logical size, host allocated bytes, virtual disk size and format are recorded separately. No `qemu-img convert`, VM launch, network operation or signing occurred.
- No test/build/lint/formatter was run in this continuation. Original command-core execution evidence remains `reports/baseline-text-engine.txt:5-51`, not a rerun or target pass.

### Dirty tracked and untracked byte manifest

Full `-uall` census, including relevant source, scripts, source/ABI docs, plan evidence and image/screenshots. ` M` means tracked worktree modification; `??` means untracked. Untracked unrelated research/screenshots are preserved, not adopted into Cellbox. Files absent from this table can still be tracked-clean or ignored; the ignored release/image sample follows separately. No claim of a complete ignored build-directory census is made.

| Status | Path | Logical bytes | SHA-256 |
|---|---|---:|---|
| ` M` | `.agents/261020-1750-cellbox-cli/phase-01-baseline-contracts.md` | 7117 | `4a5b004087e72f94c5a34394a2ec94fcc62006d63bdfd6e16d1e2434308e9687` |
| ` M` | `.agents/261020-1750-cellbox-cli/plan.md` | 10561 | `7ba32dfd1dcb0653d685dbf5b99e8633a33f0ad11bc1def21345f8edd13968fc` |
| ` M` | `.agents/failure-history.jsonl` | 5937 | `f2769c13b85552c060bd1f708eb8957c12aec9a1db431d099da46535d3ff30c7` |
| ` M` | `.agents/plan-portfolio.md` | 40995 | `71695fb102f4906b10011ad4111d8395f04d166f80f369c84a0c063d075a1083` |
| ` M` | `.agents/workflow-artifacts.json` | 177 | `2ec34026ae2abdcf965f613047634e2938c642d8495949b231ae7e320ffe1565` |
| ` M` | `CHANGELOG.md` | 276516 | `26a68c5edcdce3ce79c429c7f51b6f524574c57c95eca2cb30d12050090dae05` |
| ` M` | `Cargo.lock` | 75817 | `d564f3fbbe15dd198c829337b9679aca638c9da02a990220b184e599ab66fe26` |
| ` M` | `build/x86-iso-root/boot/kernel.elf` | 19602560 | `dbd862d6791c9f37e3ae3f066639975dc4c8f9e938212a99d0b95e97feb67731` |
| ` M` | `cells/apps/chrome/src/main.rs` | 22701 | `e940e8f2210069ed8f6efc98fc811db3c115d834f7ab4160aedc3694bae43518` |
| ` M` | `cells/apps/desktop/src/main.rs` | 15372 | `de787d62f6196772a3acb04d1f4183ea0f6e6ae4aee60bc4efb7d3ac33c02fcf` |
| ` M` | `cells/apps/files/src/main.rs` | 23366 | `fcb5224c8bf79aecd29f03a88d0f30674aab88b7328f1d2eab3f4d19e7e78408` |
| ` M` | `cells/apps/ocel/src/main.rs` | 82411 | `84d1a011158d7811e450379a8b017ac1ad14da9328a3e15b25e74460b648f203` |
| ` M` | `cells/apps/terminal/src/main.rs` | 7857 | `79c9563df9c5ac8b8e98a646d1827b95a608be3b7f1cb80fc854dda1136db1a7` |
| ` M` | `cells/services/input/src/dispatcher.rs` | 9191 | `75c5433b9065d2ebeab31f4e013a0af6f8abfb213de52fc99dffbe30fc661bfd` |
| ` M` | `cells/services/input/src/main.rs` | 23793 | `3f6d44243f5caaf4d023f3b01e604666d2477889e4e6888d8e6eedb440e7016d` |
| ` M` | `cells/services/net/src/dns.rs` | 5060 | `a752a1fd4841af69fe61d16575abde7788a04cf1cd9cd77612c82e04de299c8f` |
| ` M` | `cells/services/net/src/dns/tests.rs` | 2151 | `1c97f4f8c2fc76cb86c59e927c62dde351b69552c3b08ef7c345463514470525` |
| ` M` | `docs/decisions/0018-cell-native-portability-and-runtime-profiles.md` | 13430 | `5ef5f350d28ca2684d1bd0219ca8dacced3c5f48b133294539af0703b1320bcf` |
| ` M` | `docs/decisions/0022-intel-x86-64-c2c-only-direction.md` | 10555 | `5b752029ba83e8a7e3e81d2b15162d7e7019a55cf95ad5d885e54ab53d80c9e9` |
| ` M` | `docs/decisions/0024-cell-kernel-authority-contract.md` | 16421 | `275b068c1f7672a7c6381d3cb80abb7118dd10e983ad3a23941c4790554c32dc` |
| ` M` | `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md` | 18451 | `36f6a2ef3d84f9c25ebe1951878d61cf4f1a55c862895787b35ce87368a1346c` |
| ` M` | `docs/roadmap/current-focus.md` | 78971 | `a99a84c7670e6ac5c1e0e9f57c878c4c45bf124e288062833868151454deb5e7` |
| ` M` | `docs/specs/11-shell.md` | 7013 | `ceef71020e509f35c5d1b558539a9ed9166d54bf38af7fb54d5ecba28f0eee75` |
| ` M` | `docs/system-architecture.md` | 39762 | `be4b85dee61f1a922122057922d027240c6b5eb29c4f88e66448d92b10d8bc80` |
| ` M` | `hal/arch/x86/src/x86_64.rs` | 13607 | `9cff5497464acdba6f6fb279dda0ad83aa8c1ea88225f9ca0dde9d7794084a33` |
| ` M` | `hal/arch/x86/src/x86_64/paging.rs` | 31288 | `d640c328ec97e8b4d4e65d24c06255e16a1b7e463cbe33ec1ede16f2678d7e9f` |
| ` M` | `hal/arch/x86/src/x86_64/vmx.rs` | 7014 | `f4a0301c6ba4710c90c752b562c4b8d8878709e031fbd134c51e44fcc7b92b2b` |
| ` M` | `hal/arch/x86/src/x86_64/world_switch.rs` | 9092 | `65e09cab92d57b307fca1ffa1a89ae3275c4bd94420304107818903ad1c1686b` |
| ` M` | `kernel/src/embedded-x86_64/kernel_fs.img` | 16777216 | `699e1df1757104bde3d37a930fa3ad93d92ee337861164f8164ae62c69c4621d` |
| ` M` | `kernel/src/hypervisor/svm_registry.rs` | 25700 | `e887e6acfa4e50dc049db9124866a9bee7f0938686725d31a44f89ccf323a44c` |
| ` M` | `kernel/src/main.rs` | 80532 | `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64` |
| ` M` | `kernel/src/task/drivers/ps2_keyboard.rs` | 21702 | `6d9d661cb81ef6dab2f354dce9024dc44fbd1d815d4ef735e887fa39f95b67be` |
| ` M` | `scripts/package-desktop-x86.py` | 3717 | `0a02f6f8c5391589ef1eb31ac2c52bd25d1b066fa207b0ce1269446c1093707c` |
| ` M` | `scripts/unsafe-allowlist.toml` | 32882 | `6b0e4779938a009cde586f2a125f289c506b206145e07a89363227912dd84cc0` |
| `??` | `.agents/261019-1534-vfs-storage-search-research/phase-01-analysis-research.md` | 8964 | `1942e831f5b61d6dd1de4a277bef8f6aa7ce466ddb8639acf57e5dcb0bfd1e1f` |
| `??` | `.agents/261019-1534-vfs-storage-search-research/phase-02-decision-review.md` | 5672 | `3a67c3d712a1d863864687dd33c2ba06b8c177735870a2fd356999d3c3b973e4` |
| `??` | `.agents/261019-1534-vfs-storage-search-research/plan.md` | 3565 | `8b8a620f1f8f0470b188b96744fa66a2b8d098893106f240b123fdb9d7c69d0c` |
| `??` | `.agents/261019-1534-vfs-storage-search-research/research-notes.md` | 10494 | `53f86460728e687ede730770639aa446f6902ea97c87b09192f07076e5930a83` |
| `??` | `.agents/261020-1750-cellbox-cli/baseline.md` | 7274 | `dc93224931e1d330fa39bf54deb0ad81502aca60c91e9427d0796dabbac31284` |
| `??` | `.agents/261020-1750-cellbox-cli/command-catalog.md` | 11563 | `838df2e17d8f1631f9524e7ea09d3f9d0ea3721028f3bf67a4eb89d50b7a26f4` |
| `??` | `.agents/261020-1750-cellbox-cli/contract-deltas.md` | 17026 | `cef7cd3ae57552fc8d9499dabeec9408466c657d010ced8cf68d5094f4dd1edb` |
| `??` | `.agents/261020-1750-cellbox-cli/dependency-review.md` | 14188 | `00d78f20e606401c86ddd4093d77d677fd3d477c07b815ef9425cb91163ab9b9` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/baseline-entry-status.txt` | 3251 | `67a6bcfa3978e309fdb93c2d05f8fb1cf79ece830f6812268a27f69899e41a32` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/baseline-text-engine.txt` | 2991 | `557eaf1030aef75474279df6b4c1927e02ae6e5ce6a7c4ef4f1f0d74b476d8af` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/final-smoke.md` | 1009 | `f2fe9237259b25470136f0489138d509567b439c22d91f0f1323d30acd53667b` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/harness/adversarial-validation.json` | 418 | `87b8539757b4bb63f5e7ceddf4b24bae891fd9698bc7b5e9214441935877a434` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/harness/context-snippets.json` | 898 | `100ea531210a349de3ce2d8f9beeb32e6901f3f054a376d67c9b9efa1790a099` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/harness/review-decision.json` | 708 | `2284549324346c25c12792b35cacde4b2cadf7d484fe4550fdf05be39a956d34` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/harness/risk-gate.json` | 243 | `b4b24e7ede49d9a26b1732b76608478bb53b331ffd3ad9a7f7bd0eb538eae4d4` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/harness/verification.json` | 801 | `294ab526c6c686c25ffe38f83844aaa7f467cd1e088c5ea13f4c18b3cb96edfb` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/preparation-review.md` | 10091 | `eae1ad0d9d005557d80b703463f1402c41a7b29f47b5d67605f9b286341034d8` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/preparation-verification.md` | 2277 | `e8e677529f40832ac76cd7d317efa1ae359d407ffc4eecf051f9d44ff606ff13` |
| `??` | `.agents/261020-1750-cellbox-cli/reports/ship-disposition.md` | 866 | `9c330290b1c1c46b27e4e3f885b572dabce794dbee52622c2a1f05798ccdb67d` |
| `??` | `.agents/logs/261019-1310-unified-desktop-phase01.md` | 3173 | `7cbb241be3427e49cdd0e92787afd203889954a1fd6a4243f5ba4fb3bc0d90fb` |
| `??` | `.agents/logs/261019-1310-unified-desktop-plan.md` | 1905 | `367695f68376b7adfe863ada2c047ec6ac57f1aa3718ece0ae7bbd996a003349` |
| `??` | `.agents/logs/261020-1922-cellbox-preparation.md` | 1323 | `c237cd0a15ecbc0ff6aacf8196116123b7e4d19b595047a141ec3e8ce165fca5` |
| `??` | `apps/chrome/chrome.ext4` | 367001600 | `5f7f197d0ac8e42759c5738dfecdfe36a3a9f360cee71c892353d4eb2714adcc` |
| `??` | `apps/chrome/chrome.lua` | 866 | `fb9fe802c918f15cb76b8da4cc72086d85f92f9bb84829325dab0120258a9a41` |
| `??` | `apps/chrome/chrome.toml` | 787 | `fb8211d0004ecb5ab7a97eb90199222f8878e0add3c17cfae670e4026796045e` |
| `??` | `build/chrome-app.ext4` | 367001600 | `5f7f197d0ac8e42759c5738dfecdfe36a3a9f360cee71c892353d4eb2714adcc` |
| `??` | `build/linux-base.ext4` | 536870912 | `b1b02e0bbba383221b64f2d65dbadeba1d4f20d42db75d9c232e4348ac778ea0` |
| `??` | `build/repeat-default-desktop.ppm` | 3072016 | `e79edcccbf0e73d606e74b0ecc287fa19c8af72b4b7aeb9a736dc5215d7fafcf` |
| `??` | `build/repeat-final-boot.ppm` | 3072016 | `e1824aff797acbe457fd704c91538b35acb51a4cf6c25aa9ac7e302a0da05618` |
| `??` | `build/repeat-final-deleted.ppm` | 3072016 | `f762bfb8d732e41fba00fa2a10dbc2f03774226a1a5f9b32bac6d0a415d53c6d` |
| `??` | `build/repeat-final-release.ppm` | 3072016 | `5678dff12a67aee0b0776d35fee87797631f6ed3c356f695908d204f090e6979` |
| `??` | `build/repeat-final-typed.ppm` | 3072016 | `4c6f86fd52456d6d311eea2173acab655e3f2d30bff49e04650b96d5fe437604` |
| `??` | `build/repeat-focus.ppm` | 3072016 | `4a4b39fb9642e20799084ab6d2751438ebe300ab3b9d9d4d386a06d0c3aa32b3` |
| `??` | `build/repeat-held-a.ppm` | 3072016 | `8b307b089fe14d5b9329c06834c5f8f92efe47cb229969d0179a96d22ecab470` |
| `??` | `build/repeat-held-backspace.ppm` | 3072016 | `de0398f35d1cfaf8173c74833f233db666b054920ee4029a2c3aec7418160fd6` |
| `??` | `build/repeat-initial.ppm` | 3072016 | `da7a532465e69d9fdd78fb195338e760b51e077f94d7cc8c292c94d00407937a` |
| `??` | `build/repeat-modifiers.ppm` | 3072016 | `4cad2f299154e2e242c75e6e9554be02c3a0583c032360492f2cffb6640ee24e` |
| `??` | `build/repeat-ocel-ready.ppm` | 3072016 | `3c2acd47263741d87093adcc25ac36b52849fa880f215567d8ea0703cc8abdb1` |
| `??` | `build/repeat-ocel.ppm` | 3072016 | `e52d3b379ce1f5973dedd215e68db2c894e19e61e509e2370de4e07dc3cd5719` |
| `??` | `build/repeat-released.ppm` | 3072016 | `0678da33831a90bdfb4fd83dc023e4029a3d3574c0779958d012993a4f867445` |
| `??` | `build/repeat-shortcut.ppm` | 3072016 | `4a1fdbdf16dfe1a3ecf2d9729a0f59f233787f26dd9bd388bb3e7e33b0d7d467` |
| `??` | `build/repeat-spotlight.ppm` | 3072016 | `e52fdefebb81edab01aae2ad7f86fa46ac5fd3f70aeaa46b3ccc966605c103f5` |
| `??` | `build/screen-after.png` | 96803 | `21d40db809bab4b59ea85fe6a770e552967a73056a370811cdaf6e1d7be2c226` |
| `??` | `build/screen-after.ppm` | 3072016 | `3d4cc6dbf0901eb06a86fa0d7c83a9ad95f6bd1c18be61e48a7877d4b18fbb22` |
| `??` | `build/screen-before.ppm` | 3072016 | `b626307dd19bfdc7acd0a371e1cb7420885bf4b47660b16c9b85152e09b97847` |
| `??` | `build/screen-chrome-run-final.png` | 125311 | `45d18e2909e17cb6ed4a32b9782049c303be7d508a98352264bea3647501d345` |
| `??` | `build/screen-chrome-run-final.ppm` | 3072016 | `43e7d052d037a1525ec512b8418e89f4b8ac747ac43a35bd60f4b558d3f5074e` |
| `??` | `build/screen-chrome-run.png` | 118726 | `7bcf532aaa5bc03173def5d4db3d57eee8d4a34fea49e667047ba21ce1b97505` |
| `??` | `build/screen-chrome-run.ppm` | 3072016 | `7e9b86eaa71cd95f7cd3f66ad3974270a31436afebcbadac63a3d34a1ba06b3d` |
| `??` | `build/screen-chrome-success.png` | 125117 | `837928dd95c63ba3d939abcf5dab8c71cdef5d64f0834ac8d74fe5f523d7937d` |
| `??` | `build/screen-chrome-success.ppm` | 3072016 | `001c0e8544d47e5dab810dfc7bdec3e7103eb9bd746bda0cc0614ffa618052fd` |
| `??` | `build/screen-chrome.png` | 96166 | `6127c7c7e3b0fd5a9742fefc57ad81ff2586a0c645d4317020100628e3d9221c` |
| `??` | `build/screen-chrome.ppm` | 3072016 | `b6c43d01551282acc80319d3e346d3926858f47cd5f86da007442f66d83ac1b1` |
| `??` | `build/screen-chrome2.png` | 119132 | `997b6dfc906e49262923b531073fb13b218191e6fec5961f72783ce535587def` |
| `??` | `build/screen-chrome2.ppm` | 3072016 | `2011cad34af11bb017290d28bc2c3bfbc7f75cc00954e5012821950541e8c641` |
| `??` | `build/screen-chrome3.png` | 118338 | `f194f112603c112360e1aa7cf344c1d85ea06a7b705eaa339de99617286263b0` |
| `??` | `build/screen-chrome3.ppm` | 3072016 | `c5eaca3a6455bd4d93da01d5cc39f626bf1091dc913921ec3ce29cd4531b0f94` |
| `??` | `build/screen-files-clean.png` | 104052 | `da4f59d5b7812c5713299e824e61184dfd0e0e3cd3fbbef3b4a6892e4273956c` |
| `??` | `build/screen-files-clean.ppm` | 3072016 | `24a8ecf848ad39f6d90dd688c611346c10ade5a425b773f47e0af15e8a940640` |
| `??` | `build/screen-haiku-desktop.ppm` | 3072016 | `7da3c1c4c3d3d711700219fe9caeac8372809390eb4b36235add039ad0ed70b3` |
| `??` | `build/screen-haiku-terminal.ppm` | 3072016 | `16267adf107e28abdea0d2b27a2811abe1891c362fe1e2bf60141757098d3e28` |
| `??` | `build/screen-live.png` | 46736 | `2a7e8f13c14b5293a6b16465e1d9ac4776e3352b6adc318f15003afd96e2fc16` |
| `??` | `build/screen-live.ppm` | 3072016 | `bab9d082408d9a1164d50f53139ec041a9c0b8c853e9410e31d63bc6afb6dbb4` |
| `??` | `build/screen-ocel-exit.png` | 117633 | `1951b4e3132cf064c95b4782737bd54a84c17af82efd6b077427c50030a1d13b` |
| `??` | `build/screen-ocel-exit.ppm` | 3072016 | `a15763fcc56a3665d736b16c4a2931b8dfc0cf30166c2c3c274410ad62d0cb10` |
| `??` | `build/screen-ocel-running.png` | 46736 | `2a7e8f13c14b5293a6b16465e1d9ac4776e3352b6adc318f15003afd96e2fc16` |
| `??` | `build/screen-ocel-running.ppm` | 3072016 | `bab9d082408d9a1164d50f53139ec041a9c0b8c853e9410e31d63bc6afb6dbb4` |
| `??` | `build/screen-ocel.png` | 46736 | `2a7e8f13c14b5293a6b16465e1d9ac4776e3352b6adc318f15003afd96e2fc16` |
| `??` | `build/screen-ocel.ppm` | 3072016 | `bab9d082408d9a1164d50f53139ec041a9c0b8c853e9410e31d63bc6afb6dbb4` |
| `??` | `build/screen-spotlight-ontop.ppm` | 3072016 | `f9b0a71a153c36b967a99377ff2849b2e4fde759012772f8ab94cf2562732628` |
| `??` | `build/screen-taskbar.png` | 124134 | `2af84048adfc4100e37805f930dcee7cddf506e9a6f912f28b455bdc24d8450d` |
| `??` | `build/screen-taskbar.ppm` | 3072016 | `df3f94fff5354bf88fd206c0445863cab1cc8d1fe7898fef367184e119a8be93` |
| `??` | `build/screen-tb-clicked.ppm` | 3072016 | `7798c5dfea30174096f6bd20650049c4c135afc34e4e5cbd02a0362f37caaeab` |
| `??` | `build/screen-terminal-haiku.png` | 100249 | `81d02ff14b73ccd5ace5de04b197edc43b21f8dbd945142462344b7ce827ebba` |
| `??` | `build/screen-terminal-haiku.ppm` | 3072016 | `57bf2befbac24b2efe45037072d61c94e18d8b6f5f866a307344d7e57f896d69` |
| `??` | `build/screen-terminal-vector.png` | 101978 | `22f14ce706b1819d2e8979b24a172b1681aab7b5d03185028202e94ea8a2cc84` |
| `??` | `build/screen-terminal-vector.ppm` | 3072016 | `c8bddb83c01fc39063c4edfa9c2f6a44306e60a8e913e1fdbc6288242dd9fe11` |
| `??` | `build/screen-test.png` | 78175 | `9ed3049e3bb4ced1f397fd4dca3fa5edb40ce0bb6db43871279e8dafe3a5fc46` |
| `??` | `build/screen-test.ppm` | 3072016 | `5cf53f8ce2c81f2e6b7115a3aef9d43ddad7601e15fba45198feb8b94ffa7005` |
| `??` | `cells/apps/chrome/build.rs` | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` |
| `??` | `cells/apps/vm-runner/Cargo.toml` | 364 | `0e05a3a1bdb1c285ba3faaf990b2a1ec58561b9b4cefcea521c778585f31ae85` |
| `??` | `cells/apps/vm-runner/build.rs` | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` |
| `??` | `cells/apps/vm-runner/src/main.rs` | 3750 | `dad12baf485fea0ed4c1e20a006c8d88a3323dfff277f36bed75ba25e57b22fd` |
| `??` | `cells/services/input/src/key_repeat.rs` | 4304 | `ef59592bea41b2b3c9c2dc82c7e3ef64caf9bae4d1e0ecb9eea42405769488f4` |
| `??` | `hal/arch/x86/src/x86_64/vmcs.rs` | 16266 | `5a2185cc15321f7be22a04151d68fe19075c490f32c0f9e34accb8bc50225384` |
| `??` | `hal/arch/x86/src/x86_64/vmx_vcpu.rs` | 16860 | `e907e88c2345481cbea07e753c12280ab6efc21bb67abd1eeca78a18aadc1169` |
| `??` | `scripts/build-chrome-app.sh` | 2468 | `bac87ec8d1b2964770011e12956dd9dcb036c09422836d8ffea7ad08854bf1f8` |
| `??` | `scripts/build-linux-base-image.sh` | 4298 | `6a84b2c6d2e9f9aa0d4196b0f00511b68d0f86fb0f5905be83e01b561481bcf0` |

### Existing selected image footprint (not a matched signed baseline)

Selection covers canonical desktop/serial names, observed repeat images, and existing test/embedded variants; unrelated hostile/domain/production-refusal images are not adopted into this workstream. Exact byte hashes do not establish which dirty source, features, policy signer or recipe produced an image. Equal lengths are not equal content.

| Image | File bytes | Host allocated bytes | Format | Virtual bytes | SHA-256 |
|---|---:|---:|---|---:|---|
| `build/cellos-x86_64.iso` | 23476224 | 23478272 | raw | 23476224 | `b9a08fcb9050b5dd5c89cafa059c5ba20820f8bec4957fdc38d93cbf9d585b76` |
| `build/vicell-x86.iso` | 15063040 | 15065088 | raw | 15063040 | `eb1cb8130476120a2f25ab63f6d1ef45036d497722005ffef93e62433abfef09` |
| `build/cellos-repeat-x86.iso` | 23476224 | 23478272 | raw | 23476224 | `15c90cd9783e6fb3bb4aa3b814e9f823a667689edaa97d4c53e3600c4bff34d7` |
| `build/cellos-x86_64.qcow2` | 16777216 | 16719872 | qcow2 | 23476224 | `1fd9ac0dee70f5edd6d042fa33f0b9b4f2fe32c5ee66b14ea21da8816f85ea72` |
| `build/cellos-x86_64-desktop.qcow2` | 16777216 | 16719872 | qcow2 | 23476224 | `0f662dfb6e6b8e0a446d5f945ad0579b4837a2d710d49b3a4d249270a414d676` |
| `build/cellos-repeat-x86.qcow2` | 16777216 | 16719872 | qcow2 | 23476224 | `581befef13312334168bd42e2e490a6ea3f77b9824ab67ca8f49d1a32d954861` |
| `build/cellos-x86_64-test.qcow2` | 12779520 | 12722176 | qcow2 | 19603456 | `9ff58ab150456e77d67beec136e47d8b147c5d5748b8003208708a7f5805ca55` |
| `build/cellos-x86_64-embedded.qcow2` | 11730944 | 11730944 | qcow2 | 19277824 | `4b964a1dc9d55b97964c0373030e8ffeb33a593222d73954f23d47200972c012` |
| `build/cellos-x86_64.vmdk` | 12320768 | 12275712 | vmdk | 19279872 | `03dc3a85d6429c1e866cdf8fe685109be56f31db5a1bce37cb174290833d5517` |

`build/cellos-x86_64.iso` and `build/cellos-repeat-x86.iso` have identical length but different hashes. The three 16,777,216-byte desktop/repeat QCOW2 files also differ. VMDK virtual bytes differ from both current canonical ISO and embedded QCOW2. A filename/mtime/size match must never substitute for profile equivalence. None is selected as an approved known-safe boot or rollback image.

### Embedded filesystem inventory and split-tool arithmetic

`kernel/src/embedded-x86_64/kernel_fs.img`: **16,777,216 file bytes**, SHA-256 `699e1df1757104bde3d37a930fa3ad93d92ee337861164f8164ae62c69c4621d` (also captured in both dirty manifests). Reader returned **40 files / 12,919,090 payload bytes**; **29 `/bin` ELF64 x86-64 files / 12,912,048 bytes**. These are payload file sizes, not resident memory, allocator use or verified launchability. `/bin/sshd` is present but is explicitly excluded as a trusted SSH fixture; its presence is not acceptance.

The measurable existing six-file split subset is `/bin/shell`, `/bin/ls`, `/bin/cat`, `/bin/echo`, `/bin/ps`, `/bin/kill`: **980,528 bytes, 6 ELF files** (shell 881,120 + five utility ELFs 99,408). This is the builder's subset, not all 17 sys/net manifest targets and not feature-equivalent to future Cellbox. No separate net-tool ELF appears in this ramdisk. The remaining sys-tools and all net-tools have no top-level release artifact in either inspected release directory; never count them as zero-sized or successfully packaged. Shell builtins remain inside the shell ELF and are not separate executable-count savings.

| Embedded path | Bytes | SHA-256 |
|---|---:|---|
| `/POLICY.BIN` | 1519 | `ef21d5086163ff08c82c34ea0f5927630b260309ed5cb2fa5ef85ee6e06dc910` |
| `/apps/chrome/chrome.lua` | 866 | `fb9fe802c918f15cb76b8da4cc72086d85f92f9bb84829325dab0120258a9a41` |
| `/apps/chrome/chrome.toml` | 787 | `fb8211d0004ecb5ab7a97eb90199222f8878e0add3c17cfae670e4026796045e` |
| `/bin/ahci` | 44720 | `fde08ac246ce56ecc078387ac8b3d292bd8e2923515a1dad0272fe4134afbabe` |
| `/bin/cat` | 20976 | `7d5b4199faabb7aa52419a0881ce191c5b29ef0a5baa80115fed201d7d9d9471` |
| `/bin/chrome` | 1147232 | `67cb3a9f1da7d040707637bd852008f4716255614f9588ce54c8647d8bbb9826` |
| `/bin/compositor` | 112064 | `2fe61691d9f6e0112a0940744ddb9e0c6d1f9a7f9e41ef6b0b337d348f65b8a5` |
| `/bin/config` | 42912 | `6ec6aff2029b8b814ffca2235d3f1451cec188ef6ee859c40cf46455b2f54551` |
| `/bin/desktop` | 1168336 | `4d2af9bdf0eda2b779bea9e63dc6fb0f0c467c3e329d25d46d9cf52a284b797f` |
| `/bin/e1000` | 29672 | `b46bd1c5d131869216e45ff73d5b7be92b4efe891556c4cd0c6a2faae267bbfb` |
| `/bin/echo` | 20952 | `b3f4f1174bf9450761095987667d75d5ec20e9d566ba5d34b6735eaa3fe2ba46` |
| `/bin/fb-console` | 588592 | `2df20ca6c9532df83aeb8c1bbf681ec81b4f3dcb9a45ae7b6e109dd60544bde4` |
| `/bin/files` | 1178056 | `b5fb7ab90c01ff78716275178b5b385e3e30fbde12ffa5c030aef7f32d1daa71` |
| `/bin/igb` | 39424 | `9fb90ebd3546431c7f46142dea6824213bc31fb77381261b9845ff0d60a6acdf` |
| `/bin/input` | 42288 | `13176cc66a80091335909bcd781434842969b6f2d0ab4cc10995c39e47daca75` |
| `/bin/kill` | 21096 | `3afff6107e5877f3988ab4849b4b37367898fa0d34bd6737352132b959a5588a` |
| `/bin/ls` | 20952 | `acf78124418194424b3db7cf3a1f698a34714005c428c0e6b9ff24e9002d16c6` |
| `/bin/net` | 806144 | `7edb7e4179b1769477dff76a5550f35932cee212b8d4f5192ce952d90950cbd9` |
| `/bin/nvme` | 36096 | `63f9997609d79f8b4a2178dd2a886c16649c1505296302081bf4ce093125149b` |
| `/bin/ocel` | 4326184 | `00a8916e279bd5c8e7b3e13f7027708893554e1c67c64f65c8bea1a1f90d3105` |
| `/bin/ocel-quickjs` | 919272 | `692841b87d684d872c794ae8a66850e269ddcfeecff5f6e065ca147926fe8c65` |
| `/bin/platform` | 38488 | `7fb6ee4f9db332d41dd108bc1d7e84c57788b264f7e7dcfbf2674e948afcf48b` |
| `/bin/ps` | 15432 | `74b5bc6e0826f6f84bda495bce33284a7addd1363b595dbd0004d5415be8d468` |
| `/bin/serial` | 35752 | `a67a01d86102b0ecb1ce45f12b3fa29015850d2431c4acea65857564477ba350` |
| `/bin/shell` | 881120 | `225fb29f45a6bf486357dac791dd4ea9cc76e9120f2edae047d2bbe2acbedf90` |
| `/bin/sshd` | 197048 | `502fc7312f93ba861cf2ff082f7c37304580219621891a2e08e39e181afb37e3` |
| `/bin/terminal` | 598888 | `e216c46c5634529a9f97ac5a09df0eb2edf5b328e25e34c1b9f3c39a8457eb02` |
| `/bin/tier2-exploit` | 28904 | `64ffc91b920ef75506ed6ba7f07839ff85569b3b87c46bfcf638dcd49f4600f2` |
| `/bin/tier2-smoke` | 34264 | `74da62ea56a05631ef4ea8dbaaaed6bb1453b85c75018446ba5b3296d3366a6f` |
| `/bin/vfs` | 425624 | `5046e588278049c5fba87a7542e28b52ad578c1150e48f700c9a1c762e1ebb31` |
| `/bin/vm-runner` | 38848 | `ad3b8ecd264685d0d3a53057c9f580b38e6c234bf16d544a8a87784bc6479b3e` |
| `/bin/xhci` | 52712 | `2a7eae9b4282e6b1b6b20459826604febd9659de93223426f6a5c8c38a69d89d` |
| `/data/ocel-js-demo.html` | 1263 | `4ca5b4d8d98f3b75847db3f77de19e999a8c3eb71377127cd7740486ab6c9ab2` |
| `/data/ocel-native-web.css` | 303 | `ea09ac56965b811b49d944c3b050a7d3a045e39fc3d78957280d767dde4d4d50` |
| `/data/ocel-native-web.html` | 402 | `442c8819d58cf157fbedd88f47536f6cf7400caedff055c2de3df7cd05938ba9` |
| `/data/ocel-native-web.js` | 388 | `f9619a12c01ca47ab390ce4992cbb988444df2cec414e26751ca5316196bafb5` |
| `/etc/cellos/autoload.toml` | 281 | `0f670b5f1e0bed68219eba41737434efa71f508115cb544113636d0fa13f8631` |
| `/etc/cellos/services.toml` | 703 | `3fb6cf84b09e21306cb6c7122b976fb9a5cf31c5f7e26c45ba619c4bd393a19c` |
| `/etc/cellos/system.toml` | 128 | `74ecea5cd57faacfa1e32d1b006870eb3a85dea5e0b4f9e95753a57ceb147293` |
| `/welcome.md` | 402 | `442c8819d58cf157fbedd88f47536f6cf7400caedff055c2de3df7cd05938ba9` |

### Existing release artifacts and target ambiguity

All top-level executable artifacts in the two inspected release directories are listed; `.d`, dependency/incremental/build directories and lock marker files are not executable payloads. `sig` records only a `__ViCell_sig` section found by `readelf -SW`; **yes does not mean valid or matched signed provenance**. The kernel is not a Cell and absence of that section is not a kernel signing failure. RISC-V artifacts must not be compared to x86 Cellbox as though they were the admitted target.

| Release path | Bytes | ELF machine | sig section | SHA-256 |
|---|---:|---|---|---|
| `target/x86_64-unknown-none/release/cellos-kernel` | 19602560 | ELF64; x86-64 | no | `dbd862d6791c9f37e3ae3f066639975dc4c8f9e938212a99d0b95e97feb67731` |
| `target/x86_64-unknown-none/release/app-shell` | 881120 | ELF64; x86-64 | yes | `225fb29f45a6bf486357dac791dd4ea9cc76e9120f2edae047d2bbe2acbedf90` |
| `target/x86_64-unknown-none/release/service-vfs` | 425624 | ELF64; x86-64 | yes | `5046e588278049c5fba87a7542e28b52ad578c1150e48f700c9a1c762e1ebb31` |
| `target/x86_64-unknown-none/release/service-net` | 806144 | ELF64; x86-64 | yes | `7edb7e4179b1769477dff76a5550f35932cee212b8d4f5192ce952d90950cbd9` |
| `target/x86_64-unknown-none/release/ocel` | 4326184 | ELF64; x86-64 | yes | `00a8916e279bd5c8e7b3e13f7027708893554e1c67c64f65c8bea1a1f90d3105` |
| `target/x86_64-unknown-none/release/chrome` | 1147232 | ELF64; x86-64 | yes | `67cb3a9f1da7d040707637bd852008f4716255614f9588ce54c8647d8bbb9826` |
| `target/x86_64-unknown-none/release/terminal` | 598888 | ELF64; x86-64 | yes | `e216c46c5634529a9f97ac5a09df0eb2edf5b328e25e34c1b9f3c39a8457eb02` |
| `target/x86_64-unknown-none/release/files` | 1178056 | ELF64; x86-64 | yes | `b5fb7ab90c01ff78716275178b5b385e3e30fbde12ffa5c030aef7f32d1daa71` |
| `target/x86_64-unknown-none/release/desktop` | 1168336 | ELF64; x86-64 | yes | `4d2af9bdf0eda2b779bea9e63dc6fb0f0c467c3e329d25d46d9cf52a284b797f` |
| `target/x86_64-unknown-none/release/service-input` | 42288 | ELF64; x86-64 | yes | `13176cc66a80091335909bcd781434842969b6f2d0ab4cc10995c39e47daca75` |
| `target/x86_64-unknown-none/release/service-compositor` | 112064 | ELF64; x86-64 | yes | `2fe61691d9f6e0112a0940744ddb9e0c6d1f9a7f9e41ef6b0b337d348f65b8a5` |
| `target/x86_64-unknown-none/release/ocel-quickjs` | 919272 | ELF64; x86-64 | yes | `692841b87d684d872c794ae8a66850e269ddcfeecff5f6e065ca147926fe8c65` |
| `target/x86_64-unknown-none/release/vm-runner` | 38848 | ELF64; x86-64 | yes | `ad3b8ecd264685d0d3a53057c9f580b38e6c234bf16d544a8a87784bc6479b3e` |
| `target/x86_64-unknown-none/release/app-init` | 393152 | ELF64; x86-64 | yes | `0c2f607bb29e090b31c7820290aafc1974c818c0610d216f00b5f88a10104104` |
| `target/x86_64-unknown-none/release/service-sshd` | 197048 | ELF64; x86-64 | yes | `502fc7312f93ba861cf2ff082f7c37304580219621891a2e08e39e181afb37e3` |
| `target/x86_64-unknown-none/release/fb-console` | 588592 | ELF64; x86-64 | yes | `2df20ca6c9532df83aeb8c1bbf681ec81b4f3dcb9a45ae7b6e109dd60544bde4` |
| `target/riscv64gc-unknown-none-elf/release/files` | 1191512 | ELF64; RISC-V | no | `2898633c80159874c85f6babb03326281374177a56d0ab4b2566d41676d385f9` |
| `target/riscv64gc-unknown-none-elf/release/desktop` | 1187848 | ELF64; RISC-V | no | `67012b6f19d1702b52ca4f12dcd8f38cda2bcb44e53cc146bc368c373062414e` |
| `target/riscv64gc-unknown-none-elf/release/ocel-pdf` | 3547592 | ELF64; RISC-V | no | `08dd548492d28548065e2dad2b745942dc98dccb0941ac46636bdbb360f84a37` |
| `target/riscv64gc-unknown-none-elf/release/ocel-quickjs` | 582160 | ELF64; RISC-V | no | `1fd8f65a8383eaa0b3977147d9023448bfd40ebb4426942c681e2ab02686f1f6` |
| `target/riscv64gc-unknown-none-elf/release/ocel` | 3778024 | ELF64; RISC-V | no | `99f94f2c64c714499d921c27f02a066344dad0973dd54e1f5987899ba220db08` |
| `target/riscv64gc-unknown-none-elf/release/service-net` | 545968 | ELF64; RISC-V | no | `869b5626498ed5b7b569757410f1705d1fc73e307223696f88ed924b92c01c16` |

The x86 release kernel and dirty `build/x86-iso-root/boot/kernel.elf` have the same measured hash/length. The release shell/VFS/net/desktop applications/sshd/VM-runner payload hashes match the corresponding ramdisk files where present; this proves byte equality only, not that a selected ISO embeds this ramdisk/kernel or that its policy/signature chain is admissible. `/POLICY.BIN` is measured at 1,519 bytes with hash above; signer, accepted key, policy validity and source-to-image chain were not verified. Init is separately embedded (`scripts/build-x86_64-cells.ps1:136-147,205-209`); the release `app-init` hash alone does not bind the kernel's embedded init.

### Representative behavior evidence and its ceiling

| Behavior | Actual retained evidence | Supported conclusion |
|---|---|---|
| Quoted/empty argv in pure text argument core | Original 38-test run, `reports/baseline-text-engine.txt:6-7` | Arg cursor exhaustion and legacy argv boundaries passed on host; does not prove external Cell spawn's 512-byte stash |
| Existing grep statuses/errors/bounds | Same run `:14-25,29` | Per-input count/no-match, quiet, exact/regex/invert, invalid/unknown flag and aggregate-budget cases passed; not ripgrep or VFS traversal |
| Existing sed/awk behavior | Same run `:8-18,26,30-38,41` | Parser/filter/divide-by-zero/replacement cases passed; not complete shell script execution |
| Existing top arithmetic/CLI parsing | Same run `:33-34,39-43` | Task-oriented sorting/format/count/delay cases passed; not Cell-incarnation CPU/memory evidence |
| Existing packed command footprint | Read-only FAT traversal above, exit 0 | Exact current shell/five-tool payload sizes/count and paths measurable despite unknown signing provenance |
| Serial/GUI/external pipes/TUI/SSH behavior and startup/heap/stack/redraw | **Not executed**; no VM/service start allowed here | Requires admitted disposable target fixtures below; not a blanket blocker on metadata/contract preparation |

Current implementation semantics are source observations, not runtime evidence: sequential captured shell pipelines and stash argv are in `cells/tools/shell/src/executor.rs:248-270,354-388,853-990`; local fabricated GUI commands are in `cells/apps/terminal/src/session.rs:39-142`. They must not be relabeled as observed live behavior. Existing desktop test only exercises serial help plus pointer rendering (`tests/integration/tests/desktop-x86.rs:63-187`), not GUI Terminal→shell attachment.

### Prepared verification and remaining gates

Concrete candidate targets/routes, safe exact commands, per-phase data/runtime evidence and cleanup are in [runtime-fixtures.md](runtime-fixtures.md). All command examples there are prepared instructions, not executed results. Missing matched source/features/Cell+policy+kernel/image provenance blocks signed before/after comparison and target qualification, **not** the measured baseline arithmetic or candidate fixture definition now completed here. Controlling-owner admission and per-delta CP1 remain pending; no CP2 request is made before implementation. No source/ABI, lock, physical, guest-VMX, remote or production gate is released.
