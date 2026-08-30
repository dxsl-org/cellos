# ADR-0009: Correlate relay packet failures without legacy framing

- **Status:** Accepted
- **Date:** 2026-08-29

## Context

The relay server currently accepts `FT_SEND_PACKET (0x08)` as
`type || destination_node_id[32] || Noise_ciphertext` and returns
`FT_ERROR (0x7f) || code`. The encrypted C2C envelope contains the application
`request_id`, but the relay cannot read it without breaking end-to-end Noise.

One authenticated relay connection may carry several outstanding requests. An
uncorrelated destination or forwarding error therefore cannot identify which
request failed. Applying it to every request loses definite outcomes; guessing a
request can retry a non-idempotent call after possible delivery. Serializing the
connection would avoid ambiguity but introduce head-of-line blocking and defeat
the bounded pipelining already required by the broker contract.

No Cellos relay client is implemented or deployed. This permits a clean wire
cutover without negotiation, compatibility aliases, or a downgrade path.

## Decision Drivers

- Correlate every request-scoped relay failure to one exact outbound request.
- Keep the C2C envelope and application `request_id` opaque to the relay.
- Preserve the distinction between definite pre-write absence and uncertain
  failure after a destination write may have started.
- Support bounded pipelining without allocation or unbounded relay state.
- Reject legacy framing rather than silently misparse it.
- Keep protocol-fatal errors separate from request outcomes.

## Considered Options

### Option A (chosen): Clean cutover to correlated send and packet-error frames

- **Pro:** Gives exact request correlation while leaving Noise ciphertext opaque.
- **Pro:** Needs no relay-side request table; the correlation value is copied only
  into an immediate error.
- **Pro:** A new send frame type makes legacy frames fail visibly instead of being
  interpreted under a changed layout.
- **Con:** Existing clients using `FT_SEND_PACKET (0x08)` become incompatible.
- **Chosen because:** No Cellos relay client exists, so compatibility machinery
  would preserve only a hypothetical caller and create downgrade risk.

### Option B: Dual-stack legacy and correlated frame types

- **Pro:** Allows old and new clients to share one relay deployment.
- **Con:** Requires capability negotiation or implicit mode detection, permanent
  legacy tests, and conservative semantics for uncorrelated errors.
- **Rejected because:** There is no deployed client to justify the extra state or
  downgrade surface.

### Option C: Permit only one outstanding request per relay connection

- **Pro:** The existing uncorrelated error would be unambiguous.
- **Con:** Creates head-of-line blocking, suppresses bounded concurrency, and
  turns one slow request into connection-wide backpressure.
- **Rejected because:** It avoids a small wire correction by weakening the
  required transport behavior.

### Option D: Expose the C2C `request_id` outside Noise

- **Pro:** Reuses an existing identifier.
- **Con:** Couples relay framing to C2C envelope semantics and reveals a stable
  application-level sequence to the relay.
- **Rejected because:** A transport-local correlation sequence is sufficient and
  preserves the end-to-end protocol boundary.

## Decision

1. **Retire legacy send framing.** `FT_SEND_PACKET (0x08)` is no longer accepted.
   Receipt is a fatal unknown-frame error followed by connection close. There is
   no compatibility mode or negotiation.
2. **Correlated send.** `FT_SEND_PACKET_CORRELATED (0x0d)` is encoded as
   `type || correlation:u64be || destination_node_id[32] || Noise_ciphertext`.
   The ciphertext remains untouched and subject to the existing frame bound.
3. **Protected framing owner.** The Protected Relay Authority accepts only a
   typed bounded `{session_generation, correlation, destination_node_id,
   Noise_record}` operation and constructs `0x0d` inside the TLS endpoint. On
   receive it parses the outer relay frame and returns a typed generation-bound
   packet/error event. Net-broker never supplies or receives raw relay
   application-frame bytes. Server-only codec work may land before the protected
   Build gate; authority/client integration waits for ADR-0008 Phase 4 Build.
4. **Session-scoped sequence.** Each authenticated TLS connection allocates
   correlation values from `1..=u64::MAX` in strict order. Zero is invalid;
   values never repeat within the connection; exhaustion closes the connection
   before another request is accepted. The broker proposes the next value and
   the authority enforces it for the current session. Broker state keys a bounded
   outstanding entry by `{relay_session_generation, correlation}`.
5. **Request-scoped error.** `FT_PACKET_ERROR (0x0a)` is exactly
   `type || correlation:u64be || code:u8`. Only
   `ERR_DESTINATION_UNAVAILABLE (0x01)` and
   `ERR_DELIVERY_UNCERTAIN (0x04)` are valid packet-error codes.
6. **Delivery classification.** Missing destination or failure proven before any
   destination write returns correlated `ERR_DESTINATION_UNAVAILABLE`. Once a
   destination write may have started, write/drain failure returns correlated
   `ERR_DELIVERY_UNCERTAIN`. Successful drain is not an application receipt and
   sends no positive acknowledgement.
7. **Fatal error separation.** `FT_ERROR (0x7f) || code` remains uncorrelated and
   is used only for malformed, unknown, or otherwise connection-fatal protocol
   input. It is followed by connection close. The authority rejects malformed
   packet errors, unknown packet-error codes, zero correlation, a correlation
   not yet accepted in the current TLS session, and legacy send framing before
   emitting any broker event.
8. **No receive-frame expansion.** `FT_RECV_PACKET (0x09)` remains
   `type || source_node_id[32] || Noise_ciphertext`. The authority parses the
   outer type/source fields; authenticated C2C responses retain their end-to-end
   request identity inside Noise.
9. **Bounded retirement.** The broker retires an active correlation on a matching
   typed packet-error event, authenticated C2C completion, or deadline/disconnect
   resolution. A current-generation correlation lower than the session's
   next-allocation counter but absent from the active table is already retired:
   ignore it and increment a bounded late/duplicate counter. It never closes the
   connection or selects a newer request. The authority rejects zero,
   future/unaccepted, stale-generation, or unauthenticated carrier input without
   applying it to the current session.
10. **Exact submission boundary.** An outbound request is `NotSubmitted` while
   the broker owns its complete typed send request. Only an explicit protected
   authority rejection that returns ownership unchanged preserves that state and
   permits a definite local unavailable result. Authority acceptance, or loss,
   timeout, reset, or cancellation before an explicit rejection, moves the
   request to `Submitted`; subsequent transport/TLS disconnect is
   `Indeterminate` unless a matching correlated definite error or authenticated
   C2C completion resolves it. No partial/implicit acceptance state exists.
11. **Not an authority token.** Correlation values authorize nothing, prove no
   identity, and are not logged as stable node identifiers.

## Consequences

### Positive

- Pipelined relay errors map to exactly one bounded outbound request.
- The relay learns no C2C `request_id` or plaintext and stores no correlation
  table.
- Definite and uncertain delivery outcomes remain mechanically distinct.
- Legacy clients fail closed instead of entering ambiguous mixed framing.

### Negative / Risks

- Relay server fixtures move immediately; every future client starts on the new
  frame because no legacy client remains supported.
- Correlation lifecycle becomes part of broker reconnect, deadline, and dedup
  tests, while protected authority/client integration remains Phase 4 Build work.
- A transport disconnect cannot identify which submitted requests reached a
  destination; every unresolved `Submitted` request remains conservatively
  `Indeterminate`.

## Verification

Acceptance evidence must prove:

- the new send and packet-error byte layouts round-trip at exact bounds;
- `0x08`, zero correlation, future/unallocated correlation, malformed length,
  and unknown packet-error code are fatal without selecting another request;
- a late/duplicate retired correlation is ignored, increments only its bounded
  counter, and cannot close the session or select a newer request;
- destination absence returns the same active correlation with definite
  unavailable;
- destination write/drain failure returns the same active correlation with
  uncertain;
- two interleaved requests receive independently correlated failures;
- correlation exhaustion closes before reuse;
- explicit pre-accept rejection returns typed request ownership and remains
  `NotSubmitted`; acceptance or an ambiguous send-call outcome becomes
  `Submitted`;
- disconnect maps `NotSubmitted` to definite local unavailable and every
  unresolved `Submitted` request to `Indeterminate`;
- `FT_RECV_PACKET` and Noise payload bytes remain unchanged;
- server-only codec tests may pass before the Build gate, but client evidence
  must prove that the authority alone constructs/parses outer relay frames from
  typed generation-bound operations and net-broker never exchanges raw frames;
- disconnect/reconnect cannot apply an old session's correlation to a new
  session generation.

Current server-only evidence: four changed Python files compile and the focused
relay-server suite passes 40/40, including opaque forwarding, exact correlated
definite/uncertain errors, two interleaved failures, and fatal legacy/zero/
malformed input. Authority/client lifecycle evidence remains Phase 4 work.

## Links

- [ADR-0005](./0005-mutual-tls-relay-identity.md) — relay mTLS identity and NodeId admission.
- [ADR-0008](./0008-protected-relay-tls-endpoint-ownership.md) — protected TLS endpoint ownership remains unchanged.
- [Relay-first C2C plan](../../.agents/260819-1409-cell-to-cell-anywhere-core/phase-05-relay-first-remote-correctness-oracle.md) — implementation and isolated oracle owner.
