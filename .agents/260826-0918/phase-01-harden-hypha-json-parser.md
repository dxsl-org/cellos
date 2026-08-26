# Phase 01 — Harden Hypha JSON Parser

## Context Links

- [Plan](plan.md)
- `cells/apps/hypha/llm-gateway/src/http.rs`
- `libs/http-core/src/json.rs`

## Overview

Use the existing no-std JSON codec for structured extraction and add a bounded
recursive validator that rejects duplicate object keys before deserialization.

## Key Insights

- `ostd::json` already supplies `serde_json` with `alloc`; a second parser is unnecessary.
- `serde_json::Value` keeps the last duplicate key, so uniqueness must be checked first.
- Escaped-equivalent keys must compare after JSON decoding.

## Requirements

- Reject malformed syntax, trailing tokens, invalid escapes/UTF-8, and duplicate keys.
- Support escaped response strings and nested tool arguments.
- Preserve no-std operation and existing public signatures.

## Architecture

`http.rs` owns protocol extraction; a private JSON validation module owns
recursive syntax/duplicate checks; `ostd::json::Value` owns semantic traversal.

## Related Code Files

- Modify `Cargo.toml`, `src/main.rs`, and `src/http.rs`.
- Create `src/json-validation.rs` and `src/http-tests.rs` if needed to stay under 200 LOC.

## Implementation Steps

1. Enable `ostd`'s `json` feature.
2. Validate complete JSON input and reject duplicate decoded keys recursively.
3. Extract `choices[0].message.content` through typed `Value` traversal.
4. Parse tool-call root/name/args structurally and serialize object arguments.
5. Add focused positive and negative regression tests.

## Todo List

- [x] Capture baseline.
- [x] Implement validator and structured extraction.
- [x] Run host tests and RV64 compile.
- [x] Complete independent test and production-readiness review.

## Success Criteria

- Requested malformed, escaping, nested-object, and duplicate-key tests pass.
- No accepted trailing data or escaped-equivalent duplicate keys.
- Modified Rust files remain below 200 lines.

## Risk Assessment

- Strict parsing intentionally rejects provider responses accepted accidentally before.
- Recursive cursor errors are contained by comparing behavior with the shared JSON codec.

## Security Considerations

- Duplicate keys fail closed at every object depth to prevent parser differential attacks.
- Parser inputs remain bounded by the gateway transport/IPC response buffers.

## Next Steps

- Commit the completed parser hardening when requested.

## Evidence

- Host tests: 7 passed, 0 failed.
- RV64 `cargo check`: PASS.
- Host Clippy `--no-deps -D warnings`: PASS; seven existing `ostd` warnings remain baseline-only.
- Formatting and diff checks: PASS.
- Final review: PASS 10/10 with no residual findings.
