Confirmed: the tracked `tools/hypha-mock-llm/ca-key.pem` is the private key of the exact certificate embedded as the default trust anchor (same serial). I have all slices; final report follows.

## Slice: NetStack (net service, net-broker, httpd, sshd, http-core, vissh, NIC drivers) — files read: ~110 (net 28, http-core/httpd 10, sshd/vissh 12, broker 48, drivers 24) — unsafe blocks reviewed: 51 (net service 9 incl. 3 `unsafe fn`; drivers 42; sshd/vissh/broker/httpd/http-core 0 — all `#![forbid(unsafe_code)]`)

### Findings

#### [S-1] sshd: USERAUTH always succeeds and channel/exec is not gated on auth phase — Severity: Critical — STRIDE: S/E — OWASP: A07
- Where: cells/services/sshd/src/session.rs:147-156, 171-201 (read); 202-266 [per SshAudit]
- What: `SSH_MSG_USERAUTH_REQUEST` discards user/service/method and replies `USERAUTH_SUCCESS` unconditionally (session.rs:191-201, comment "Instant authenticated access"). `ServiceRequest | UserAuth | ActiveSession` all funnel into `handle_authenticated_packet`, which never checks `self.phase`, so `CHANNEL_OPEN`/`exec` are honoured directly after NEWKEYS. No credential store, no attempt limit.
- Exploit: Any network peer that completes KEX gets the built-in shell (currently canned commands + `ps` via `sys_get_procs`); becomes unauthenticated RCE the moment a real shell/exec is wired. Production path (not test).
- Fix: Enforce phase per message type; implement real `publickey` auth (signature over `session_id‖USERAUTH_REQUEST…`, construction exists in libs/vissh/src/userauth.rs:35-57) against an authorized-keys store; `USERAUTH_FAILURE` + attempt cap + disconnect.

#### [S-2] sshd: hardcoded x25519 "ephemeral" secret and hardcoded Ed25519 host-key seed — Severity: Critical — STRIDE: I/S — OWASP: A02
- Where: cells/services/sshd/src/kex.rs:29-36 (host seed `"CellosSSHDevHostKeySeed20261017\x01"`), 63-66 (constant KEXINIT cookie), 95-102 (constant `StaticSecret`), 138 (sign H)
- What: The server's DH private scalar is a compile-time constant and `Q_S` is identical on every connection; the host key derives from a constant seed. sshd has no RNG dependency at all. Session keys are a deterministic function of public wire data; the host key is reproducible by anyone with the repo.
- Exploit: Passive pcap observer computes `K = x25519(const, Q_C)`, recomputes `H`, derives chacha20-poly1305 keys → decrypts/forges both directions; any party can impersonate every Cellos sshd to pinning clients. No `was_contributory()` check (kex.rs:101-102) so a small-order `Q_C` additionally forces K=0.
- Fix: Per-KEX ephemeral from kernel entropy (fail closed like libs/vissh/src/lib.rs:21-25), host key generated once and persisted via KMS/authority, random cookie, reject non-contributory DH.

#### [S-3] Private key of the default embedded TLS trust anchor is committed to git — Severity: High — STRIDE: S — OWASP: A02
- Where: cells/services/net/roots/private.der (X.509 CA cert, CN=Cellos Dev CA, serial 45552E70…B93F — verified with openssl, **not** a private key); tools/hypha-mock-llm/ca-key.pem (tracked, `BEGIN PRIVATE KEY`, same serial via ca.pem); tools/hypha-mock-llm/.gitignore:4 ("intentionally tracked"); cells/services/net/Cargo.toml:19,47 (`tls-ca-private` is the default feature); cells/services/net/src/tls/roots.rs:70-71,103-105
- What: Every default-feature `service-net` build pins exactly one CA, and that CA's private key is public in the repo. Anyone can mint a server certificate for any hostname that the fleet build will accept (embedded-tls `CertVerifier` does chain + hostname matching, pki.rs:109-128). Today TLS in the fleet flavor is fail-closed because `clock::observe()` returns `None` (tls/clock.rs:19-28; gate at tls/socket.rs:36-43 and tls_handler.rs:122-126), so this is latent — it becomes a live MITM the moment authenticated time ships unless the anchor is replaced.
- Exploit: Attacker on path issues `CN=<any host>` signed by ca-key.pem; fleet net service accepts.
- Fix: Remove `ca-key.pem`/`ca.pem` from git, rotate the dev CA, make `tls-ca-private` refuse to build (or warn loudly) when `private.der` serial equals the known dev serial; require an operator-supplied anchor for release builds.

#### [S-4] NIC Driver Cells accept TX/RX/GetMac from any IPC sender — Severity: High — STRIDE: S/I/D — OWASP: A01
- Where: cells/drivers/virtio-net/src/main.rs:69-93 (read: `sender_tid` used only for reply); cells/drivers/e1000/src/main.rs:103-127, cells/drivers/igb/src/main.rs:167-191 [per NicDriverAudit]; net service discovers the driver via lookup at cells/services/net/src/interface.rs:98-103
- What: No sender check before `handle(dev, data…)`, unlike block drivers which gate on the VFS TID. Any Cell that can look up `service::NIC_DRIVER` can inject raw Ethernet frames (bypassing the net service's `NET_L2` right at rights.rs:60) and pop RX frames (`[OP_RX]`), stealing inbound traffic from the net service. Dispatch is serialized under `STATE.lock()` so a TX flood also stalls the net service for the driver's TX wait.
- Exploit: Untrusted Cell with base Send/LookupService sends `[0x00, len_le16, frame]` (ARP/DHCP spoofing, MAC spoof) or spams `[0x01]` (plaintext disclosure + stack starvation).
- Fix: Resolve `sys_lookup_service(service::NET)` once and drop requests from any other `sender_tid` (re-resolve on mismatch), mirroring the block-driver pattern.

#### [S-5] smoltcp `random_seed` never set → predictable TCP ISNs, DNS transaction IDs/source ports, IP IDs — Severity: High — STRIDE: S/T — OWASP: A02
- Where: cells/services/net/src/service-runtime.rs:626-628 (`Config::new(HardwareAddress::Ethernet(MAC))`, no `random_seed`); smoltcp-0.11.0/src/iface/interface/mod.rs:147,172 (`random_seed: 0` → `Rand::new(0)`); smoltcp-0.11.0/src/socket/tcp.rs:884 (ISN from `cx.rand()`); smoltcp-0.11.0/src/socket/dns.rs:294-295 (txid + source port from `cx.rand()`); sequential ephemeral ports at service-runtime.rs:92-102
- What: The interface RNG is seeded with the constant 0 on every boot, so the sequence of ISNs, DNS txids and DNS source ports is identical and predictable; connect-side source ports are a simple counter from 49152. A real entropy source exists in the cell (`sys_get_random`, tls/rng.rs:31) but is not used for the stack.
- Exploit: Off-path attacker predicts ISN → spoofed-SYN connections to Cell listeners, RST/data injection into any Cell's TCP session (e.g. broker/httpd); DNS cache-poisoning (Kaminsky-style) with no port/txid entropy, redirecting `NetRequest::Resolve` for every Cell.
- Fix: `config.random_seed = u64 from sys_get_random` (fail closed if absent); randomize `next_ephemeral_port` from the same source.

#### [S-6] httpd exposes the entire VFS root and unauthenticated control/AI endpoints to the network — Severity: Medium — STRIDE: I/E — OWASP: A01
- Where: cells/services/httpd/src/router.rs:45-69 (`/files/<p>` → `Route::File("/<p>")`, `/api/files?path=` → `ListDir(path)` unvalidated, `POST /api/cells/*/restart`, `POST /api/infer`); reactor.rs:104-127 (components resolved from "/" with no web-root prefix), 136-166, 764-782, 1012-1051; handlers.rs:238-247, 308-311
- What: There is no document root: `/files/x/y` opens `/x/y` from the VFS root the httpd cell holds; `/api/files?path=…` passes the raw query string to `VfsRequest::ListDir`. No authentication on any route; `/api/infer` drives the LLM service for any client; `/api/cells/*/restart` is currently a stub (returns `restart not yet wired`) but is reachable unauthenticated. `.`/`..`/separators are rejected (libs/api/src/services/dir_name.rs:67-89) and `%`-sequences are not decoded, so traversal *below* root is not possible — the exposure is the root itself.
- Exploit: `GET /files/etc/cellos/cluster.key` (or any file the httpd VFS cap can read, e.g. the broker's K1 path per BrokerAudit) from any LAN/Internet client.
- Fix: Prefix a fixed web root (e.g. `/www`) in `FileSession::new` and `Route::Files`; validate the listing path with `validate_dir_path`; put infer/restart behind an auth token or drop them from the public listener.

#### [S-7] hypervisor-bridge: guest can claim the host MAC via source-MAC learning and divert all host RX — Severity: Medium — STRIDE: S/I/D — OWASP: A01
- Where: cells/services/net/src/interface.rs:346-356 (learn from any `L2Send` frame's src), 608-615 (frames whose dst == `guest_mac` go only to `guest_rx_queue`); handlers.rs:211-221; service-runtime.rs:28 (host MAC is the fixed, public `52:54:00:12:34:56`)
- What: `learn_guest_mac_from` overwrites `guest_mac` with the Ethernet source of any guest-originated frame, only rejecting zero/multicast. A Tier-3 guest (untrusted by the security model) that sends one frame with src = host MAC makes `route_rx` queue every unicast frame addressed to the host into the guest queue instead of smoltcp.
- Exploit: Guest `ip link set dev eth0 address 52:54:00:12:34:56` + any packet → host stack (and every Cell's TCP/DNS/DHCP) stops receiving; the guest reads the host's inbound plaintext. Requires a `hypervisor-bridge` build. [INFERENCE: hypervisor forwards guest TX frames verbatim via `L2Send`.]
- Fix: Refuse to learn a MAC equal to the interface's own hardware address (and optionally restrict learning to the first observed address / hypervisor-registered one); route host-MAC frames to smoltcp unconditionally.

#### [S-8] Cross-Cell listen-port squatting: any `NET_LISTEN` Cell may listen on a port another Cell already serves — Severity: Medium — STRIDE: S — OWASP: A01
- Where: cells/services/net/src/handlers/tcp.rs:128-149 (`TcpListen` → `make_tcp` + `listen(port)` with no port-ownership check); 177-202 (`TcpAccept` adds the replacement listener *after* the first is Established); rights.rs:57-59
- What: The socket table binds caps to owners but nothing binds ports to owners; smoltcp's `listen` does not detect duplicates across sockets. Between a SYN being accepted on httpd's listener and its replacement being added, a second Cell's listener on the same port is the only one in `Listen` state [INFERENCE: smoltcp dispatches an incoming SYN to the first socket that `accepts()` it], so it receives the connection.
- Exploit: Cell with `NET_LISTEN` right calls `TcpListen{port:8080}` and races httpd for inbound connections (phishing/credential capture on the LAN-facing port).
- Fix: Keep a `port → owner` map in `SocketTable`; reject `TcpListen`/`UdpBind` on a port held by a different live owner.

#### [S-9] Net service: unbounded per-cap TLS request queue and unbounded legacy-reply ledger — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/net/src/tls/dispatch.rs:140-175 (`op.queue.push_back` unbounded; foreign/unknown cap → `answer(...)` at 153/170/173), 180-200 (`replies.push_back` with 30 s `REPLY_TIMEOUT`), 202-222 (retry loop re-pushes until deadline); tls_wire.rs:77-106 (Send ≤ 501 B, no cap on count); tls_handler.rs:153-180 (`Send`/`Recv` ungated by rights, rights.rs:83-85)
- What: Every raw TLS `Send` for a cap the caller does not own still appends a `LegacyReply` Vec held up to 30 s if the sender never does a matching receive; an owner can also queue unlimited `Request::Send(Vec)` behind one in-flight operation. Nothing bounds either container.
- Exploit: Any Cell loops `TLS_SEND_OP` with random caps (or its own busy cap) without receiving → heap growth in the net service → allocator failure kills networking for all Cells.
- Fix: Bound `queue` (e.g. 4) and `replies` (e.g. 64) and answer immediately with failure when full; drop legacy replies for callers not in `Recv` instead of retaining them.

#### [S-10] virtio-net (MMIO/aarch64/riscv64) RX trusts device-controlled used `id`/`len` → index panic — Severity: Medium — STRIDE: D — OWASP: A08
- Where: cells/drivers/virtio-net/src/device.rs:60-73 (read: `rx.packet()[..len]` with `len = packet_len().min(buf.len())`); kernel/third_party/virtio_drivers/src/device/net/dev.rs:78-92 (read: `self.rx_buffers[token as usize]` unchecked); net_buf.rs:75-77, queue.rs:514-549 [per NicDriverAudit]
- What: The vendored `receive()` indexes the RX buffer array with the raw used-ring id and stores the device-reported `len-10` as `packet_len`; `packet()` slices `buf[10..10+packet_len]` so a `len` > 2048 or `id` ≥ queue size panics the only NIC cell. The x86 path (pci_x86/raw_net.rs:149-171) bounds both correctly.
- Exploit: Hostile/buggy virtio backend writes `id=0xFFFF` or `len=0x10000` into the used ring → driver abort → network loss.
- Fix: `get(usize::from(token))` and clamp `pkt_len` to `rx_buf.len() - NET_HDR_SIZE` in the vendored `receive`, or clamp in `device.rs::try_recv` before slicing.

#### [S-11] sshd: unbounded version-line / line buffers and single-connection reactor with no deadlines — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/sshd/src/session.rs:63-85, 283-293; cells/services/sshd/src/main.rs:24, 69-70, 79-167 [per SshAudit, main.rs not read by me]
- What: Pre-handshake `rx_buf` grows until a `\n` arrives (no 255-byte cap); each inbound printable byte is appended to `line_buffer` and echoed as its own packet; one `active_client` slot with no handshake/idle timeout.
- Exploit: One idle TCP connection to :22 locks everyone out; a newline-less stream OOMs the 6 MiB cell heap.
- Fix: Cap version line at 255 B, cap `line_buffer`, add per-session deadlines, coalesce echo.

#### [S-12] net-broker (prod path): beacon `boot_epoch` is uptime-ms, so a rebooted peer is ignored until uptime exceeds its previous value — Severity: Medium — STRIDE: D — OWASP: A04
- Where: cells/services/net-broker/src/local_runtime.rs:74,86,100-105; beacon.rs:410-412,445-460 [per BrokerAudit]
- What: Freshness check `boot_epoch > last || (== && counter > last)` rejects authenticated beacons from a peer whose new (smaller) uptime epoch follows a reboot; the peer is reported lost for as long as its prior uptime.
- Exploit: Crash/restart any member (including via S-9-style resource kills) → long partition.
- Fix: Persisted monotonic boot counter, or accept `boot_epoch != last && counter == 0` as an authenticated rebase.

#### [S-13] net-broker (unwired prototypes): framing desync, cap leaks, unbounded dedup pinning, unauthenticated gossip claims, tick/ms unit bug — Severity: Medium — STRIDE: D/S/T — OWASP: A04/A01
- Where: transport/tcp_framing.rs:29-60 (single non-looping `TcpRecv`, truncation at 58-60; records > 3840 B always truncated vs noise_session.rs:120,130); connection_manager.rs:54-56,79-85,105-128 (no `TcpClose` on error paths, `find_session` returns `None`); c2c_ingress.rs:335-349 + c2c_dedup.rs:82-89,175-184 (`Dispatched` entries never evicted); gossip.rs:90-103,144-192 + lease.rs:99-122 (`machine_id`/`epoch` from wire, not from authenticated session); lease.rs:26-70, enrollment.rs:170,206, gossip.rs:63-145 (`sys_get_time()` raw ticks vs `*_MS` constants) [all per BrokerAudit]
- What: `main.rs:139-155` prints "remote disabled" and no listen/accept path exists, so none of these are reachable today; they will be live when the dispatch loop is wired.
- Exploit: Peer segments a frame → responder desyncs; unresponsive peer → ≤18 leaked sockets exhausts the net budget; 16 dropped dispatches → every request from every peer gets `Busy`; any enrolled peer forges `TaskClaim{machine_id=victim, epoch=MAX}`.
- Fix: Loop reads to exact length; close caps on all error paths; evict/TTL `Dispatched`; reject `machine_id != derive(session.peer_node_id)`; use `sys_get_time_ms()`.

#### [S-14] Multicast membership not bound to caller: `MulticastJoin` ignores `cap_id`, `MulticastLeave` needs no right — Severity: Low — STRIDE: D — OWASP: A01
- Where: cells/services/net/src/handlers.rs:190-201 (`cap_id: _`, interface-wide join/leave); rights.rs:59 (Join needs Listen), 68 (Leave ungated)
- What: Group membership is per-interface and unaccounted; any Cell (no right required) can `MulticastLeave` the broker's beacon group, silently dropping its peer discovery (beacon RX).
- Fix: Reference-count groups per owner; require `NET_LISTEN` for Leave and only drop the caller's own membership.

#### [S-15] e1000/igb: promiscuous RX, un-zeroed RX pages returned up to device-reported length, TX slot reuse after timeout — Severity: Low — STRIDE: I/- — OWASP: A05
- Where: e1000/src/controller.rs:255,270-313,320-353; igb/src/controller.rs:364-366,389-432,438-476 [per NicDriverAudit]
- What: `RCTL_UPE|RCTL_MPE` accepts all LAN unicast/multicast; RX buffers are never zeroed and descriptor error/EOP bits are ignored, so an over-reported `length` returns stale bytes (copy is bounded, no overflow); a DD-poll timeout leaves a device-owned TX slot that is overwritten 16 sends later.
- Fix: Drop UPE/MPE; zero RX pages; honor error/EOP bits; check DD before slot reuse.

#### [S-16] Hygiene: hardcoded interface MAC; dev-only TLS RNG panic; httpd JSON built by string interpolation; `dev-weak-rng` undetectable by broker; `K1 XOR const` gossip key; `restart-oracle` unguarded — Severity: Low/Info — STRIDE: - — OWASP: A05
- Where: service-runtime.rs:28 (`52:54:00:12:34:56` for every node → duplicate MACs on one LAN; not derived from the NIC's `GetMac`); tls/rng.rs:36-40 (`assert!` → cell abort when virtio-rng absent; reachable only in `qemu-dev-https` builds because fleet builds refuse TLS first); httpd/src/handlers.rs:238-247 (`path`/names interpolated unescaped into JSON — invalid JSON, not XSS, content-type is application/json); net-broker rng.rs:25-46 vs kernel `dev-weak-rng` fallback, beacon.rs:53-61, restart_oracle.rs:13-25 [per BrokerAudit]
- Fix: Use driver-reported MAC; make RNG absence a refused handshake, not a panic; JSON-escape; one-way KDF; gate restart oracle on a cell id.

### Verified-safe areas
- Net service IPC authority: kernel-attested `CallerIdentity` with non-zero cell/generation required and rights admitted before any effect (service-runtime.rs:782-820; rights.rs:125-140 mirrors dispatch exactly); every socket op checks `SocketTable::is_owner` (socket_table.rs:122-170, 297-303); readiness validates every cap before emitting events (handlers/tcp.rs:338-362); per-owner socket budget (socket_table.rs:69-78).
- Driver↔net framing: RX reply length bounded `14..=1514` into a `MAX_FRAME+2` buffer, reply sender must equal driver TID, buffer zeroed between commands (interface.rs:454-461, 478-490, 520-595); TX ≤ 1514 (389-392, 529-535); queue limits 32 (396-408, 576-579); driver side validates TX header length vs buffer (virtio-net dispatch.rs:53-61 etc.).
- Raw TLS wire parser: all lengths checked before slicing, hostname ≤ 495 UTF-8, send ≤ 501, recv clamped (tls_wire.rs:43-109); TLS verifier is infallible (`verifier()` always `Ok`, provider.rs:75-77) with hostname verification mandatory (provider.rs:48-54, socket.rs:106-108); fleet TLS fail-closed without authenticated time (clock.rs:14-28, socket.rs:36-43); qemu-dev `PublicVerifier` validates chain at both window ends, EKU server_auth, hostname, and CertificateVerify over the transcript (public_pki.rs:55-100, 106-165); TLS RNG is ChaCha20 seeded from `sys_get_random` (rng.rs:20-48).
- TLS unsafe: record buffers are stable `Box` allocations, `conn` is declared/dropped before `_buffers` (socket.rs:51-56, 91-97, 144-147); context pointers installed only for one synchronous `poll` and cleared after (dispatch.rs:524-528, transport.rs:20-45, 77-88); `Close` cancels the pending future before `sockets.remove` (tls_handler.rs:88-96).
- httpd: request ≤ 4096 B with 413 (net_ipc.rs:53, reactor.rs:922-940); `Transfer-Encoding` rejected and duplicate `Content-Length` rejected → no request smuggling (net_ipc.rs:69-87); `checked_add` on length; 16-header cap; `..`/`.`/separators/control bytes rejected per component (dir_name.rs:67-89, reactor.rs:105-116); connection/backend caps and read/send/backend/close deadlines (reactor.rs:19-28); `max_tokens` clamped 1..64 (router.rs:60-63); prompt ≤ `MAX_PROMPT_BYTES` (handlers.rs:250-258).
- http-core (client): httparse with 32-header cap, chunked `Transfer-Encoding` precedence, chunk size ≤ 16 MiB with bounded 32-byte size-line accumulator, EOF mid-body surfaced as error not spin (response.rs:93-151, body.rs:43-50, body_chunked.rs:33-117, 141-191).
- UDP: recv bounded to 512 B into a fresh Vec; send errors on oversize instead of truncating (handlers/udp.rs:75-131). DNS: 32-slot query table with 30 s deadline and `swap_remove` cursor handled correctly (service-runtime.rs:147-241).
- sshd packet layer: length/padding underflow guarded, no allocation from untrusted length before full packet + tag present, Poly1305 verified before decrypt, chacha20-poly1305@openssh.com construction correct (cipher.rs:159-163, 181-230, ssh_packet.rs:111-149) [per SshAudit]; vissh client accepts only `KnownExact` host keys before any credential, zeroizes password/channel data, bounds inbound queue and enforces rekey/KEX deadlines (vissh/src/session.rs:164-170, 376-407, 445-470, 714-759) [per SshAudit].
- net-broker: Noise `KKpsk0` with both statics pre-supplied + K1 PSK + role-ordered prologue (noise_session.rs:55-66, noise_identity.rs:11-27); all-zero DH rejected (kms_dh.rs:62-71, 153-158); fixed 112-B envelope with exact-length and bounded payload checks (c2c_envelope.rs:107-163); beacon AEAD with random 24-B nonce, exact 80-B frame, cluster + machine-id allow-list (beacon.rs:85-97, 401-406); bounded config/registry parsers with checked arithmetic [per BrokerAudit].
- NIC drivers (x86 virtio, e1000, igb): used-len `saturating_sub(12).min(out.len())` with token bounds check (raw_net.rs:141-172); bounce-pool `unshare` validates paddr; DMA grants authorized before any device-visible address write; volatile descriptor writes + `compiler_fence(Release)` before doorbell; ring indices masked `% N` [per NicDriverAudit].
- kernel/src/task/net_rx_selftest.rs is boot-time selftest only; it asserts NET_RX reservation/latch/abandon/`Completing`-retry invariants (:52-266) and never touches driver buffers [per NicDriverAudit].

### Not covered
- cells/services/net: `tls/qemu_time.rs` (288 lines, qemu-dev only), `tls/relay_certificate.rs`/`relay_profile.rs` (`#[cfg(test)]`), `service-runtime.rs:334-590` (graceful-close/abort reaper bodies), `main.rs`, `build.rs`, all `*tests*.rs`.
- httpd `reactor.rs` bodies outside 1-136, 732-840, 922-940, 997-1090; `handlers.rs` HTML page bodies (index/status) not checked for template injection (they take no request input per the signatures read).
- third_party/sunset (vissh's actual packet/KEX/signature code), `cells/services/sshd/src/main.rs` (read only via SshAudit), `libs/vissh/tests/*` (test-only).
- net-broker `#[cfg(test)]` modules and the `clatter`/x25519-dalek internals; libs/virtio-pci-transport; kernel GrantDma/IOMMU enforcement and MMIO-window user accessibility (basis of driver inferences).
- smoltcp internals beyond the three cited lines (ISN/txid/port derivation, listener dispatch order marked [INFERENCE]).