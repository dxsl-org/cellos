# Phase 04 — HTTP Source (optional) + x86 Multi-arch Regression + Docs

## Context Links
- Plan: [plan.md](plan.md) · Depends on [Phase 02](phase-02-pkg-installer-builtin.md)
- HTTP/TLS: `libs/ostd/src/http/client.rs`, `libs/ostd/src/clients/tls_stream.rs`,
  `cells/demos/http-smoke/src/main.rs`
- x86 cell-store on NVMe: `.agents/260707-1726-g2-loader-redesign/` F4; `service::BLOCK_DRIVER`
- Roadmap: `docs/project-roadmap.md` §E, §L3 (Tooling: package manager)

## Overview
- **Priority:** P3 · **Status:** pending
- Add an optional HTTP(S) package source, validate install on the second architecture (x86_64, cell-store
  on NVMe), and reconcile docs. HTTP is a **stretch** — the core (local) install path is fully verifiable
  in P01-P03 without networking.

## Key Insights (verified)
- Download transport exists: `HttpClient::send` over `TlsStream`/`TcpStream`, body drained into a `Vec`
  (`http-smoke` line ~57). No dedicated `.get()` — build a GET via `RequestBuilder::new("GET", …)`.
- HTTP is a **source** only. Verification + persistence are unchanged (P02 path). **Remote repo protocol
  / package index / discovery = OUT OF SCOPE v1** — a URL points at one signed ELF.
- x86 routes cell-store I/O through the NVMe Driver Cell (`service::BLOCK_DRIVER`), same VFS write path
  as riscv64 virtio-blk — so P01's gated write should work unchanged; this phase **proves** it.

## Requirements
**Functional**
- `pkg install <https-url> [name]` — GET the ELF → write to `/tmp/<name>` → run the P02 install path
  (sanity + cap review + gated `/bin` write). Same for `pkg upgrade <name> <https-url>`.
- Network failures (DNS/TLS/HTTP status ≠ 200/short body) → clear error, **no** partial `/bin` write.
- x86_64 QEMU: `pkg install` + spawn works against the NVMe-backed cell-store.

**Non-functional:** optional/feature-gated so a no-net image builds without it; `#![forbid(unsafe_code)]`.

## Architecture / Data Flow
```
pkg install https://host/hello.cell
  1 GET  ── RequestBuilder GET ▶ HttpClient.send over TlsStream ▶ drain body → Vec
  2 stage ── write /tmp/hello  (writable tmp)
  3 install ── P02 path (sanity + cap review + P01 gated /bin write)
```

## Related Code Files
**Modify:** `cells/tools/shell/src/cmd_pkg.rs` — URL detection (`http://`/`https://`) → download branch
before the existing local-path branch.
**Create (maybe):** `cells/tools/shell/src/pkg_http.rs` (download helper) if it pushes `cmd_pkg.rs` over
200 LOC.
**Docs (modify):**
- `docs/project-roadmap.md` §E — mark package manager In-Progress/Done with scope note (local + optional
  HTTP; no remote repo); §L3 Tooling row.
- `docs/code-standards.md` or a new `docs/guides/cell-packages.md` — `pkg` usage + the trust model
  (signed ELF = package; spawn-gate = enforcement; install-time = advisory).
- `docs/specs/15-kernel-boundary.md` — one line confirming package distribution added **0** kernel code
  (userspace vfs+shell only).
**Test harness (modify):** add the multi-arch install oracle to the x86 + riscv64 suites.

## Implementation Steps
1. URL scheme detection in `pkg install`/`upgrade`; branch to download helper.
2. `download(url) -> Result<Vec<u8>, Err>` using `RequestBuilder` + `HttpClient` + `TlsStream`; enforce
   status 200 and complete body; stage to `/tmp`.
3. Feature-gate the HTTP branch (e.g. `--features pkg-http`) so non-net builds exclude it.
4. x86_64 QEMU regression: install a signed cell into the NVMe cell-store and spawn it.
5. Docs + spec reconciliation; roadmap status update.

## Todo List
- [ ] URL detection + download branch
- [ ] `download` helper (GET/TLS, status + body checks, stage to /tmp)
- [ ] Feature gate for HTTP
- [ ] x86_64 install+spawn regression (NVMe cell-store)
- [ ] riscv64 full hardened-suite green
- [ ] Docs: roadmap §E/§L3, cell-packages guide, spec 15 note

## Success Criteria
- **Oracle (opt, if test net available):** `pkg install -y https://<test-host>/hello.cell && hello` runs;
  a 404/short-body URL fails cleanly with no `/bin` write.
- **x86_64 QEMU:** `pkg install -y <path> && <cell>` runs against the NVMe cell-store; boundary tamper
  oracle also holds on x86.
- **riscv64:** full hardened suite green (no regression from P01 VFS changes).
- Docs updated; `grep kernel/src` for this plan's changes = empty.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|-----------|
| Test env has no HTTP server / TLS cert | High×Low | HTTP oracle optional + feature-gated; core install verified locally in P01-P03 |
| x86 NVMe cell-store write differs from virtio-blk | Med×Med | Same `service::BLOCK_DRIVER` VFS path; this phase explicitly tests it; fall back to documenting x86 install as follow-up if a driver gap surfaces |
| Partial download written to /bin | Low×Med | Stage to `/tmp`, validate fully, only then gated `/bin` write |
| Scope creep into a remote repo protocol | Med×Med | Explicitly out of scope; URL = one ELF only |

## Security Considerations
- A downloaded ELF is untrusted bytes — it flows through the **same** sanity + cap-review + spawn-gate as
  a local package. TLS protects transport integrity; the fleet **signature** protects authenticity. A
  package from any URL still cannot spawn without a valid fleet sig.
- Do not follow cross-host redirects silently in `download` (surface them) — avoids SSRF-style surprises.

## Next Steps
Follow-ups (out of scope, note in roadmap): remote package **repo/index** protocol; userspace Ed25519
**install-time verify** (reject-early); per-package **version index**; `pkg upgrade --hotswap` wired to
the Supervisory Cell; hardware/kernel **install-capability** for untrusted third-party packages (spec 15
§1.4, G2).
