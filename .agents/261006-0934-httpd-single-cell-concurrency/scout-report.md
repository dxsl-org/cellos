# HTTPD concurrency — scoped recon

## Project and architecture
Rust 2021, no_std cells, Cargo workspace, smoltcp network service; mixed event-driven service loops and synchronous capability IPC. HTTP parsing uses httparse. No implementation changed during this planning session.

## Verified source findings
- `cells/services/httpd/src/main.rs:88-108`: accept → whole handler → 200 yields → close; one connection at a time. Comment claiming accept blocks until connection is inaccurate: net returns not-ready immediately, although the IPC exchange blocks the caller.
- `httpd/src/net_ipc.rs:14-21,48-72,149-181`: blocking typed IPC, whole-response send loop, receive loop bounded by 200 iterations rather than elapsed time. Partial request returned after loop exhaustion.
- `httpd/src/router.rs:24-63`: dispatch follows parse without requiring `httparse::Status::Complete` and a fully received declared body.
- `httpd/src/handlers.rs:31-74,165-189,240-324`: combined header/body allocations; VFS whole-file buffering; synchronous AI generate. `static_files.rs:8` caps file bodies at 64 KiB.
- `cells/services/net/src/handlers/tcp.rs:51-167`: TCP operations probe immediately. Accept uses Err(0xFE) for not-ready; recv empty and send zero conflate several states. Accept replaces listening socket with a new listener; close immediately removes socket rather than proving TX drained.
- `net/src/socket_table.rs:14-20,59-89`: 18 global capability slots, SocketSet storage +2; misleading legacy comment about management accounting. Owner = attested CellId + generation, not TID.
- `net/src/handlers.rs:70-83`: TCP RX/TX each 4 KiB. `TcpRecv` temporary buffer and reply envelope need framing-aware sizing.
- `net/src/service-runtime.rs:132-155,230-304`: single mutable SocketSet owner. Blocking DNS (`dns.rs:125-184`), driver reply waits (`interface.rs:247-306`), and raw TLS connect/handshake (`tls_handler.rs:99-166`) can stall TCP dispatch.
- Scout found no owner-death socket sweeping in net; explicit close is normal reclamation path. Need integrate cleanup before crash/restart capacity claims.
- `libs/ostd/src/task.rs:21-48`: same-cell threads exist. `heap.rs:12-19,74-98`: allocator assumes single task/hart and is unsynchronised. `kernel/src/task/scheduler.rs:9-22,778-843`: 32 tasks/cell including root, stacks charged to quota. Thread-per-request rejected.
- `libs/ostd/src/ipc.rs:116-180`: bounded call still parks after admission; late replies require poisoning peer generation. One receive owner, source masking is not per-request correlation.
- `libs/ostd/src/executor.rs:18-76`: one future plus TIMER park, not socket reactor. Completion ABI has only NET_RX/TIMER; kernel completion queue has 32 slots and one waiter.
- `libs/ai-sdk/src/lib.rs:259-326`: submit/poll/cancel exist, but exchanges are synchronous; usable as handler steps only after nonblocking transport integration.

## Public contracts and blast radius
HTTP routes/response bodies/CLI and one-request-per-connection remain; no HTTP/2, TLS termination, keep-alive or worker-cell feature. Affected: httpd, net, shared IPC/ostd adapter, kernel async prerequisite if absent, AI/VFS adapters, packaging/CI and network documentation. API additions under libs/api or libs/types require Law 1 checkpoints (`docs/code-standards.md:40-50`). Do not silently reinterpret old wire variants.

## Existing plans
- `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md`: pending shared bounded nonblocking submission/completion lifecycle; reuse this prerequisite, do not invent competing kernel queues.
- `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md`: only TIMER/NET_RX completed; generic reactor deferred; cancellation must not free grants still in use.
- `docs/roadmap/beam-parity-backend-roadmap.md`: B1/B3 overlap. Old thread/capacity claims must not override current source.

## Runtime smoke — 2026-10-06
Existing RV64 kernel `target/riscv64gc-unknown-none-elf/release/cellos-kernel` + `disk_v3.img`, QEMU virt 256 MiB, SLIRP localhost forwarding, restrict=on, -snapshot. Existing artifacts, not rebuilt or proven identical to current source.
- Shell/DHCP/httpd listen observed. GET /api/status: HTTP 200, JSON status running, wall time 0.0897 s (single sample, not benchmark).
- Open POST /api/infer with Content-Length:100 but send only x. Start GET /api/status 30 ms later: HTTP 200 in 0.0205 s. Partial POST itself returned HTTP 200 and prompt_bytes:1, confirming premature body dispatch in this image.
- This DOES NOT demonstrate concurrent execution or quantify HOL blocking: first handler could already have completed. Future concurrency oracle must withhold body through the fast-response assertion and assert no premature response/side effect.
- QEMU terminated, snapshot changes discarded. No tests/builds run for planning.

## Precedents
- 7f0cfb884: response buffering/status alias; touched AArch64 HTTP smoke and QEMU harness. Preserve both lanes, not RV64 only.
- 09d2b6b22: AI front door; touched disk packaging, HTTP-infer end-to-end gate and changelogs. Preserve native AI route, not only static GET.
- 4884ace5e: thread/TLS/futex contracts; does not make ostd allocator thread-safe.
No `.agents/failure-history.jsonl` or `.agents/incidents/*.md` found. Historical reactor plan documents masked receive, peer-death and grant-lifetime failures.

## Review conclusions
Reject cosmetic async wrappers, hundreds of OS threads, changing only MAX_SOCKETS, fast-path-only demos, and claiming no wait under saturation. Single-owner reactor + real async IPC + bounded net progress is the selected complete path. Handler/backend throughput remains independently limited; frontend concurrency is not parallel CPU execution or isolation between requests.
