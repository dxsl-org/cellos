# Phase 01 C1-e/C2-e: Q Config per-key write right (CP1 proposal)

Status: **CP2 approved by the user 2026-10-26** (relayed by Main), including two implementation deviations: (1) `/bin/config` runs only with the `ai` feature, so the lane adds it to its private image, and on an image without `ai` `export` reports `config unavailable` with status 1; (2) an oversized `List` reply gets error `0xFF`, and the unused `Mutex` was removed. Evidence: P04 466/466 (`/tmp/cellbox-phase04-t8uH/evidence/verdict.json`, SHA-256 `90db1ce6465da47d0a6107329f0131750264bf3dfa3171c1a534549ce21f2680`; 6 `config-*` markers plus `config-probe-not-on-uart`), GUI 290, P03 67, P02 48, kernel lane pass with 7/7 VFS self-tests, P09 103; host kernel 284, cellbox 189, service-config 7, api 153; canonical hashes unchanged. This file's SHA-256 before this CP2 amendment: `72895bc86865672425d92f4a80404940413e3c9dcd90779a6e0f8b8844115b8f`. Earlier: **CP1 approved by the user 2026-10-25** for Q1 with every recommendation D-Q2..D-Q7 (relayed by Main): shell identifiers up to 64 characters except `PATH`/`OS`; `Get`/`List` ungated; `Delete` gated like `Set`; the right granted to the boot shell, GUI terminal shell and test-driver shell only; v1–v6 policies decode the right as granted; no store caps. Approved text SHA-256 `26d5b6a6808fb5989684a21b349a30ea6c6fc2730e30c35eab738b6d5e4783b4` (before this status line was amended). Before approval the status read "awaiting CP1". The user chose this path on 2026-10-25 ("export → CP1 proposal for per-key Config write right"). This document closes [dossier](phase-01-closure-dossier.md) C1-e/C2-e: the `export` denial scenario is lane-proven and CP2-approved.

Evidence is from the current dirty tree, read-only, on 2026-10-25. File digests are SHA-256 prefixes (16 hex):

| File | SHA-256 prefix |
|---|---|
| `cells/services/config/src/main.rs` | `ab75c4de09ae028c` |
| `libs/api/src/services/ipc.rs` | `ca8916570450f794` |
| `libs/api/src/abi/caller_identity.rs` | `8ca506d4cc609ecc` |
| `kernel/src/task/cap.rs` | `6201fc28dd351cdb` |
| `kernel/src/task/syscall.rs` | `c56d500f72ac1adc` |
| `kernel/src/task/tcb.rs` | `78202648db04c706` |
| `kernel/src/policy.rs` | `b34b9af9f8f88f0c` |
| `kernel/src/loader/boot_ceiling.rs` | `cebbb73af1bb1525` |
| `kernel/src/loader/launch_profile/profiles.rs` | `ded1ea57b6e042f1` |
| `scripts/sign-policy.py` | `c16751197e96bdbb` |
| `cells/tools/cellbox/src/shell.rs` | `4ce913dcb6f999d8` |
| `cells/tools/cellbox/src/config_client.rs` | `dc73c013ac6e3933` |
| `cells/tools/cellbox/src/ledger.rs` | `e0d6f08426524843` |

## 1. Facts

- **Service.** `service-config` receives with `sys_recv(0, &mut buf)` (any sender, no attestation; `cells/services/config/src/main.rs:62`). It applies `Set` (`:86-90`) and `Delete` (`:91-95`) unconditionally, and serves `Get`/`List` (`:65-85,96-112`). The store is an in-memory `BTreeMap` seeded with `PATH=/bin` and `OS=Cellos` (`:31-37`). It is carried across hot-swap by `ViStateTransfer` (`:127-208`). The manifest declares no block/network/spawn (`:14`).
- **Wire.** `ConfigRequest { Get(&str), Set{key,value}, Delete(&str), List }` and `ConfigResponse { Value, Keys, Ok, NotFound, Err(u8) }` (`libs/api/src/services/ipc.rs:920-945`). The only `Err` code in use is `0xFF` (decode/encode failure, `main.rs:78,113`). `IPC_BUF_SIZE = 4096` (`ipc.rs:23`).
- **Caller identity exists but is unused here.** `ostd::syscall::sys_recv_attested` makes the kernel write a 32-byte trailer `{magic, flags:u32, cell_id, generation, sender_tid}` at the buffer tail (`libs/ostd/src/syscall.rs:1423-1447`, `libs/api/src/abi/caller_identity.rs:37-149`). A missing or garbage trailer parses to `None`, which must be treated as deny (`caller_identity.rs:30-32,105-115`). Flags in use: `CALLER_FLAG_VFS_MUTATE = 1<<0` and `CALLER_FLAG_NET_* = 1<<1..1<<4`. Bits 5..31 are reserved and zero (`caller_identity.rs:44-65`). The kernel fills them in `attested_identity_of` from the sender Cell's root `CapSet` (`kernel/src/task/syscall.rs:2889-2910`).
- **The trailer carries no install path, launch row or executable identity**; only `cell_id`/`generation`. The syscall allowlist has no free bit (`phase-04-s2n-net-annex.md` §1, citing `libs/api/src/abi/syscall.rs:496-501,600-607`), and the v1 manifest is full. The established pattern for authority with no manifest bit is a kernel-only `CapSet` field. It is requested by install path (`with_path_caps`), bounded by `∩` the reviewed launch/boot ceiling and operator policy, and exported as a trailer flag (`kernel/src/task/cap.rs:214-261,425-517,568-608`). S2-N (network) and S2-T/S2-W (observe/VFS mutate) both used it, with signed-policy CapSet encodings v5/v6 (`kernel/src/policy.rs:35-74,336-357`; `scripts/sign-policy.py:49-51`).
- **Launch edges for `/bin/cellbox`:**
  - init boot row `"/bin/cellbox" => LEGACY` (`kernel/src/loader/boot_ceiling.rs:117-119`);
  - GUI `terminal-shell-edge` (`launch_profile/profiles.rs:148-157`);
  - private `phase04-driver-shell-edge` (`profiles.rs:159-178`, feature `phase04-driver-profile`).
  - The second install `/bin/cellbox-ssh` carries `NET_CLIENT` only (`profiles.rs:70-75`). Children of the shell get `reviewed_user_target_ceiling` rows (`launch_profile/targets.rs:29-142`).
- **Client.** `cellbox` `ConfigClient::set` sends `Set` and maps any reply other than `Ok` to `ViError::IO` (`cells/tools/cellbox/src/config_client.rs:87-108`). `export` takes the first `k=v` operand, calls `set`, **discards the result** and never sets `$?` (`cells/tools/cellbox/src/shell.rs:145-152`). The status setter is `shell_state::set_status` (`shell_state.rs:179-183`), and diagnostics go to `executor::shell_stderrln` (`executor.rs:514`). The ledger has `e(Builtin, "export", InSuite)` (`ledger.rs:83`). `ViError::PermissionDenied` exists (`libs/types/src/lib.rs:117-118`).
- **Contract.** Catalog `export` row: "retain explicit legacy Config-set effect only under Q grant … Granted key update visible through Config; denied key yields nonzero and no mutation" (`command-catalog.md:43`). Q row: "service-config must authorize attested principal + exact key/write operation under owner-approved delta before common export is enabled. Deny absent/wrong key grant and forged Config identity. Do not invent enforcement from ConfigClient" (`command-catalog.md:235`). The catalog also says "Do not bypass by adding ambient rights to common cellbox" (`:294`).

### Caller census (grep 2026-10-25, Rust/C/headers/Python/Lua outside `target`/`third_party`/`vendor`)

| Sender | Requests | Evidence |
|---|---|---|
| `cellbox` `export` | `Set` | `shell.rs:145-150` → `config_client.rs:87-108` |
| `cellbox` `ConfigClient::get` | `Get` (no live caller: "the shell currently never calls `get()`") | `config_client.rs:48-85` |
| hypha `tool-sys` `lookup_service` | none (registry lookup only) | `cells/apps/hypha/tools/sys/src/main.rs:99-120` |
| `service-httpd` `api_cells` | none (registry lookup only) | `cells/services/httpd/src/handlers.rs:159-178` |
| bench `init_giveup` | none (`ForceExit` on `/bin/config`) | `cells/tests/bench/src/scenarios/init_giveup.rs:33-34` |
| Lua runtime | no Config binding | `cells/runtimes/lua/src/main.rs:231-245` exposes net/vfs only |

`Delete` and `List` have **no** sender. The only reader API (`get`) has no live caller. No consumer reads `PATH`/`OS` from Config today.

## 2. Threat

1. **Unauthenticated integrity.** Any Cell that can `Send` to the Config tid can overwrite or delete any key, the seeded `PATH`/`OS` included. That covers `/apps`, `/data/bin`, `/home` sandbox apps, retained workers, `/bin/cellbox-ssh` and every service. The tid is public through `LookupService`. Payload claims are worthless (`caller_identity.rs:4-8`). [INFERENCE] Today's impact is bounded by the absence of readers, but every future reader inherits a store that any Cell can poison.
2. **Silent failure / false success in the shell.** `export` reports nothing and leaves `$?` unchanged whether the write landed, was refused or Config was down (`shell.rs:148-149`). Catalog scenario C1-e cannot be observed.
3. **Not a confidentiality issue.** Config is not a secret store (`command-catalog.md:235`). Reads are out of scope unless D-Q3 says otherwise.

## 3. Options

| | Mechanism | Per-key? | Kernel/ABI cost | Verdict |
|---|---|---|---|---|
| **Q1** | New kernel `CapSet.config_write`, requested by path for `/bin/cellbox` only and bounded by ceiling/policy, exported as trailer `CALLER_FLAG_CONFIG_WRITE = 1<<5`. service-config switches to `sys_recv_attested` and admits `Set`/`Delete` only with the flag **and** a key in the fixed shell namespace (D-Q2). System keys are immutable at runtime. | Yes (service-side namespace per right) | CapSet field + trailer bit + signed-policy CapSet **v7** (+1 byte) + 3 ceiling rows. No syscall, allowlist, manifest or `ConfigRequest` encoding change. | **Recommended.** It is the existing S2-N/S2-T/W pattern, attested and unforgeable, and it meets both catalog scenarios. |
| Q2 | service-config only: require an attested identity plus the existing `CALLER_FLAG_VFS_MUTATE` as a proxy, plus the D-Q2 namespace. | Yes | No kernel change | Not recommended. It conflates W with Q. Every non-sandbox row holds W (`targets.rs:7-9,72`), so only the sandbox is excluded and the services/workers listed in threat 1 stay writers. |
| Q3 | Per-key allowlist keyed by caller install path or launch row in a service-config table. | Yes (exact) | Needs caller path in the trailer (32-byte layout fully used) or a new identity syscall (no allowlist bit free). | Rejected: infeasible without a syscall/trailer ABI expansion that has no owner. |
| Q0 | Make Config read-only at runtime: service-config refuses every `Set`/`Delete` with `0xFC` (no kernel change). `export` always fails with status 1 and a message. | n/a | service + api const + cellbox only | Cheapest and honest, given there are no readers. It retires the "granted key update visible" half of the catalog row, so it needs a catalog amendment and user acceptance of that loss. |
| Defer | Record "legacy ungated `export` retained; Q deferred to P10" (dossier C1-e option 2). | — | none | Possible, but superseded by the user's 2026-10-25 choice; listed for completeness only. |

## 4. Recommended design (Q1)

### 4.1 Model

| Right | `CapSet` field (new) | Trailer flag (new) | Admits |
|---|---|---|---|
| Q (config write) | `config_write: bool` | `CALLER_FLAG_CONFIG_WRITE = 1<<5` | `Set` (discriminant 1), `Delete` (2), only for keys in the shell namespace |

- **Admission** happens in service-config, before taking the store lock and with no mutation. The classifier is pure and exhaustive (no `_` arm):
  - `Get`/`List`: ungated (D-Q3).
  - `Set{key}`/`Delete(key)`: deny unless all three hold: (a) `CallerIdentity::from_recv_buf` is `Some`; (b) `flags & CALLER_FLAG_CONFIG_WRITE != 0`; (c) `key` is in the D-Q2 namespace and not in `SYSTEM_KEYS = ["PATH", "OS"]`.
  - Denial reply: `ConfigResponse::Err(CONFIG_ERR_DENIED)`, `CONFIG_ERR_DENIED: u8 = 0xFC`. The value mirrors `NET_ERR_DENIED` (`ipc.rs:836-842`) and is distinct from `0xFF`, so the client reports "permission denied", never "config unavailable".
- **Grant table** (complete; anything absent gets no Q):

| Edge | Q | Reason |
|---|---|---|
| init boot row `/bin/cellbox` (`boot_ceiling.rs:119`) | ✓ | serial `cellbox sh` (`config/autoload.toml:23-25`) |
| `terminal-shell-edge` → `/bin/cellbox` (`profiles.rs:148-157`) | ✓ | GUI terminal shell |
| `phase04-driver-shell-edge` → `/bin/cellbox` (private) | ✓ | mirrors production so the lane proves the positive case |
| `/bin/cellbox-ssh` (`profiles.rs:75`) | — | ssh child never writes Config |
| every `reviewed_user_target_ceiling` row, sandbox default, `/bin/lua`, `/bin/python`, services | — | no sender in census |

`with_path_caps` requests Q only for the exact path `/bin/cellbox`. It does this in a separate `with_path_config_rights` helper, kept out of the `with_path_caps` body because `sign-policy.py` reads that body as the P-TRUST list (`cap.rs:481-483`). Q is not P-TRUST: `without_ptrust` keeps it, and `without_observe_mutate` keeps it.
- **Policy v7 compatibility:** `CAP_BYTES_V7 = 19` = v6's 18 bytes + `config_write` (literal `0|1`, otherwise the policy is malformed). A v1–v6 entry decodes `config_write = true`, so an older Permit does not narrow it and request ∩ reviewed ceiling stays the bound. This is the same rule as S2-N E1 and S2-T/W (`policy.rs:345-357`) because the write was ambient before this slice. See D-Q6.

### 4.2 Exact files

| Area | File | Change |
|---|---|---|
| API | `libs/api/src/abi/caller_identity.rs` | `CALLER_FLAG_CONFIG_WRITE = 1<<5`; reserved-bit doc becomes 6..31; round-trip test |
| API | `libs/api/src/services/ipc.rs` | `pub const CONFIG_ERR_DENIED: u8 = 0xFC`; doc on `Set`/`Delete`/`ConfigResponse::Err`. **No enum or encoding change** |
| Kernel | `kernel/src/task/cap.rs` | field in `CapSet`, `EMPTY`, `ALL`, `of_task`, `from_manifest` (false), `with_path_config_rights`, `intersect`, `apply_to`, `from_task`, `config_caller_flags()`; tests |
| Kernel | `kernel/src/task/tcb.rs` (`:377-385` region) | `config_write: bool`, default false |
| Kernel | `kernel/src/task/syscall.rs:2889-2910` | OR `CALLER_FLAG_CONFIG_WRITE` from the root CapSet |
| Kernel | `kernel/src/policy.rs` | `VERSION_V7`, `CAP_BYTES_V7 = 19`, decode/compat rule, selftest vector |
| Kernel | `kernel/src/loader/boot_ceiling.rs:117-119`; `kernel/src/loader/launch_profile/profiles.rs:148-178`; `launch_profile/tests.rs` | three rows per §4.1; row tests (cellbox-ssh and a reviewed child row still lack Q) |
| Signer | `scripts/sign-policy.py` | `VERSION = 7`, `CAP_BYTES[7] = 19`, `config_write_request(path)` mirroring the kernel helper |
| Service | `cells/services/config/src/main.rs` | `sys_recv_attested`; call the classifier before dispatch; `#![cfg_attr(target_os = "none", no_std/no_main)]` as service-net does (`cells/services/net/src/main.rs:1-2`) so host tests build |
| Service | `cells/services/config/src/rights.rs` (new) | pure `admit(&ConfigRequest, Option<&CallerIdentity>) -> Result<(), u8>`, `SYSTEM_KEYS`, key syntax; host tests |
| Client | `cells/tools/cellbox/src/config_client.rs:87-108` | `Err(CONFIG_ERR_DENIED)` → `ViError::PermissionDenied`; others stay `IO`; reject an encoded request longer than `IPC_BUF_SIZE - CALLER_IDENTITY_LEN` with `InvalidArgument` (the attested tail is kernel-owned) |
| Shell | `cells/tools/cellbox/src/shell.rs:145-152` | export status/message per §4.3 via a pure host-tested helper |
| Fixture | `cells/tests/phase03-filter/src/main.rs` | new `config-probe` mode (rights-free edge) |
| Lane | `cells/tests/phase04-terminal-driver/src/main.rs`, `scripts/qemu-phase04-terminal-test.sh` | §4.4 steps/checks |
| Docs | `command-catalog.md` rows `export`/Q (`:43,235`), `acceptance.md`, `CHANGELOG.md` | evidence after the lane passes |

`ledger.rs:83` stays `InSuite`: export is still an in-suite builtin, now enforced by the service.

### 4.3 `export` behaviour

| Case | stdout | stderr | `$?` | Config |
|---|---|---|---|---|
| `export K=V`, admitted | — | — | 0 | `K=V` |
| refused (`0xFC`: no Q, system key, key outside namespace) | — | `export: K: permission denied` | 1 | unchanged |
| Config unreachable / other reply | — | `export: K: config unavailable` | 1 | unknown, not claimed |
| no operand, or operand without `=` | — | — | unchanged (today's behaviour) | unchanged |

The client never pre-filters keys (catalog `:235`: "do not invent enforcement from ConfigClient"). The service is the sole decision point. `export` still does not mirror into shell variables, and `env` still does not read Config (`command-catalog.md:40,43`).

### 4.4 Tests and lane evidence plan

- **Host:**
  - `cargo test -p service-config --target x86_64-unknown-linux-gnu`: classifier matrix covering {no identity, identity without Q, identity with Q} × {Set, Delete} × {namespace key, `PATH`, `OS`, empty, over-long, bad syntax}, plus Get/List always admitted. A runtime-level test shows a denied Set/Delete leaves the map byte-identical and an admitted one mutates it.
  - `cargo test -p api`: flag constant/trailer round trip; bit 5 no longer reserved.
  - Kernel host tests: `intersect`, `from_manifest` false, `with_path_config_rights` (only `/bin/cellbox`), v7 encode/decode, v6 Permit leaves Q unnarrowed, malformed byte rejects the policy. Policy selftest vector. `sign-policy.py` v7 output decodes identically.
  - `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`: export outcome helper (0 / denied message / unavailable message).
- **Private lane** (Phase 04 terminal runner on the private image built by `scripts/build-phase04-qemu-image.sh`; no canonical image change). New checks:
  - `config-export-granted`: `export P01Q_OK=1; echo $?` → `0`; `/bin/p03-filter config-probe get P01Q_OK` prints `1`.
  - `config-export-system-denied`: `export PATH=/tmp; echo $?` → `1`, stderr `export: PATH: permission denied`; the probe reads `PATH` = `/bin`.
  - `config-export-namespace-denied`: `export a.b=1` → `1`; the probe reads `NotFound`.
  - `config-probe-set-denied`, `config-probe-delete-denied`: the rights-free `p03-filter` Cell (edge `phase03-filter-fixture-edge`, `LEGACY` ceiling, `profiles.rs:216-230`) sends `Set{P01Q_PROBE}` and `Delete(OS)`, gets `0xFC`, and the following `Get`s show `NotFound` / `Cellos`.
  - `config-denial-on-stderr-only`: the denial text is absent from stdout captures.
- **Regression:** S3 terminal runner (all prior checks), Phase03 runner (60), Phase02 runner; cellbox x86_64-unknown-none build; `scripts/cellbox-ceiling-diff.py` shows no syscall-ceiling change (Q is a CapSet field, not an allowlist bit).

## 5. Risks

- **Signed-artifact format change** (policy v7). Old policies must still verify (D-Q6). Release signing stays blocked by rollout containment, as for v5/v6.
- **Q is per Cell, not per invocation.** Every command in the granted shell Cell can write namespace keys. The key namespace is the per-key bound. Exact per-key grants per caller are Q3 and need ABI that has no owner.
- **Fail-closed for unlisted writers** is intended. The census in §1 must be re-run at implementation start.
- **Store growth.** The granted shell can still add unbounded keys (D-Q7).

## 6. Decisions requested (with recommendations)

- **D-Q1 Mechanism:** **Q1** (recommended), Q0 (read-only Config; catalog amendment), or Q2.
- **D-Q2 Shell key namespace:** **(a)** shell identifiers `[A-Za-z_][A-Za-z0-9_]{0,63}` minus `SYSTEM_KEYS {PATH, OS}` (recommended; keeps `export NAME=V`); (b) a fixed prefix such as `USER_` only; (c) an exact key list.
- **D-Q3 Reads:** **`Get`/`List` ungated** (recommended; not a secret store), or require an attested identity.
- **D-Q4 `Delete`:** **gated like `Set`** (recommended; no sender today), or refused for everyone.
- **D-Q5 Grant holders:** **the three `/bin/cellbox` shell edges in §4.1** (recommended), or the boot row only (the GUI terminal shell then gets the refusal path).
- **D-Q6 Policy compatibility:** **v1–v6 decode `config_write = true`** (recommended; same rule as S2-N E1 / S2-T/W), or `false` (strips Q from every legacy-listed path under a loaded policy).
- **D-Q7 Store bounds:** **out of this slice** (recommended), or add key-count/value-length caps with a separate capacity code.

## 7. Rollback

Revert the files in §4.2, then cold-boot a matched previous image and policy. Config state is in-memory only (`main.rs:27-37`); hot-swap carries the map, not rights. Rollout, Phase 04 criterion 4 and Phase 10 remain blocked independently of this slice.

## 8. Results (2026-10-25, implemented; awaiting CP2)

**Implemented per §4.** No canonical image was touched. The source digests below are full SHA-256 values, taken after the lanes passed.

| File | Change | SHA-256 |
|---|---|---|
| `libs/api/src/abi/caller_identity.rs` | `CALLER_FLAG_CONFIG_WRITE = 1<<5`; reserved bits are now 6..31; test | `924a0325566282747d8b8afd3023ef5a7e24ec7f3b57fe6cb3aefc9033ddac8f` |
| `libs/api/src/services/ipc.rs` | `CONFIG_ERR_DENIED = 0xFC`; docs only, no enum/encoding change | `53600a6a38bf48f72600af4d884035726af21db23c05bd16189610e9a54e9ba4` |
| `kernel/src/task/cap.rs` | `config_write` in every CapSet constructor/op; `with_path_config_rights` (`/bin/cellbox` only); `config_caller_flags`; test | `fc4b8bfc7857d6a07549055966d43716c009ee3c2a067b0cf139f0264a45803f` |
| `kernel/src/task/tcb.rs` | `config_write` field, default false | `2a83ff56af16b9f2afd1e13dac7439ba1b2b3e0ffbbd3c63b00b61fe2f57f0a9` |
| `kernel/src/task/syscall.rs` | `attested_identity_of` ORs the flag from the root CapSet; test | `5cc2ef44f945df8db637b1959e36213e852ed2a878b485ba370959ae8934b4d0` |
| `kernel/src/policy.rs` | v7 / `CAP_BYTES_V7 = 19`; v1–v6 decode Q as true; narrowing bit 16 `cfgw`; `v7_parse_cases` self-test; host tests; signer test now v7 | `e1e07e7184f261711093083098e06d39e6747754908a7ee16a016001af54c779` |
| `kernel/src/audit.rs` | doc: `CapNarrowedByPolicy` bit16 `config_write` | `a731029f1bc49e4df704769f947966821c5fb500b79e2d5726fecdd40bc6e020` |
| `kernel/src/loader/boot_ceiling.rs` | `/bin/cellbox` row gets LEGACY + `config_write`; `/bin/init` row has `config_write: false`; refusal log | `d4c548d27e33fab439f04d93300f921ebeadc0a03024318c7030894e29169b09` |
| `kernel/src/loader/boot_ceiling/selftest.rs` | boot self-test: only `/bin/cellbox` keeps Q | `0f9ff98fde7d190036a61d44239533746f89ad11ebd1a6c0ca81b12c079bbdb3` |
| `kernel/src/loader/launch_profile/profiles.rs` | `SHELL_EDGE` for the terminal and private driver shell edges; the terminal-edge comment no longer says "empty child ceiling" (user-approved wording fix) | `12ff2f739cd7440d2d6b9bce22bef32460b764df6411aff396ebe6a3a93134e2` |
| `kernel/src/loader/launch_profile/tests.rs` | shell-edge ceilings and the Q grant table | `b3d19ebd2670207afdb607c023729643c6ac56edfa2453d5294b2ae37f718170` |
| `scripts/sign-policy.py` | emits v7; Q column defaults to the kernel request (`CONFIG_WRITE_PATHS`); decoder handles v7 | `70a6fb27bdff390a4a42e3eaeebe11fd48aaa619488950d512da97c6ed308598` |
| `cells/services/config/src/main.rs` | `sys_recv_attested`; pure `handle()` with admission before any store access; host-testable (`cfg_attr(target_os = "none")`); banner v0.4 | `51398ba59c1cf0c635fda8ad4871fb74d3256ab226b25755ed917d8c38ee3a14` |
| `cells/services/config/src/rights.rs` (new) | `admit`, `SYSTEM_KEYS`, `shell_key_admitted`; exhaustive match | `6c03ed6444e63518c05e794825d1448ef3f17d742561c9459e040ab16d62bdf6` |
| `cells/tools/cellbox/src/config_client.rs` | `0xFC` → `PermissionDenied`; tail-bound `encode_set`; `export_outcome`; tests | `2ed3fdcbbde355f731c836bf78a6cea9a988228f7c15bfbc9f93792423751844` |
| `cells/tools/cellbox/src/shell.rs` | `export` sets `$?` and writes the diagnostic to stderr (§4.3) | `5eda97a5e5c6397295db823297752cc3947cbc6967302f502754b2a72837887e` |
| `cells/tests/phase03-filter/src/main.rs` | `config-probe get/set/delete` mode | `2d812d2c7fc32f06d5271e88e57afdf9c89acca3568a3eb8f1eb5db5ffd1b04a` |
| `cells/tests/phase04-terminal-driver/src/main.rs` | `config_rights` step (six markers) | `8b05c055069dbe4c233d209e0a7ca73d8c70e5102671970d387c4d149b266f64` |
| `scripts/qemu-phase04-terminal-test.sh` | six expected markers; `config-probe-not-on-uart`; private plan starts `/bin/config`; profiles/tests digests re-pinned with a comment | `877fad35db36b2cdb6bb442adf313d7c49ceec3d18ed69662fa7382da048dabf` |

**Verified (2026-10-25):**
- **Host tests:**
  - `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 284 passed, both default and `--features phase04-driver-profile`. The run includes the 6 new Q/v7 tests and the v7 signer test, which runs `scripts/sign-policy.py`.
  - `-p api`: 153 passed.
  - `-p service-config`: 7 passed (new).
  - `-p app-cellbox`: 189 passed (+3).
  - `-p service-net`: 56 passed.
- **Target builds:**
  - `app-cellbox`, `service-config`, `app-phase03-filter` and `app-phase04-terminal-driver` built for x86_64-unknown-none (pic, build-std).
  - `cellos-kernel` x86_64-unknown-none release built (default features, separate target dir).
  - The signer emits a 2013-byte v7 blob.
- **Phase 04 terminal lane** (`/tmp/cellbox-phase04-t8uH`): exit 0, **466 checks / 180 driver markers**.
  - It includes `config-export-granted`, `config-export-system-denied`, `config-export-namespace-denied`, `config-denial-on-stderr-only`, `config-probe-set-denied`, `config-probe-delete-denied` and `config-probe-not-on-uart`.
  - The cellbox ceiling is unchanged (G-S1a PASS).
- **Phase 03 lane** (`/tmp/cellbox-phase03-CQZi`): exit 0, 67 PASS, 0 FAIL.
- **Phase 02 lane** (`/tmp/cellbox-phase02-l6tV`): exit 0, 48 passes.
- **`Cargo.lock`:** unchanged by this slice.

**Deviations / findings:**
1. **Config is not started in the default profile.** `scripts/generate-boot-config.py` `selected()` starts `/bin/config` only for the `ai` feature. The first lane run therefore failed: every probe got `ipc-NotFound`, and `export` correctly reported `config unavailable` with status 1. The private lane plan now adds the `config` service row (service_id 4, the same as `config/services.toml`); the repository templates are unchanged. Consequence for the canonical image (not changed here) [INFERENCE from `selected()`]: in an image without `ai`, `export K=V` now prints `export: K: config unavailable` with `$?` 1, where it previously reported success and dropped the write.
2. `List` with an unencodable reply now answers `Err(0xFF)` instead of no reply, the same as the `Get` path already did. service-config's `Mutex` was removed (single-threaded loop).
3. `export` with no operand or no `=` is unchanged (§4.3). Because the shell intercepts `export` before parsing, `export K=V; echo $?` on one line is not two commands. The lane therefore reads `$?` on its own line.
4. `ConfigClient::get` remains callerless (unchanged).

Not done here: the `command-catalog.md` `export`/Q rows, `acceptance.md` and `CHANGELOG.md` (owned elsewhere; text supplied to Main).
