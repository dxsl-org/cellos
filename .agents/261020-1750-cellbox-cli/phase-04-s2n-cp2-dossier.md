# S2-N CP2 dossier — network/DNS/listen/L2 rights enforced by service-net

**Status: CP2 APPROVED by the user on 2026-10-22 (“duyệt bốn CP2”) for dossier SHA-256 `5a7fa43deb7dba4c51001a1460e2efd6cfdd7513ba43e4c6fb6a4ee8fe3e73eb` (digest before this status amendment).** CP2 would authorize the S2-N source, the hunks attributed to S2-N below at the exact file hashes listed, as the S2-N implementation baseline **only**. Quarantine, canonical packaging, release signing (including any release-signed policy v5/v6 blob) and rollout stay **BLOCKED**.

Prepared 2026-10-22 (S4b of the [S4 retirement proposal](phase-04-s4-retirement-proposal.md)) from source bytes, read-only `git diff` against `HEAD` (`3471028bc`), and the saved logs in `reports/s4-final-evidence/`. The only new execution was `sha256sum`/`stat` and read-only git and grep. No build, test or QEMU run was done for this dossier.

## Authority

- [S2-N annex](phase-04-s2n-net-annex.md): text approved by the user ("Duyệt annex + theo khuyến nghị") with decisions D-N1 coarse V1, D-N2 sandbox gets no rights, D-N3 lua/python get no rights, D-N4 `GetLocalIp` ungated. **CP1 APPROVED** ("duyệt") for annex SHA-256 `d774b9c5386f5d400cf4717c9e518ae4e0836d130d7a3ae71b724df351476255`.
- Implementation results and deviations are in annex §12. This dossier adds no scope.
- S2-T/S2-W ([CP2 dossier](phase-04-s2tw-cp2-dossier.md)) was implemented **after and on top of** S2-N in the same files. Its hunks are not part of this request.

## Delta summary

The kernel adds four kernel-only `CapSet` rights: `net_resolve`, `net_connect`, `net_listen` and `net_l2`.

1. **Request and bounds.** The install path requests the rights (`with_path_net_rights`, following the annex §5 grant table). The reviewed launch or boot ceiling bounds the request, and policy v5 bounds it further. v1–v4 entries decode the net rights as `true`, so legacy entries do not narrow them.
2. **Attestation.** `attested_identity_of` exports the rights of the **Cell root task** as trailer flags `CALLER_FLAG_NET_{RESOLVE,CONNECT,LISTEN,L2}` = bits 1–4.
3. **Enforcement.** service-net checks every attested request with the exhaustive classifier `rights::required_right`. This runs **before** the owner watch, the ready/resolve intercepts and dispatch. A missing right gets `NetResponse::Err(NET_ERR_DENIED = 0xFC)`.
4. **Clients.** `ostd` maps `0xFC` to `ViError::PermissionDenied`. curl, wget, mqtt, nc and ocel print an explicit permission message.
5. **Audit.** The policy narrowing audit uses bits 9–12, and a new event `AuditEvent::NetRightsGranted = 27` records the grant.

The `NetRequest`/`NetResponse` wire encodings are unchanged.

## Evidence

All line numbers refer to files under `reports/s4-final-evidence/`. The runs exercised the **combined** tree: S2-N + S2-T/W + S4a + every earlier delta.

| Check | Saved log lines | Result |
|---|---|---|
| Restricted networking | `phase04-terminal.log:333` (`private services plan: … ('net', '/bin/net')`, `('e1000', '/bin/e1000')`), `:522`, `:869` `PASS qemu-restricted-slirp-no-forward` | One e1000 on `-netdev user,restrict=on`, no forward |
| Net service live | `phase04-serial.log:242` `SpawnFromElf: /bin/net`, `:265-266` `DHCP acquired`, `IP address: 10.0.2.15` | — |
| Rights-free probe denied 7/7 | `phase04-terminal.log:580-586` `PASS marker:net-denied-{resolve,tcp-connect,udp-create,udp-bind,tcp-listen,multicast-join,l2-send}`; `phase04-serial.log:1542-1548` | Each reads `denied` (= `Err(0xFC)`) from `/bin/p03-filter net-probe` on the cellbox edge (`serial:1540-1541`: `'p03-filter'` loaded, task 38 exited 0) |
| Probe completed | `phase04-terminal.log:587` `PASS marker:net-probe-done`, `:867` `PASS net-probe-not-on-uart` | — |
| curl admitted, network failure | `phase04-terminal.log:588` `PASS marker:net-curl-admitted`, `:863-866` `PASS net-curl-frame`, `net-curl-network-failure-uart`, `net-curl-not-refused-uart`, `net-no-response-uart`; `phase04-serial.log:1550-1562` | Inside the frame: `[net-bridge] NIC driver reply timeout; frame not acknowledged` (`:1558`), `curl: empty response` (`:1559`), `net-curl-end elapsed_ms=30062 status=Ok(["stcurl=0"])` (`:1561`) |
| Whole runner | `phase04-terminal.log:870-874` (`no-unexpected-markers`, `no-FAIL-markers`, `driver-final-verdict`, `no-unexpected-launch-deny`, `unknown-program-denied`), `:878` `PASS: 353 Phase04 S3-output runtime checks (87 driver markers)` | 354 `PASS` lines = 353 checks + the summary line; 87 `PASS marker:` lines and 87 serial `P04 PASS` lines |
| Signed private artifacts | `phase04-terminal.log:486` Cargo.lock `e58e780a…`, `:495-496` driver-e1000 `cedccf59…` and service-net `6df6d221…`, `:508` curl `010a071a…`, `:510` phase03-filter `ab8534ae…`, `:515` kernel `ce4045bb…`, `:516` ISO `fa751cb6…` | Private throw-away key `d82b5f55…` (`:13`), not release keys |
| Phase03 regression | `phase03.log:397-456` (60 `PASS` lines), `:457` `PHASE03 PRIVATE FIXTURE: observations complete` | 60/60 |
| Phase02 regression | `phase02.log:404` (`networking disabled`), `:407` `passes: 48`, `:409` completion line | The log does not print an exit status. "Phase02 exit 0" is recorded in the [S4a Results](phase-04-s4-retirement-proposal.md#results--s4a-2026-10-22) |
| On-image kernel self-tests (evidence kernel boot) | `phase04-serial.log:123` `policy verify+parse self-test PASS`, `:127-128` `[selftest] boot-ceiling: PASS` | `policy::self_test` runs `v5_parse_cases` and `v6_parse_cases` (`policy.rs` 438-443, called from `kernel/src/main.rs:1369`). `boot_ceiling/selftest.rs` holds the net boot-row cases (197-225) and the httpd path/ELF edge checks (227-246). The same boot logs the historical `COMPLETION-QUEUE: FAIL` (`serial:108-110`), which is unrelated to S2-N and already disclosed in earlier CP2s |
| Host tests | **No raw log saved.** Annex §12 records agent runs only: service-net 56 (13 new), api 129, ocel 172, net-tools 11, ostd net 1, kernel cap/policy tests, four-target check | Not re-run here |
| First S2-N image run (342 checks / 77 markers) | **No raw log saved.** Annex §12 only | Superseded by the 353-check run above |

`phase04-terminal.log:10` `G-S1a FAIL` is the informational Terminal `SetTimer` delta approved earlier. It is not S2-N.

**Evidence ↔ current source.**
- The run's `cargo_lock_sha256` (`terminal.log:486`) equals the current `Cargo.lock`.
- The runner's embedded pins for `launch_profile/{mod,profiles,targets,tests}.rs` (runner lines 196-202) equal the current hashes below. The pin check runs before key generation under `set -euo pipefail`, and the log proceeds to `:12`, so the pinned files matched at run time.
- No other file in this inventory has a per-file hash in the evidence. Their mtimes (latest 18:16) precede the log files' mtimes (18:29), but those are copy times. The claim that the run used exactly these bytes for the remaining files is therefore an `[INFERENCE]`.

## Proposed implementation inventory — current disk SHA-256

**Exclusive to S2-N** (every changed hunk is S2-N, or the file is new for S2-N):

| Path | State | Bytes | SHA-256 |
|---|---|---:|---|
| `cells/services/net/src/rights.rs` | new | 5498 | `be28f955afa58db6f4b49365b7ce2c4376de2c252f33d6a9c0174c062c0e5442` |
| `cells/services/net/src/rights/tests.rs` | new (6 tests) | 5535 | `f9783014bc7829bb5eee3e90701a568af2d55985998cb34af5a915fe347cc258` |
| `cells/services/net/src/service-runtime-admission-tests.rs` | new (7 tests) | 11673 | `95aff118a05574456d91562cb0e93b31529fa33ecd099b3ecea8b37607384486` |
| `cells/services/net/src/service-runtime.rs` | modified: `admit_request` + `dispatch_admitted` (362-408), gate call (≈613-623), test module hook | 29417 | `80c64227c53cf28fa01d20dd09094528f631d1419a4be81f832ba03c50b027ff` |
| `cells/services/net/src/main.rs` | modified: `mod rights;` | 2215 | `76c486d06511d0ab72c567c2318b05326c88a2e795abdcc917a4116967766ced` |
| `cells/services/net/src/interface.rs` | modified: `#[cfg(test)]` `tx_queue_len`/`guest_mac` (362-373), used only by the admission tests | 31770 | `08d0a7a8742248396380c1aeed045cc5580294bb3ceba4fa508750f995233684` |
| `libs/api/src/abi/caller_identity.rs` | modified: `CALLER_FLAG_NET_*`, `CALLER_FLAG_NET_RIGHTS_MASK` (47-65) + test | 9955 | `8ca506d4cc609ecc5aef2a37e80e7ba685c9e9bd2e5804b7c93b624a7fa03adb` |
| `libs/ostd/src/clients/net.rs` | modified: `net_error` + 6 call sites + test | 9341 | `7e3dd281b0895efce43944eb204cfe7e07f08da51ea5efaaa191706996382ce5` |
| `cells/tools/net-tools/src/bin/curl.rs` | modified: denial message, `resolve_host` → `Result` | 13174 | `f4c78ca3fa18de95a71896466ee4690d43f87c60aef0383c74ffb3b3c804e56a` |
| `cells/tools/net-tools/src/bin/wget.rs` | modified (same pattern) | 13323 | `760107257a6881d0f077291fd6c3512c9508f1f179c3977b5e06348122912b30` |
| `cells/tools/net-tools/src/bin/mqtt.rs` | modified (same pattern) | 15030 | `2daca7411542c18132cf6d29905939fb6c0cc8a535ac8bad6d841a99c6938ce6` |
| `cells/tools/net-tools/src/bin/nc.rs` | modified (same pattern + listen denial) | 12703 | `f20c3b4604873c341b6a94d853f80394100fa84f3e1e73b249e384916ae0c9bb` |
| `cells/apps/ocel/src/net/http.rs` | modified: `NET_DENIED_MESSAGE`, two `Err(NET_ERR_DENIED)` arms | 45673 | `e6d7b9a88c286c12cc8923cbce72abad9102dfca2a9b5b2e7a963878144828aa` |

**Shared files.** Each is hashed once, as a whole file. A hash covers the hunks of every delta in that file. Approving this dossier covers only the S2-N hunks named here.

| Path | Bytes | SHA-256 | S2-N hunks (current line numbers) | Other deltas in the same file |
|---|---:|---|---|---|
| `kernel/src/task/cap.rs` | 43493 | `8676f4330363056d7fed88e3723b66d6079e6173e44dc01b29fb76b318598cd2` | fields 236-249; the first four net lines of each field-list block (`EMPTY` 277-280, `ALL` 324-327, task snapshot 347-350, `from_manifest` 402-407, `intersect` 581-584, `apply_to` 624-627, `from_task` 649-652); `with_path_caps` tail 471; `with_path_net_rights` 475-498; `net_caller_flags` 591-608; tests 842-1025 | **S2-T/W**: 250-260, the three lines after each net block, `LEGACY_OBSERVE_MUTATE` 286-295, 472, 500-517, `without_observe_mutate` 553-565, tests 1026-1139. S1 rename doc line 22 |
| `kernel/src/task/tcb.rs` | 40658 | `65effec593a0c39870e27075e9afd16dd3681127c04c90d69e011419322c5b7b` | net fields + doc (370-377), defaults 657-660 | **S2-T/W** 378-385, 661-663; Phase02 `child_context`/`child_stdio`; S1 rename doc |
| `kernel/src/task/syscall.rs` | 417105 | `79cd3ec36c8aacf81ecd7c5238689e518f27dfe6e40ebcbb2322c9e0048e0286` | `attested_identity_of` 2892-2898 (root-task net flags; a missing root yields `None`) | **S2-T/W** 2891, 2909-2920, `check_allowlist`/`observe_right_held` ≈9508-9614, tests ≈9755-9905; S2c raw shutdown (`authorize_raw_shutdown`); Phase02/S3 `CellSessionV1`, user-output preflight and launch-edge hunks; S1 rename |
| `kernel/src/policy.rs` | 50520 | `b34b9af9f8f88f0cddd2e3c532b59e4ac7036d1b3b0f7869e3a576fed78e0196` | `VERSION_V5` 44-46, `CAP_BYTES_V5` 56, `cap_bytes_for` 73, v5 decode 327-349, entry fields, `v5_parse_cases` (from 652), narrowing bits 9-12 (939-942), `NetRightsGranted` audit 975-986, tests `v5_entry_round_trips_every_network_right`, `v4_permit_leaves_network_rights_unnarrowed`, `malformed_network_byte_rejects_the_policy` | **S2-T/W**: v6 constants/decode (350-…), `v6_parse_cases`, bits 13-15, v6 tests and `signer_v6_output_decodes_with_kernel_rule`. The shared stride comment and wider audit format string serve both. S1 `is_trusted_core` rename |
| `kernel/src/audit.rs` | 11233 | `d6fa7e0a1d16adeace6c9c802aac4e5543bdc40938d6093f81109a720be17c09` | doc bits 9-12 (82), `NetRightsGranted = 27` (128-133) | **S2-T/W** doc bits 13-15 (83) |
| `kernel/src/loader/launch_profile/targets.rs` | 5469 | `62a67cbbe89ae7ff2d61e4a9e74257e7a9657314973c99f4146eb1b020e2ab24` | `NET_CLIENT`/`NET_LISTENER`/`NET_PEER` (10-27), rows 76-81, net-broker `..NET_PEER` 84, hypervisor `net_l2` 97, robot-demo `..NET_CLIENT`, removal of curl/ocel from the EMPTY list, sandbox comment (D-N2) | **S2-T/W**: `LEGACY` (7-9). Every `..LEGACY` base, **including the base of the S2-N constants**; `/bin/s2tw-probe` row 118-123 |
| `kernel/src/loader/launch_profile/profiles.rs` | 13851 | `3140c61b0765100bdf14eea8b8455e294c069abbffd71aa86885367377f0b0ca` | hypha `/bin/llm-gateway` → `reviewed_user_target_ceiling(target)?` (90; deviation 3) | **S2-T/W** `LEGACY` (12-14) and every `LEGACY` row; Phase02/03/04 fixture profiles, terminal edge, S2c shutdown row, S1 rename |
| `kernel/src/loader/launch_profile/tests.rs` | 33844 | `a0f6aa7bbb7937f237301395de8398b31140ecfc16bb5d8b767ec51410ecee86` | hypervisor edge `net_l2` (51-63), `net_rights`/`network_worker_edges_carry_only_their_grant` (65-144), httpd removed from the capability-free list (deviation 6), `hypha_fixed_edges_remain_exact` llm-gateway (375-390) | **S2-T/W** `LEGACY`, 620-1000 (`every_pre_annex_*`, `s2tw_probe_fixture_row_is_feature_gated`, `elf_route_ignores_only_the_legacy_rights`); S2c/S3/Phase04 driver tests; S1 rename |
| `kernel/src/loader/boot_ceiling.rs` | 13369 | `cebbb73af1bb1525f73de3510173b0f846a4ba27a3eae63fa4a70d2bbdae22d0` | init net fields false (76-82), `/bin/net` row split, `/bin/net-broker` D+N+Nl (108-117), `/bin/hypervisor` `net_l2` (134-140), `/bin/sshd` listen (191-195), `log_refusal` net lines 258-269 | **S2-T/W** `LEGACY` (45-48), init X/M/W, every `..LEGACY`, sandbox comment, refusal lines 270-278; S1 `/bin/shell`→`/bin/cellbox` |
| `kernel/src/loader/boot_ceiling/selftest.rs` | 10599 | `c1b9796daf7318deb4b17467fc1b938ba03a45190cbd8c860728e1813f69f31b` | net boot-row cases (197-225), httpd path/ELF edge checks (227-246) | **S2-T/W** 39, 152-157, 165-195, 254; S1 rename |
| `scripts/sign-policy.py` | 24555 | `1ebeefada57020464f2cf87feb2858b178694d26d07bbb0ff4755257979f2c4b` | `NET_*` tuples (102-105), net columns on the net-broker/hypervisor/robot-demo/llm-gateway/nc/curl/wget/httpd/mqtt/https-demo/sshd/ocel rows, net decode/domain checks, E1 compatibility rule (≈329) | **S2-T/W**: `VERSION = 6` (the signer now emits **only v6**), `OBSERVE_MUTATE`, `observe_mutate_request`, `full_row`, v6 encode/decode. S1 `/bin/cellbox` row |
| `libs/api/src/services/ipc.rs` | 27985 | `9ef29631ecb8319cb8efd13ab74fbcaf7ab193446972dd0fd7d2167f583c206e` | `NetResponse` error-code doc (≈488-492), `NET_ERR_DENIED = 0xFC` (≈510-516) | Phase02 `PreparedChildV1`, `SessionV1` and wire tests |
| `cells/tests/phase03-filter/src/main.rs` | 10935 | `8971d38593ca9d1729ad8fb42e5c523cf4b798e98b995ff6204d8b650d74870a` | `net-probe` doc (21-27), imports (44, 46), `PROBE_FRAME` 238-253, `probe`/`net_probe` 255-295, dispatch 316 | Phase03 fixture (crate new, untracked) |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 53581 | `0139d5cc3404d092d848ba756d73c680bbeeaa0eeea59c912625cf7166d8fe61` | call `net_rights(&mut a)` 329, `NET_PROBE_OPS` + `net_rights` 777-835 | **S2-T/W** 330, 837-918; S3/S2-K/S2c/S4a driver |
| `scripts/qemu-phase04-terminal-test.sh` | 42941 | `d4cdaadaffe48949d3122ca9ab81863b3732b8bce7f7de4d2dab382a0ee884a6` | header 35-41; net/e1000/curl/p03-filter packaging (241-258, shared lists); acceptance-limit text 306-309; services.toml check 377-381; provenance `network` 459; restricted SLIRP boot and guard 467-482; expected markers 551-553; UART checks 671-688, 692-696 | **S2-T/W** 43-48, 76, 267, 399-413, 310-314, 554-557, 689-691; digest pins 189-202 (S2-N+S2-T/W rows); S3/S2-K/Phase04 runner |

### Context files: not S2-N hunks, listed for completeness

- **`kernel/src/loader/launch_profile/mod.rs`** (`b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c`, 4459 B): deviation 4 relies on its pre-existing caller-ELF gate "non-empty ceiling → refused". S2-N did not edit the file. S2-T/W later changed that gate to `without_observe_mutate()` (line 109). Hashed in the [S2-T/W dossier](phase-04-s2tw-cp2-dossier.md).
- **`cells/services/net/src/dns.rs`** (`a752a1fd4841af69fe61d16575abde7788a04cf1cd9cd77612c82e04de299c8f`) and **`dns/tests.rs`** (`1c97f4f8c2fc76cb86c59e927c62dde351b69552c3b08ef7c345463514470525`) are modified in the same crate: 32 DNS query slots plus one test. They are **not attributable to S2-N**: they are absent from annex §12, and their mtime is 2026-10-18, before the annex. They are excluded from this request, but they are compiled into the evidence `service-net` binary.
- **`cells/apps/ocel/src/main.rs`**: its only diff is key-repeat handling, which is not S2-N. Excluded.
- **`libs/api/src/services/posix/net.rs`**: unchanged (deviation 2).

### Relationship to earlier approved hashes

None of the earlier approvals extends to the current bytes. These files changed after earlier CP2s, through several later deltas including this one:

| File | Earlier approval | Approved hash | Current hash |
|---|---|---|---|
| `ipc.rs` | Phase02 CP2 | `b3bf300e…` | `9ef29631…` |
| `tcb.rs` | Phase02 CP2 | `9def966e…` | `65effec5…` |
| `syscall.rs` | Phase02 CP2 | `ea8443a6…` | `79cd3ec3…` |
| `targets.rs` | Phase02 CP2 | `8892a225…` | `62a67cbb…` |
| `profiles.rs` | S3-output CP2 | `cd51eb0a…` | `3140c61b…` |
| `tests.rs` | S3-output CP2 | `a60748c8…` | `a0f6aa7b…` |
| driver | S3-output CP2 | `ec167edc…` | `0139d5cc…` |
| runner | S3-output CP2 | `bcce7aad…` | `d4cdaada…` |

## Caller census (read-only grep, 2026-10-22)

- **`CapSet::net_*`** (kernel-only):
  - Written by `with_path_net_rights` and bounded by the target/boot rows above.
  - Narrowed by policy `parse`/`apply`.
  - Copied by `apply_to`/`from_task`/the task snapshot. Threads inherit them through the same copy.
  - **Read only by** `net_caller_flags` ← `attested_identity_of` (syscall.rs 2898) and by `log_refusal`.
- **`CALLER_FLAG_NET_*`**: produced by kernel `net_caller_flags`; consumed only by service-net `rights.rs` (`Right::flag`, `admit`).
- **`NET_ERR_DENIED` (0xFC)** is produced only by service-net `rights::Denial::reply`. Consumers:
  - `ostd::clients::net::net_error`, and through it `NetClient` users (e.g. `cells/apps/hypha/llm-gateway/src/transport.rs`, `libs/ostd/src/app.rs`, `libs/port-platform`);
  - net-tools curl, wget, mqtt and nc;
  - ocel `net/http.rs`;
  - phase03-filter `net-probe`.
- **Raw TLS opcode `0x30`**: gated by `raw_tls_required_right`. A denial returns cap 0, the existing connect-failure reply (deviation 1).
- **Every `NetRequest` sender** is now subject to admission. Grep census by file:
  - **Granted senders** (each row in brackets): net-tools curl/wget/mqtt/nc (D+N, nc +Nl); ocel `net/http.rs` (D+N); `cells/services/httpd/src/reactor.rs` (`/bin/httpd`, Nl); `cells/services/sshd/src/main.rs` (Nl); net-broker `beacon.rs`, `stun.rs`, `connection_manager.rs`, `transport/tcp_framing.rs` (D+N+Nl); `cells/services/hypervisor/src/net_backend.rs` (L2); robot-demo `mqtt.rs` (D+N); llm-gateway (D+N via the hypha edge).
  - **No grant, fail-closed by decision**: `cells/runtimes/lua/src/bindings_net.rs` (D-N3); `cells/demos/{async-echo,tokio-smoke}` and their `libs/{mio,polling}-cellos` (E5); `libs/api/src/services/posix/net.rs` (posix-shim-test; returns `-1`/`NULL`, deviations 2/5); `http-smoke` (deviation 5); net-tools `httpd.rs` (not packaged at `/bin/httpd`, E3).
  - **Ungated**: `cells/tools/cellbox/src/cmd_sys.rs` sends only `GetLocalIp` (D-N4).
  - This is a text census. It does not resolve cfg reachability or unpackaged binaries.

## Known limitations / not exercised (annex §12 and the logs)

1. **No end-to-end transfer.** No end-to-end data transfer was run; smoltcp has no loopback path. curl's admission is inferred from a network failure: `empty response` after `NIC driver reply timeout` (`serial:1558-1559`) and no `HTTP/` line.
2. **curl exit status.** curl exited `stcurl=0` on that failure (`serial:1561`). Its exit status does not encode the failure, so the verdict rests on the UART text checks.
3. **Host-test-only claims.** These are claimed only by the (unsaved) host tests, not observed on the image:
   - socket count, DNS-pending set and L2 TX queue unchanged on denial;
   - denials of `UdpSend`, `L2Recv` and raw TLS connect.

   The runner's own acceptance-limit text (runner 306-309) says the same for the socket count.
4. **Runtime grants not exercised.** nc, httpd, sshd, net-broker and hypervisor grants were not exercised at runtime (host and row tests only). Also not run: GUI 6x103, KVM, the kernel AP lane, and riscv64/aarch64 runtime.
5. **Owner-watch log line, unassessed.** During the curl run the serial log shows `[net] owner death watch unavailable; socket reclaim deferred` (`serial:1557`). This dossier does not assess it. It qualifies the rollback statement "sockets are owner-scoped and reclaimed" (annex §10) for that run.
6. **Coarse rights.** Rights are per Cell and coarse (D-N1): a granted worker reaches any destination or port. Unlisted senders fail closed by design.
7. **No v5-only artifact on disk.** The current signer emits only v6 (S2-T/W). S2-N's v5 decode path is covered by kernel tests `v5_*`/`v4_permit_*`. No v5-only signer artifact exists on disk any more.

## Deviations (annex §12, cross-checked against source)

1. **Legacy raw TLS connect (`0x30`) is gated by `net_connect`.** A denial uses the existing connect-failure reply (`rights.rs` `Denial::RawTlsConnect`; test `raw_tls_connect_is_denied_before_allocating_a_socket`).
2. **The POSIX shim has no errno channel.** A denial is `-1`/`NULL`; no `EACCES`.
3. **The hypha `llm-gateway` edge reuses the reviewed target row** (`profiles.rs:90`).
4. **Non-empty ceilings are refused on the caller-ELF route.** Net workers launch only by kernel path (pre-existing gate, `launch_profile/mod.rs:109`; selftest `selftest.rs:242-246`).
5. **`posix-shim-test` and `http-smoke` have no grant.**
6. **Tests that pinned an empty httpd edge were updated** (`tests.rs`, `selftest.rs:227-246`).

## What CP2 would authorize

The S2-N hunks identified above, at the listed whole-file hashes, would become the S2-N baseline. Later work could then rely on the rights model, `0xFC` and the service-net admission order.

CP2 would **not**:
- approve the S2-T/W, S2-K, S2c, S3, Phase02/03, S4a or S1 hunks in the shared files;
- approve the unattributed `dns.rs`/`dns/tests.rs` changes;
- lift quarantine, authorize canonical packaging, release signing of policy v5/v6 blobs or rollout;
- close criterion 3 on its own (that needs all four S4b CP2s; S4 proposal §5).

## Options

A. **Approve this CP2 baseline** at the exact hashes and evidence scope above.

C. **Not yet**: keep the implementation and evidence, and withhold CP2.

No approval has been recorded.

## Rollout and rollback

**Rollout stays BLOCKED** under the [containment](reports/rollout-containment.md) release conditions. This dossier changes none of them.

Rollback means reverting the S2-N hunks and new files together with the runner/driver/probe checks, then cold-booting a matched previous kernel, image and policy:

- **Kernel and policy roll back together.** A v5/v6 policy blob is not accepted by the `HEAD` kernel: its `cap_bytes_for` knows only v1–v4.
- **S2-T/W must go first or at the same time.** S2-T/W builds on S2-N: v6 extends v5, the `NET_*` row constants use `..LEGACY` as their base, and tests, selftests and the signer interleave both. Reverting S2-N alone would leave S2-T/W code referencing removed fields.
- **The shared files need hunk-level edits.** No S2-N-only snapshot of them exists on disk.

There is no persisted state.

## Addendum (2026-10-22): `[net] owner death watch unavailable` (serial 1557)
**Pre-existing; not an S2-N regression.** The service-net message comes from `cells/services/net/src/service-runtime.rs:642-643` and was already in HEAD; it was introduced by `b0952aa90`. It is printed once, the first time `sys_watch_cell_owner` returns `None`. The kernel only grants owner watches to the **registered VFS Cell**: `kernel/src/task/scheduler.rs:629-634` `watch_live_cell_owner` requires `is_registered_vfs_cell(watcher)`, unchanged from HEAD (line 520 there). The net service therefore never obtains a watch on any image, with or without S2-N. S2-N moved admission ahead of the watch attempt (`service-runtime.rs:613-623`), but that only skips the attempt for denied callers. Admitted callers reach the same refused call.
Consequence (pre-existing): a dead client's sockets are reclaimed only when the net Cell restarts, as the in-code comment at `:628-630` states. This is a separate defect between the kernel and service-net; this CP2 does not fix it or depend on it.
