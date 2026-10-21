# ADR-0026: Use time.txryan.com Roughtime for DEV_REFERENCE signed time

- **Status:** Accepted
- **Supersedes:** [ADR-0011](./0011-use-cloudflare-roughtime-for-dev-signed-time.md) for upstream selection; its strict verification and fail-closed requirements remain in force.

## Context

The operator approved replacing Cloudflare with `time.txryan.com` and retaining
`roughtime.se` as the second candidate. Cloudflare's observed public replies
failed the existing mandatory root-NONC and minimum-radius requirements. Those
requirements must not be weakened to restore availability.

## Decision

The sole admitted upstream for this DEV_REFERENCE lane is:

- Endpoint: `time.txryan.com:2002`, UDP.
- Pinned Ed25519 long-term public key:
  `iBVjxg/1j7y1+kQUTBYdTabxCppesU/07D4PMDJk2WA=`.
- Protocol: existing draft-11 profile, version `0x8000000b`.
- Transport budget: one DNS lookup, one selected address, one send and receive,
  2,000 ms timeout, 1,012-byte message and 1,024-byte framed packet.

Keep the existing exact request binding, root `VER`/`NONC`, pinned-key delegation,
response signature, delegation window, nonce Merkle proof, `RADI >= 3`, checked
interval arithmetic, freshness, uncertainty and protected-floor checks. The
existing Merkle fold convention is unchanged. A live single-leaf response does
not establish interoperability for every non-empty batch. Regression evidence
uses an authenticated capture from the selected source and a locally specified
four-leaf tree covering both branch directions, with no legacy-provider fixtures.

`roughtime.se:2002` is the second **candidate**, not a second admitted runtime
source. Its published candidate key is
`S3AzfZJ5CjSdkJ21ZJGbxqdYP/SoE8fXKY0+aicsehI=`. Candidate designation grants no
runtime trust. Promotion requires a separate reviewed decision, authenticated
live compatibility evidence, explicit key approval and the epoch transition
below. Runtime must not query it as a fallback, discover keys through DNS TXT,
select another protocol, retry another address or fall back to host/RTC time.

## Epoch and Deployment Boundary

An existing Cloudflare lineage must advance exactly `N -> N+1` through the
reviewed signed transition in [ADR-0012](./0012-use-external-lineage-table-and-kms-key.md),
rotate the response-signing key and retire the old key. Preserve source sequence
and Unix floors; update the packaged manifest, protected admission configuration
and exact lineage head together. Never relabel old Cloudflare state or reset its
epoch, sequence or time floor. A genuinely undeployed, fresh lineage may use the
existing genesis protocol; test fixtures are not deployed epoch state.

This change selects the adapter's source; it does not create a deployed AWS
service, perform a live epoch migration, provision protected storage or entropy,
or enable HTTPS in Ocel. Certificate verification remains fail-closed until its
independent runtime prerequisites are met. Deployment policy must permit the
full uncertainty span `2 * RADI` (at least six seconds for the observed radius),
not merely the inner integer interval width.


### Approved QEMU DEV exception

For the local Ocel test image, the operator separately approved a trusted-host
floor outside the guest image, deferring whole-host rollback protection. The
loopback-only `scripts/qemu-dev-time.py` verifies fresh draft-11 replies across
an approved upstream list: primary `time.txryan.com:2002` (key `iBVjxg/1j7y1+kQUTBYdTabxCppesU/07D4PMDJk2WA=`),
secondary `time.teax.dev:2002` (key `84pMADvKUcSOq5RNbVRjVrjiU16Dxo2XV2Qkm+4DRTg=`),
and tertiary `rough.time.nl:2002` (key `v2CievhgKsxzlWPwIkYFUXeA51Akhkv5uhJCj1/kbiY=`).
It accommodates uncertainty radius up to 5 seconds (10 s uncertainty span, matching
clock policy), failing over cleanly if an upstream times out or fails transport.
The service durably commits floor/sequence to SQLite before responding to the fresh
guest nonce.
The explicit kernel/NET `qemu-dev-https` profile cannot select the fleet TLS
flavor; default fleet time refusal is unchanged. See the
[QEMU profile guide](../guides/ocel-viewer.md#qemu-dev-https-profile).

**Amendment 2026-10-21 (maintainer decision):** a new TLS handshake is satisfied
by a nonce-bound host exchange that completed at most 2 s before the handshake's
demand, or by the first exchange started after it. The guest demands at TLS
connect acceptance so the exchange overlaps the TCP connect, and a pending demand
starts an exchange immediately instead of waiting for the 1 s idle cadence.
Measured on tuoitre.vn under QEMU/KVM the gate dropped from 0.85–2.2 s to the
idle-refresh residue; TLS itself is 13–38 ms. An exchange older than 2 s (including
any restored guest lease) still never authorizes a handshake. This loosens only the
DEV profile; fleet time refusal is unchanged.

## Considered Alternatives

- **Continue using Cloudflare:** rejected because observed responses violate the
  selected NONC/radius contract. Do not weaken verification or reinterpret its
  signed radius to accommodate it.
- **Admit roughtime.se immediately or as automatic fallback:** rejected for now;
  a draft-11 probe timed out from this environment and authenticated live
  compatibility has not been established. This is not proof the service is down.
- **Trust host/RTC time or unauthenticated NTP:** rejected because it abandons
  the approved independent, nonce-bound authenticated-time model.

## Evidence and Consequences

The preceding survey received a 392-byte reply from `time.txryan.com`, with
version `0x8000000b`, matching root NONC and `RADI=3`. Delegation and response
signatures, nonce Merkle proof and delegation validity verified. After the source
cutover, the unmodified strict verifier accepted another 392-byte live reply;
the real `RoughtimeClockAdapter` then admitted a separately fetched sample using
maximum age two seconds and maximum uncertainty six seconds. These were host
smokes with fresh nonces and the pinned source, not mocked transport or verifier
bypasses, and not guest/protected-device qualification. The selected-source
capture is retained only as an offline authentication/nonce regression vector;
runtime must never consume it as fresh time.

The service is independently operated by Tanner Ryan and explicitly provides no
uptime or accuracy guarantee. The operator requests contact before high-volume
infrastructure use. Source outage or invalid evidence stops new time allocation.
One provider's valid signature proves attribution and integrity, not that its
clock is correct; no multi-provider consistency claim is made. Any key change
requires review and a new epoch, never automatic refresh. All evidence remains
DEV_REFERENCE under [ADR-0007](./0007-development-first-hardware-constrained-execution.md).

## References

- [Operator service and public key](https://time.txryan.com/).
- [Operator implementation and draft support](https://github.com/tannerryan/roughtime).
- [Published ecosystem keys](https://github.com/cloudflare/roughtime/blob/master/ecosystem.json).
- [Ecosystem service descriptions](https://github.com/cloudflare/roughtime/blob/master/ecosystem.md).
