# Phase 04 S2-N annex — network/DNS/listen/L2 rights enforced by service-net

Status: **TEXT APPROVED by the user on 2026-10-22 (“Duyệt annex + theo khuyến nghị”: D-N1 coarse, D-N2 sandbox no rights, D-N3 lua/python no rights, D-N4 GetLocalIp ungated). Entry checks E1–E6 resolved below (§11). FROZEN; **CP1 APPROVED by the user on 2026-10-22 (“duyệt”) for SHA-256 `d774b9c5386f5d400cf4717c9e518ae4e0836d130d7a3ae71b724df351476255`** (digest of the text before this status line was amended).** No source, ABI, kernel, policy, launch-profile or image change has been made.

## 1. Problem (facts)
- `service-net` authenticates each caller by the kernel trailer `{flags:u32, cell_id, generation, sender_tid}` (`libs/api/src/abi/caller_identity.rs:42-71`) and binds sockets to `(cell_id, generation)` (`cells/services/net/src/service-runtime.rs:541-603`, `socket_table.rs:24-120`). It authorizes no operation class. Any attested caller may `Resolve`, `TcpConnect` anywhere, `UdpCreate/UdpBind/UdpSend`, `TcpListen` on any port, join multicast, and **send and receive raw Ethernet frames** (`L2Send/L2Recv` are unconditional, `cells/services/net/src/handlers.rs:177-243`). The last is the most severe: any Cell with IPC can inject frames onto the NIC.
- The trailer carries no executable, launch-row or registry identity. The only flag is `CALLER_FLAG_VFS_MUTATE = 1<<0` (`caller_identity.rs:45`), derived from allowlist bit 63 (`kernel/src/task/syscall.rs:2886-2905`).
- The syscall allowlist is full: bits 0–62 are assigned, 63 is VfsMutate (`libs/api/src/abi/syscall.rs:496-501,600-607`). The v1 manifest is also full (`kernel/src/task/cap.rs:338-345`). Kernel-only `CapSet` fields requested by install path (`with_path_caps`, `cap.rs:358-405`) and bounded by `∩ ceiling` are the established pattern for authority that has no manifest bit (`pcie_driver`, `serial_port`, `usb_driver`, …).
- Error codes in use (`NetResponse::Err(u8)`, response discriminant 5): `0xFF` generic failure, `0xFE` accept not ready, `0xFD` busy/capacity. L2 deferred replies may forward driver status `1`.

## 2. Model
Coarse **per-Cell rights**. They are requested by install path, bounded by the reviewed launch ceiling, stored in the Cell's kernel `CapSet`, exported by the kernel as trailer flags, and enforced by service-net before any effect. **`NetRequest`/`NetResponse` wire encodings are unchanged.** No new variant and no grant token in request bytes, so a caller cannot forge or replay authority.

| Right | `CapSet` field (new) | Trailer flag (new) | Admits |
|---|---|---|---|
| D (resolve) | `net_resolve: bool` | `CALLER_FLAG_NET_RESOLVE = 1<<1` | `Resolve` (9) |
| N (egress) | `net_connect: bool` | `CALLER_FLAG_NET_CONNECT = 1<<2` | `TcpConnect` (0), `UdpCreate` (6), `UdpSend` (7) |
| Nl (inbound) | `net_listen: bool` | `CALLER_FLAG_NET_LISTEN = 1<<3` | `TcpListen` (4), `UdpBind` (11), `MulticastJoin` (13) |
| L2 (raw frames) | `net_l2: bool` | `CALLER_FLAG_NET_L2 = 1<<4` | `L2Send` (15), `L2Recv` (16) |

The numbers in parentheses are `NetRequest` discriminants. Flag bits 5..31 remain reserved and must be zero.

Not gated, because they act only on a socket the caller already owns, and owning one required a gated request: `TcpSend/Recv/Close` (1–3), `TcpAccept` (5, on an owned listener), `UdpRecv` (8), `SocketState` (10), `MulticastLeave` (14), `TcpReady` (17), `TcpRecvReady/SendReady/CloseGraceful` (18–20). `GetLocalIp` (12) stays ungated (decision D-N4).

**Denial:** new `NetResponse::Err(0xFC)` = `Denied`. It is returned before any allocation, socket creation, DNS query/pending record, frame transmission or owner-watch install, and it is distinct from `0xFF`, so clients report "permission denied" rather than a network failure. `0xFC` is newly allocated. Existing codes keep their meanings.

## 3. Kernel delta
1. `kernel/src/task/cap.rs`: add the four fields to `CapSet`, `EMPTY`, `from_manifest` (always false), `intersect` (AND), `without_ptrust` (unchanged: these are not P-TRUST), and `with_path_caps` rows per §5.
2. `kernel/src/task/syscall.rs` `attested_identity_of`: OR the four flags from the sender's **Cell** CapSet, the same CapSet that spawn intersected. Threads inherit their Cell's rights. A dead or unattributable sender stays `None` (deny).
3. `kernel/src/policy.rs` + `scripts/sign-policy.py`: signed-policy CapSet encoding **v5** = version byte `5`, `CAP_BYTES_V5 = 15`, i.e. v4's 11 bytes followed by `net_resolve, net_connect, net_listen, net_l2`, each validated as literal `0|1` (anything else makes the policy malformed, as for v2–v4 privileged bytes). **Compatibility rule (E1):** v1–v4 decoding sets the absent net fields to **true** in the decoded `Permit` ceiling, so an older entry does not narrow them and the request ∩ reviewed edge ceiling stays the bound. This preserves today's behavior for unchanged policies without widening past the edge ceiling. It deliberately differs from the P-TRUST fields (absent = false): net rights existed ambiently before this slice, and false would silently strip every legacy Permit. The signer gets the v5 row schema, encoder and its independent decoder.
4. `kernel/src/loader/launch_profile/targets.rs`: rows per §5 carry the rights in their ceiling. `boot_ceiling.rs` rows for boot-started services (`sshd`, `net-broker`, `hypervisor`) likewise. `kernel/src/audit.rs` records the four bits.
5. Launcher edge (E2, resolved): child CapSet = path request ∩ reviewed target ceiling, then policy. It is not intersected with cellbox's own CapSet (`launch_profile/mod.rs:27-115`, `governed_spawn.rs:125-145`). Cellbox therefore holds no net right and can still launch granted workers. Boot-started Cells use `Spawner::Root` ∩ `boot_ceiling(path)` (`boot_ceiling.rs:44-169`).

## 4. Service-net delta
- `cells/services/net/src/service-runtime.rs`: after attestation (`:541-570`), classify the decoded request per §2. On a missing flag, reply `Err(0xFC)` and skip dispatch. This covers both early intercepts (`Resolve`, `TcpReady`) and `handlers::dispatch`.
- One pure `fn required_right(&NetRequest) -> Option<Right>` in a new `cells/services/net/src/rights.rs`, with an exhaustive `match` (no `_` arm), so a future variant cannot compile without a classification.
- `libs/api/src/abi/caller_identity.rs`: four constants, plus a `NET_RIGHTS_MASK` used to reject unknown bits in host tests.
- `libs/api/src/services/ipc.rs`: doc comment for `0xFC` only, no wire change. `libs/ostd/src/clients/net.rs` and `libs/posix/src/net.rs`: map `0xFC` to `ViError::PermissionDenied` and `EACCES`.

## 5. Grant table (complete census; anything absent gets no rights)

| Install path | D | N | Nl | L2 | Senders observed |
|---|---|---|---|---|---|
| `/bin/curl`, `/bin/wget`, `/bin/mqtt` | ✓ | ✓ | | | `cells/tools/net-tools/src/bin/{curl,wget,mqtt}.rs` |
| `/bin/nc` | ✓ | ✓ | ✓ | | client + `-l` listener |
| `/bin/httpd` | | | ✓ | | net-tools `httpd.rs:303-328` and `cells/services/httpd/src/reactor.rs:277-568`: one path, both listen-only. **E3:** confirm which binary owns `/bin/httpd` in each image |
| `/bin/sshd` | | | ✓ | | `cells/services/sshd/src/main.rs:50-159` |
| `/bin/net-broker` | ✓ | ✓ | ✓ | | beacon/multicast, STUN, C2C TCP (`cells/services/net-broker/src/*`) |
| `/bin/hypervisor` | | | | ✓ | `net_backend.rs:109-112,257-260` |
| `/bin/ocel` (+ `ocel-js`, `ocel-quickjs` if they fetch; **E4**) | ✓ | ✓ | | | `cells/apps/ocel/src/net/http.rs:405-587` |
| `/bin/llm-gateway`, `/bin/https-demo`, `/bin/robot-demo` | ✓ | ✓ | | | `hypha/llm-gateway/src/transport.rs`, robot `mqtt.rs` |
| `/bin/async-echo`, `/bin/tokio-smoke` (demos; only if reviewed rows exist, **E5**) | | | ✓ | | demo listeners |
| `/bin/lua`, `/bin/python` | decision D-N3 | | | | `cells/runtimes/lua/src/bindings_net.rs:105-241` |
| `/bin/cellbox` | | | | | none; `ifconfig/ip` use ungated `GetLocalIp` |
| `/apps/*`, `/data/bin/*`, `/home/*` sandbox default | decision D-N2 | | | | `targets.rs` default arm |

## 6. Effects on cellbox and criterion 3
The shell keeps no network right. Network effects happen only in retained workers launched on reviewed edges whose ceiling carries the right. This makes the S2-lite "retained worker" boundary true at the service instead of nominal. Criterion 3's network/DNS row becomes provable: a Cell with cellbox's rights is refused D/N/Nl/L2 by `0xFC`, with no socket, DNS query or frame produced. Destination/port scoping is **not** provided by V1 (D-N1).

## 7. Verification plan
- **Host:** `service-net` tests drive the real classifier and runtime admission. For each gated variant: missing flag gives `Err(0xFC)`; the socket table, DNS pending set and L2 TX queue are unchanged (counts/state, not mocks); flag present proceeds to existing behavior. Every ungated variant on a foreign or absent cap still gives its existing error. Kernel host tests: `intersect`, `from_manifest` = false, and v5 encode/decode round trip, including that a v4 Permit leaves net rights unnarrowed and malformed net bytes reject the policy. Signer test: `sign-policy.py` v5 output decodes identically in the kernel decoder.
- **Private image** (E6): the terminal runner boots with `-nic none` and packages no `/bin/net`. S2-N adds `/bin/net` + `/bin/e1000` + `/bin/curl` and boots with `-netdev user,id=net0,restrict=on -device e1000,netdev=net0`: SLIRP with no host forward and no outside access. A fixture-only `net-probe` mode of `cells/tests/phase03-filter` with no rights gets `0xFC` for Resolve/TcpConnect/UdpCreate/UdpBind/TcpListen/MulticastJoin/L2Send, with the net socket count unchanged. `curl http://10.0.2.2:9/` through its reviewed edge must fail with a **network** error (refused/timeout), not `0xFC`, which proves admission without traffic leaving the VM. A guest-local `httpd`↔`curl` pair is **not** claimed: smoltcp has no loopback path (`handlers/tcp.rs:27-47`, `interface.rs:266-293`). Full end-to-end data transfer stays unexercised.
- **Regression:** S3 terminal (327), Phase03 (60), Phase02 runners; kernel AP lane; existing net-service host tests; Ocel mock tests.

## 8. Risks
- **Breaking any unlisted sender** (an unknown path or a third-party app under `/apps`). This is intentional fail-closed behavior, made visible by `0xFC`. The census in §5 must be re-run at implementation start and any new sender added to the table through this annex.
- The policy v5 encoding is a signed-artifact format change. Old policies must still verify (E1). Release signing remains blocked by rollout containment.
- Rights are per Cell, not per invocation. A granted worker can still reach any destination; that is V1's stated ceiling.

## 9. Decisions requested (with recommendations)
- **D-N1 Scope:** *coarse V1 as above* (recommended), or V1 plus a per-row listen-port allowlist (e.g. httpd 80/8080, sshd 22). The port option adds a `u16` set to CapSet and policy v5 and is roughly 2× the kernel delta.
- **D-N2 Sandbox default (`/apps`, `/data/bin`, `/home`):** *no network rights* (recommended; matches "zero ambient authority" in the row's own comment), or D+N by default.
- **D-N3 `/bin/lua`, `/bin/python`:** *no rights* (recommended; scripts reach the network through a granted worker), or D+N to keep current Lua networking. Choosing "no rights" breaks `bindings_net.rs` callers until a worker exists.
- **D-N4 `GetLocalIp`:** *ungated* (recommended; it discloses only the local address, which `ifconfig` needs), or require D.

## 10. Rollback
Revert the files in §3–§4 and the runner checks, then cold-boot a matched previous image and policy. There is no persisted state; sockets are owner-scoped and reclaimed. Rollout, S4 and CP2 remain blocked.

## 11. Entry checks (2026-10-22, read-only)
- **E1:** policy v1–v4 carry 6/9/10/11 cap bytes, and absent later fields decode to false; spawn order is request → `with_path_caps` → ceiling → policy Permit (`policy.rs:32-68,278-329,641-676`, `governed_spawn.rs:120-154`). Hence the compatibility rule in §3.3.
- **E2:** resolved as in §3.5; the slice is not blocked.
- **E3:** images install **`service-httpd`** at `/bin/httpd` (`scripts/gen-disk-ci.sh:155-160,407-410`). Net-tools' `httpd` binary is not packaged at that path. The §5 row covers the installed binary, and both are listen-only.
- **E4:** only `/bin/ocel` sends NetRequest (`ocel/src/net/http.rs:405-413`). `ocel-js`, `ocel-quickjs`, `chrome`, `files` and `desktop` get no rights.
- **E5:** `/bin/async-echo` and `/bin/tokio-smoke` have no reviewed rows and stay without rights (no row added by this slice). Boot rows today: `sshd` EMPTY, `net-broker` `network`, `hypervisor` `hypervisor`. The §5 net rights are added to those boot rows.
- **E6:** as in §7.

## 12. Results (2026-10-22)
- **Implemented** per §2–§5. Kernel: `CapSet` net fields, `with_path_net_rights`, trailer flags in `attested_identity_of`, policy v5 plus `scripts/sign-policy.py`, target/boot rows, audit `NetRightsGranted = 27`. Service: `cells/services/net/src/rights.rs` exhaustive classifier; admission runs before the owner watch, intercepts and dispatch. API: `CALLER_FLAG_NET_*`, `NET_ERR_DENIED = 0xFC`. `ostd` maps it to `PermissionDenied`. curl/wget/mqtt/nc/ocel print an explicit permission message.
- **Deviations:**
  1. Legacy raw TLS connect (opcode `0x30`) is also gated by `net_connect`. Its reply format has no error code, so a denial returns the existing connect-failure reply.
  2. The POSIX shim (`libs/api/src/services/posix/net.rs`) has no errno channel, so a denial is `-1`/`NULL` like any other failure; no `EACCES`.
  3. The hypha `llm-gateway` edge in `profiles.rs` reuses the reviewed target row.
  4. A non-empty ceiling is refused on the caller-ELF (`SpawnFromElf`) route, so net workers launch only through the kernel path route; VFS-only copies get `PermissionDenied` at spawn.
  5. `posix-shim-test` and `http-smoke` have no grant (fail-closed).
  6. Existing tests that pinned an empty httpd edge were updated.
- **Verified:**
  - Host (agent runs): service-net 56 (13 new), api 129, ocel 172, net-tools 11, ostd net 1; kernel cap/policy tests and a four-target check.
  - Private terminal runner exit 0: **342 checks / 77 driver markers**, under restricted SLIRP with no forward.
  - `net-denied-{resolve,tcp-connect,udp-create,udp-bind,tcp-listen,multicast-join,l2-send}` come from a rights-free probe on the cellbox edge.
  - `net-curl-admitted`: curl on its reviewed edge ends in a network failure ("empty response"), not a refusal. No `HTTP/` and no probe bytes on the UART.
  - Regressions: Phase03 60/60, Phase02 exit 0.
- **Not exercised:** end-to-end data transfer (no loopback); socket count inside the guest (host tests only); GUI 6x103; KVM; and nc/httpd/sshd/net-broker/hypervisor grants at runtime (host and row tests only).
- **Criterion 3:** the network/DNS row is proven for the shell edge. T and W remain open.
