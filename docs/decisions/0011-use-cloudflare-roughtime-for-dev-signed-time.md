# ADR-0011: Use Cloudflare Roughtime for DEV_REFERENCE signed time

- **Status:** Accepted — deployment blocked by provider incompatibility
- **Date:** 2026-08-31

## Context

The Phase 5 `DEV_REFERENCE` signed-time service exposes the CellOS HTTPS
`POST /v1/time` contract. Its Lambda must obtain a fresh authenticated upstream
time interval before allocating and signing a CellOS time response. The prior
plan assumed a custom signed-HTTPS upstream, but no provider contract was
available: there was no exact endpoint, response schema, signature preimage,
canonicalization rule, pinned verification identity, or freshness and
uncertainty semantics to implement or verify.

Cloudflare publishes a Roughtime service with an exact endpoint and long-term
public key. Roughtime binds a request nonce into a signed response and reports
rough time as a midpoint and radius. That is the authenticated interval required
by the existing clock-policy boundary without trusting the Lambda host clock.

Cloudflare marks the service beta and warns that its root public key may change.
Its public source currently implements IETF drafts 11 and 8, regards its API as
unstable, disclaims backwards compatibility, and says not to use it in
production software. Cloudflare publishes no deployed source revision,
response-profile contract, radius configuration, or rollout record for the
public endpoint. This lane pins draft 11 exactly; it does not infer deployed
conformance from repository code or synthetic vectors.
The selected service is also a single provider. This decision therefore trades
availability and protocol stability for a concrete, independently operated,
cryptographically authenticated development source.

## Decision Drivers

- Pin an exact external source identity instead of trusting ambient Lambda,
  application-processor, RTC, build, or public-network time.
- Obtain a fresh, signed interval on every allocation, bound to a per-request
  nonce and carrying explicit uncertainty.
- Fit the existing Phase 5 clock-policy input without inventing a provider
  schema or signature contract.
- Keep the source independent from the project-operated Lambda, DynamoDB, and
  KMS signing path.
- Fail closed on source outage, key change, invalid evidence, or excessive
  uncertainty; do not gain availability by silently changing trust roots.
- Keep all resulting evidence beneath the `DEV_REFERENCE` ceiling and preserve
  every production-admission gate.

## Considered Options

### Option A (chosen): Query Cloudflare Roughtime directly

- **Pro:** Cloudflare publishes the exact server endpoint and long-term public
  key needed for static verification.
- **Pro:** A fresh request nonce is cryptographically incorporated into the
  signed response, and the signed `MIDP`/`RADI` values provide the required
  authenticated interval.
- **Pro:** Cloudflare operates the source independently of the CellOS AWS
  account and Phase 5 signing stack.
- **Con:** The service is beta, its root key may rotate, and Cloudflare warns
  that its draft-11 implementation is unstable and not for production software.
- **Con:** A single provider and single configured server make Cloudflare or
  network-path outage a CellOS signed-time outage.
- **Chosen because:** It is the only considered option with a currently
  published endpoint, pinned verification key, nonce-bound signed response, and
  explicit uncertainty interval that can satisfy the development clock-policy
  contract without expanding the project-operated trust base.

### Option B: Retain the planned custom signed-HTTPS upstream

- **Pro:** HTTPS egress and a JSON or CBOR adapter would be operationally
  familiar.
- **Con:** No available provider freezes an endpoint, response schema,
  signature and canonicalization contract, pin identity, or freshness and
  uncertainty semantics for this lane.
- **Con:** Designing against an imagined provider would make tests prove a
  repository-created fixture rather than a live external contract.
- **Rejected because:** The required authentication and interval contract does
  not exist, so the adapter could not be implemented or admitted truthfully.

### Option C: Use NTS or public NTP directly

- **Pro:** NTP is widely deployed, and NTS authenticates an NTP client/server
  exchange.
- **Con:** Unauthenticated public NTP is not an admissible trust source.
- **Con:** NTS does not provide the portable, independently verifiable signed
  response interval/object, bound to the request nonce, that the Lambda clock
  contract must validate and pass into allocation policy.
- **Con:** Treating an NTP sample as that object would reintroduce trust in the
  sampler or host clock instead of satisfying the signed-evidence boundary.
- **Rejected because:** Neither public NTP nor NTS supplies the required
  portable signed interval object for this contract.

### Option D: Operate a CellOS Roughtime or signed-time gateway

- **Pro:** CellOS could control deployment, monitoring, and protocol adaptation.
- **Con:** A project-operated gateway adds its host clock, signing-key custody,
  deployment principals, persistence, and incident response to the trusted
  computing base.
- **Con:** Placing both the upstream source and Phase 5 signer under project
  operation weakens the independent-source value that detects a bad local AWS
  clock or control plane.
- **Con:** It creates another service whose availability, key rotation, rollback,
  and audit rules must be qualified.
- **Rejected because:** It expands the TCB and weakens source independence
  without solving a requirement that Cloudflare's direct service already meets.

## Decision

Phase 5 will use one Cloudflare Roughtime provider as its sole upstream
`DEV_REFERENCE` time source:

- endpoint: `roughtime.cloudflare.com:2003`;
- pinned base64 long-term public key:
  `0GD7c3yP8xEc4Zl2zeuN2SlLvDVVocjsPSL8/Rl/7zg=`; and
- protocol basis: Cloudflare's published draft-11 implementation profile,
  version `0x8000000b`, pinned by its Apache-2.0 source and official vectors.

Each upstream request must use a fresh per-request nonce. The exact request root
contains `VER`, `NONC`, `SRV`, and `ZZZZ`; it contains no draft-15 `TYPE` tag.
The response root contains `SIG`, `VER`, `NONC`, `PATH`, `SREP`, `CERT`, and
`INDX`; `SREP` contains `ROOT`, `MIDP`, and `RADI`. Acceptance requires the
draft-11 delegation context `RoughTime v1 delegation signature--\0`, response
context `RoughTime v1 response signature\0`, and Merkle leaf
`H(0x00 || nonce)`. Merkle paths follow Cloudflare `protocol.go` and official
vector 010: index bit 0 folds `H(0x01 || current || sibling)` and bit 1 folds
`H(0x01 || sibling || current)`. This is the opposite of the ordering written
in IETF draft-11 section 6.3.1; this ADR selects Cloudflare's provider profile
and makes no generic draft-11 interoperability claim.
The complete signature chain and request-inclusion proof must validate from the
exact pinned long-term key and bind that nonce. `MIDP`
is uint64 seconds and `RADI` is uint32 seconds with `RADI >= 3`. The asserted
true-time interval is the open interval
`(MIDP - RADI, MIDP + RADI)`; checked arithmetic and the existing configured
age, uncertainty, floor, and expiry policy determine whether Phase 5 may use it.
The Roughtime nonce binding and the CellOS `/v1/time` request binding are both
mandatory freshness controls; one does not replace the other.

This is a single-source decision. There is no alternate Roughtime server,
alternate time protocol, cached holdover, host-clock fallback, or availability
exception. Every new allocation requires a fresh valid response from this exact
source. An unreachable source, timeout, malformed response, nonce mismatch,
signature or inclusion failure, radius below three or above policy, stale sample,
regressed interval, or policy ambiguity produces no CellOS time fact.

Runtime DNS TXT key discovery is forbidden. DNS A/AAAA resolution may locate the
published endpoint, but DNS does not establish time-source identity; only the
manifest-pinned long-term key does. The key may not be downloaded, refreshed,
or replaced automatically.

The change is confined to the upstream clock adapter: the planned custom
signed-HTTPS upstream is replaced by Roughtime over UDP only on port 2003. The
externally consumed CellOS Regional API Gateway HTTPS `POST /v1/time` request,
canonical CellOS response, DynamoDB allocation, and AWS KMS signing contracts do
not change.

## Key-Rotation Rule

Cloudflare documents the service as beta and may rotate its root public key. A
published or observed key different from the pinned value is a hard stop, not a
recoverable verification or availability event. Phase 5 must remain sealed.

Service may resume only after an operator explicitly approves the replacement
key in the reviewed deployment manifest and advances to a new source epoch.
The old and new keys must not be accepted concurrently, and DNS TXT or remote
document contents must never update the running trust root. The new epoch makes
the trust-root change visible to protected non-regression policy and evidence.

## Implementation Constraints

- The deployment manifest must pin the exact endpoint, long-term key, Cloudflare
  draft-11 profile, source epoch, `udp` transport, 2,000 ms timeout, exact
  1,012-byte request-message size, 1,024-byte maximum packet size, maximum
  sample age, and maximum accepted uncertainty.
- Parsing and verification must be bounded and fail closed. Before exposing a
  sample, the adapter must verify the complete Cloudflare-profile response path,
  including root `VER`/`NONC`, the long-term-key-anchored delegation, signed
  `SREP`, provider-vector Merkle inclusion, `MIDP`, and `RADI >= 3`.
- Interval arithmetic must reject underflow, overflow, malformed widths, and
  values outside the existing clock-policy bounds. Integer-second policy input
  is the closed subset
  `[MIDP - RADI + 1, MIDP + RADI - 1]`; neither excluded endpoint of the
  provider's open interval may be admitted. The uncertainty checked against
  policy remains the full provider span `2 * RADI`, not the narrower closed
  subset width. Because Roughtime supplies no separate sample expiry,
  `sample_valid_until` is exactly `MIDP + RADI`; overflow rejects. This
  authenticated interval is input to policy, not permission to bypass protected
  source-epoch, sequence, or Unix floors.
- A Roughtime response must not be cached or reused for a later allocation.
  Retries of an already committed CellOS receipt remain governed by the existing
  immutable receipt contract and do not constitute a new time sample.
- The network, DNS result, Lambda host clock, and transport success are
  untrusted. Only a valid object rooted in the pinned key can supply upstream
  time evidence.
- One request uses one exact 1,024-byte UDP datagram containing its 12-byte
  `ROUGHTIM` packet header and 1,012-byte Roughtime request message. The adapter
  performs exactly one DNS resolution, selects exactly one returned UDP address,
  sends once, and receives once with truncation detection and a 2,000 ms timeout.
  A response above 1,024 bytes, truncation, empty or unusable resolution, partial
  send, timeout, or any other transport ambiguity fails. There is no retry,
  address iteration, TCP, or transport fallback.
- No production marker, provider, endpoint, trust root, or admission rule is
  selected or relaxed by this ADR.

## Compatibility Gate Result

Cloudflare's Apache-2.0 vectors 001 and 010 match the generated 1,024-byte
requests and verify through the complete test-key chain; vector 010 exercises a
non-empty ten-request Merkle batch. They are not captures from
`roughtime.cloudflare.com`. Cloudflare generates them locally with fixed private
keys, `MIDP=50`, `RADI=5`, and its own `CreateReplies`/`VerifyReply` pair.

One post-correction query was sent without retry to the published endpoint. Its
352-byte response authenticated under the published long-term key: delegation,
signed `SREP`, nonce Merkle proof, response version `0x8000000b`, and delegation
window all verified. It nevertheless omitted draft-11's mandatory root `NONC`
and signed `RADI=1`, below draft-11's mandatory minimum of three seconds. The
strict adapter rejected it.

Source history now explains the fingerprint without making it admissible.
Cloudflare commit `d09eb373` added root `NONC` to replies in December 2024;
commit `932a07ae` made its client reject missing or substituted response
`NONC`; and regenerated vectors postdate that fix. Current draft-11 source emits
`NONC`, while the checked-in test server still hard-codes a one-second radius
without enforcing the draft-11 minimum. Cloudflare issue 72 independently
reports that the public endpoint fails the current official IETF client with
`protocol: response is missing NONC tag` and returns a one-second Google-profile
radius. The exact deployed binary and configuration remain unpublished, so
identifying the service as a particular pre-fix/test-server build is inference,
not operator evidence.

No reviewed protocol permits `VER=0x8000000b` with missing root `NONC` or
`RADI=1`. Draft 8 also mandates `NONC`, and its lower radius rule cannot override
the authenticated draft-11 version. The adapter is therefore correct: do not
remove the root-`NONC` check, lower the radius floor, reinterpret the signed
radius, or treat implementation fixtures as a live-service contract.

The ADR's original live-interoperability premise is invalid. This observation
proves endpoint/key reachability only. Deployment remains blocked until the
endpoint emits the exact pinned profile or a separately reviewed provider/profile
decision advances the source epoch and proves its nonce and interval semantics.

## Consequences

### Positive

- Phase 5 has a concrete provider contract instead of a speculative custom
  signed-HTTPS dependency.
- The clock adapter can authenticate freshness and uncertainty without trusting
  an ambient machine clock.
- The upstream source remains independently operated from the CellOS AWS
  allocator and signer.
- The existing external CellOS `/v1/time` API and protected downstream floors
  remain unchanged.

### Security and Protocol Risks

- A compromised Cloudflare long-term key or admitted server can assert a false
  interval. Pinning authenticates the selected source; it does not prove that
  the source's clock is correct.
- Single-server operation does not obtain Roughtime's multi-server consistency
  or malfeasance-detection value.
- The beta service may rotate its root, and Cloudflare's draft-11 implementation
  explicitly provides neither API stability nor backwards compatibility and
  warns against production use. Any protocol or key change requires review
  rather than automatic compatibility behavior.
- Roughtime authenticates the response object, not network availability. DNS,
  routing, UDP filtering, and denial of service can prevent allocation.

### Operational and Availability Costs

- Cloudflare service outage, endpoint reachability loss, or rejected evidence
  seals every normal runtime action that requires a new signed-time allocation.
- There is no alternate provider, cached lease continuation, holdover, or host
  clock fallback to preserve service. This single-provider availability loss is
  an explicit cost of the decision.
- Key rotation requires operator review, a manifest update, a new source epoch,
  deployment, and fresh evidence before service resumes.
- Operations must monitor provider status and the published key without allowing
  monitoring data to mutate runtime trust.

## Evidence Ceiling

This ADR remains under
[ADR-0007](./0007-development-first-hardware-constrained-execution.md). Host
parsers, vectors, and policy tests are `SOFTWARE_HARNESS` evidence. Authorized
live queries can prove only that the exact Cloudflare endpoint and pinned key
produced provider-profile responses with the observed nonce and interval at the
recorded time. Neither class qualifies Cloudflare as a production time authority,
establishes multi-provider correctness or availability, proves protected-device
state, or satisfies any production-admission or release gate.

Production use requires its own exact provider, trust-root, outage, rotation,
protected-floor, and physical/runtime evidence decision. No `DEV_REFERENCE`
result from this lane may be promoted to that claim.

## Links

- [ADR-0007: Use development-first hardware-constrained execution](./0007-development-first-hardware-constrained-execution.md) — this choice and all resulting software/live evidence remain bounded to `DEV_REFERENCE` and cannot satisfy production gates.
- [ADR-0012: Use an external lineage table and KMS key for signed-time epochs](./0012-use-external-lineage-table-and-kms-key.md) — selects the independent allocator-incarnation/epoch authority consumed by the same manifest.
- [Phase 5 nonce-bound signed-time plan](../../.agents/260826-1605-phase4-dev-reference-authority/phase-05-nonce-bound-signed-time-service.md) — owns the unchanged CellOS `/v1/time` contract and the upstream adapter implementation.
- [TIME-001..008 and AC-004/005](../../.agents/260825-1726-kms-silo-production-root/spec.md) — fail-closed signed-time and protected-floor requirements.
- [Cloudflare Roughtime usage documentation](https://developers.cloudflare.com/time-services/roughtime/usage/) — published endpoint, long-term public key, beta status, and rotation notice.
- [Cloudflare Roughtime repository](https://github.com/cloudflare/roughtime) — selected provider profile, official draft-11/draft-08 support statement, unstable-API warning, Apache-2.0 implementation, and authoritative vectors including non-empty-path vector 010.
- [Cloudflare NONC reply fix](https://github.com/cloudflare/roughtime/commit/d09eb37366a1861d0c53711ad035d1defa7e3a6a) and [matching verifier fix](https://github.com/cloudflare/roughtime/commit/932a07ae00a1912339aa62b38996a86d5f0a5eae) — establish that current source emits and requires response `NONC`.
- [Cloudflare issue 72](https://github.com/cloudflare/roughtime/issues/72) — independent reproduction that the published endpoint remains incompatible with the current official IETF client.
- [IETF Roughtime draft-11](https://www.ietf.org/archive/id/draft-ietf-ntp-roughtime-11.html) — source for selected fields, signature contexts, nonce proof, and interval semantics; its section 6.3.1 path-fold ordering is not the Cloudflare provider ordering selected above.
