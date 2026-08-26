# P03 — E2E Verification Gate (runbook + status)

**Status:** code + docs DONE and compiling. **Live e2e NOT executed in the build sandbox**
(needs outbound internet from QEMU SLIRP + headless shell-driving that this environment can't
reliably prove). Run the steps below in your environment — the pass criteria are exact.

## What was implemented (and verified to compile)
- `handlers.rs` 0x30 path: logs a **distinct** message for a verification reject
  (`connect REJECTED — certificate verification failed`) vs a transport timeout
  (`transport I/O`). Makes the gate falsifiable — a timeout can't be mistaken for "MITM blocked".
- `run.ps1` (riscv): added `virtio-rng-device` (TLS entropy; `ViRng` panics without it). ARM
  `run-arm-virt.ps1` already had it.
- `transport.rs`: 30s wall-clock deadline + heartbeat (so a slow software verify can't false-timeout).
- `docs/specs/07-networking.md` §6: verification contract + residual threat model.
- `app-https-demo` compiles against current ostd/net.

## The gate logic (why this proves verification runs)
The default build (`tls-ca-private`) trusts ONLY our self-signed ECDSA P-256 test CA. A real public
host (example.com etc.) presents a cert chaining to a public CA — which does **not** chain to our test
CA. So:

| Build | Connect to a real public HTTPS host | Expected |
|-------|--------------------------------------|----------|
| **default** (`tls-ca-private`) | example.com:443 | **REJECT** — `https-demo` prints `TLS handshake failed`; net logs `connect REJECTED — certificate verification failed` |
| **`tls-insecure`** | same host | **ACCEPT** — `https-demo` prints `TLS handshake OK`; net logs `!!! INSECURE TLS BUILD !!!` banner |

The *contrast* is the gate: same connect, opposite result. Under the old `UnsecureProvider` the default
build would have printed `TLS handshake OK` — so a reject here proves verification is enforced. This is
the **NEG-untrusted** case and needs no private key.

## Runbook

### 0. Package https-demo onto the disk — DONE ✅
`gen_disk.ps1` now builds `app-https-demo` and installs it as `/bin/https-demo` in both VIFS1
(`$kfs_args`) and the disk table (`$table_args`). No action needed — `.\gen_disk.ps1` picks it up.

### 1. NEG-untrusted (default build) — THE gate
```powershell
cd D:\ViCell
cargo build -p service-net --release                 # default = tls-roots-embedded + tls-ca-private
.\gen_disk.ps1                                        # regenerates disk with the verifying net cell + https-demo
.\run.ps1                                             # boots with virtio-rng now present
# at ViCell> prompt:
ViCell> https-demo
```
**PASS** = `https-demo` prints `[https-demo] ERROR: TLS handshake failed` AND the net cell logs
`[net/tls] connect REJECTED — certificate verification failed`.
**FAIL (ship-blocker)** = `TLS handshake OK` (verification silently bypassed) OR the failure logs
`transport I/O` (a routing/timeout problem, not a verification result — fix networking and re-run).

> If your QEMU has no outbound internet, the TCP connect itself fails before TLS and you'll see a
> transport/timeout failure, not a verification reject — the gate is inconclusive. Ensure SLIRP can
> reach the host (the default `-netdev user` NATs to the internet) and the target IP/host is current
> (update `EXAMPLE_IP`/`HOSTNAME` in `cells/demos/https-demo/src/main.rs` to a reachable HTTPS host).

### 2. INSECURE regression (proves the verifying build genuinely differs)
```powershell
cargo build -p service-net --release --no-default-features --features tls-insecure
.\gen_disk.ps1 ; .\run.ps1
ViCell> https-demo
```
**PASS** = `TLS handshake OK` AND the `!!! INSECURE TLS BUILD !!!` banner appears.

### 3. (Optional) POSITIVE — valid matching cert
Build `--no-default-features --features "tls-roots-embedded,tls-ca-amazon"` and point `https-demo` at an
AWS endpoint whose chain roots in **Amazon Root CA 3** (ECDSA). Expect `TLS handshake OK`. Note: many AWS
endpoints use Amazon Root CA 1 (RSA) — pick an ECDSA endpoint or build the RSA opt-in.

### 4. (Optional) Other negatives — local controlled certs
Run a host `openssl s_server` presenting an expired / wrong-hostname / tampered cert, expose it to the
guest via SLIRP `guestfwd`, and point `https-demo` at it. Each must yield `TLS handshake failed` with the
`REJECTED` log. (Deferred — the example.com NEG-untrusted in step 1 already proves the verifier runs.)

## Honest status
Steps 1–2 are the minimal gate and are fully prepared; they were **not executed here**. The
`verifier_always_ok` unit test (B3 guard) likewise needs `ViRng`/syscalls → runs only in-QEMU.
