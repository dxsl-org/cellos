# Preparation review and parent dispositions

## Parent disposition

All eight findings accepted. Corrected architecture-versus-implementation approval wording, three denied desktop paths, dependency-test scope, SPDX OR interpretation, unsupported SSH artifact provenance, preparation-producer role and launch-policy labeling. Persisted raw baseline command output in baseline-entry-status.txt and baseline-text-engine.txt. Earlier phase/status summaries in the reviewer snapshot predate parent synchronization; current plan and phase01 explicitly remain blocked/incomplete. No review grants human approval or completes CBX-01/02.

## Original review (historical snapshot)

## Findings

### 1. Preserve the accepted architecture approval while denying implementation admission
Priority: P2  
Confidence: 0.99  
File: `.agents/261020-1750-cellbox-cli/contract-deltas.md:35-38`

“Cellbox has no current approval” is inaccurate. ADR-0028 and `.agents/plan-portfolio.md:106-126` establish maintainer approval for the architecture and documentation/plan. What is absent is an admitted source WIP slice, named integrating owner, per-delta CP1, implementation evidence, CP2, lock release, and rollout authority. The cited plan also says Phase 01 is pending; `docs/roadmap/current-focus.md:92-112` says all ten phases and CBX-01..29 remain pending, not that Phase 01 is recorded as “blocked/incomplete.” Replace the heading with “Cellbox has architecture/documentation approval only” and correct both source summaries.

### 2. Catalog the desktop launch paths that current policy denies
Priority: P2  
Confidence: 0.99  
File: `.agents/261020-1750-cellbox-cli/command-catalog.md:77-78`

The desktop caller census points generally to `apps.rs` but misses three concrete catalog edges: `/bin/app-sys-tools`, `/bin/service-httpd`, and `/bin/app-c-math-smoke`. None is accepted by `reviewed_user_target_ceiling`, so the exact desktop spawn path is denied even if an artifact exists. Packaging uses `/bin/httpd`, while `app-sys-tools` contains individual binaries and `c-math-smoke` is explicitly a verify-only binary not shipped to an image. These paths need explicit broken-current-caller dispositions and migration scenarios before the catalog can satisfy CBX-01.

### 3. Scope the “no builds or tests” statement to dependency candidates
Priority: P2  
Confidence: 0.99  
File: `.agents/261020-1750-cellbox-cli/dependency-review.md:7`

The statement that no builds or tests were performed during “Phase 01 preparation” contradicts `baseline.md:14-16`, which records an offline locked `text-engine` test run. The accurate statement is that no proposed Cellbox dependency was downloaded, compiled, or target-probed as part of the dependency review. The existing baseline test remains separate host evidence.

### 4. Remove the unsupported ripgrep license blocker
Priority: P2  
Confidence: 0.97  
File: `.agents/261020-1750-cellbox-cli/dependency-review.md:32`

`MIT OR Unlicense` is a disjunction, and MIT is already allowed by `deny.toml`; absence of Unlicense alone does not establish that a ripgrep component requires a policy change. Exact selected crates/files, their actual SPDX metadata, notices, and transitive licenses still require review, but the document should not claim a license blocker or required allow-list change until that concrete graph demonstrates one.

### 5. Support or remove the SSH “build-artifact evidence” claim
Priority: P2  
Confidence: 0.94  
File: `.agents/261020-1750-cellbox-cli/dependency-review.md:11`

The report cites manifests and exact `Cargo.lock` resolutions for the existing inbound server crypto stack, but it provides no build-artifact path, hash, target, features, or provenance. Those sources prove declarations and lock resolution, not a usable target build. Either cite durable artifact evidence or narrow the claim to local manifest/lock/source evidence. In either case, it remains unrelated to approval or feasibility of an outbound client.

### 6. Keep `--auto` and the assistant role below the authorization gates
Priority: P2  
Confidence: 0.99  
File: `.agents/261020-1750-cellbox-cli/baseline.md:3-10`

Calling this an “implementation request” and the current assistant session the “Integration owner” conflicts with the actual preparation-only scope and with `contract-deltas.md:10`, which correctly says the named integrating owner is missing. `--auto` automates actions already admitted by the plan; it does not provide CP1, select a source WIP slice, release locks, or authorize implementation. Describe this as a Phase 01 preparation invocation and the session as the preparation producer, not the cross-surface integrating owner.

### 7. Persist baseline evidence before treating it as phase provenance
Priority: P2  
Confidence: 0.99  
File: `.agents/261020-1750-cellbox-cli/baseline.md:8-16`

The exact dirty-status counts and raw test output refer only to session-local `artifact://1` and `artifact://5`. They are reviewable in the current session but are not durable repository evidence for a later implementation or CP2 packet. Persist raw status/test output or a digest-addressed evidence file before claiming reproducible phase provenance. Until then, the document can report these as session observations only. The document also correctly notes that the 65 untracked paths’ bytes were not inventoried, so the dirty workspace is not a complete source manifest.

### 8. Name the launch-profile source accurately
Priority: P3  
Confidence: 0.98  
File: `.agents/261020-1750-cellbox-cli/command-catalog.md:91-95`

`kernel/src/loader/launch_profile/profiles.rs:15-55` is an init launch-profile allowlist, not an enumeration of artifacts actually started or packaged. Keep the valid observation that `/bin/cellbox` has no reviewed launch edge, but label this source as policy evidence. Actual target/image/startup inventory comes from packaging lists and service/autoload plans.

## Remaining Phase 01 blockers

The artifacts correctly admit most of these, but they remain mandatory before Phase 01 or CBX-01/02 can be marked complete:

- Exact command syntax, options, statuses, diagnostics, script compatibility, help, completion, and shell-versus-standalone dispositions are not frozen.
- The authenticated caller-edge × backend-right matrix is absent, including enforcing service handle/worker and restricted-caller denial cases.
- Desktop-exposed operations are not exhaustively mapped to shared typed CLI operations.
- Search flags/ignore/encoding policy, editor keys/save/conflict behavior, telemetry schemas/units/completeness, and SSH behavior/trust policy are not frozen.
- Value/record/byte/session contracts, filter syntax, serialization, redirects, cancellation, backpressure, pipeline status, job identity, and argv/cwd/environment handoff remain unspecified.
- No delta has an exact layout/version/error/migration/concurrency specification, digest, accountable integrating owner, or Cellbox CP1.
- Applicable protected-lock relations have not been mapped per delta to an exact controlling owner/file-owner handoff. All 38 registered relations remain active, but they must not be described as 38 blanket Cellbox blockers; only actual overlapping scopes require the corresponding rebase and handoff.
- Ratatui features/version, Kibi reference revision, ripgrep components, and an SSH runtime remain unselected. Licenses, notices, advisories, compiler requirements, and target feasibility are unproven.
- No selected signed split-tool image/policy baseline, exact artifact set, representative behavior baseline, footprint provenance, controlled fixture set, or selected implementation slice exists.
- The dirty working tree remains user-owned. The tracked-file hashes do not bless the changes, and uninventoried untracked bytes cannot be adopted into an implementation slice.

## Facts that should be retained

- Local `Cargo.lock` and searched local cache absence means there are no current local Cellbox pins/evidence; it does not prove Ratatui, ripgrep, Kibi, Sunset, Russh, or ssh2 are globally unavailable or unportable.
- CP1 is per concrete delta and precedes edits to that protected contract surface.
- CP2 is not required before implementation. It follows exact implementation diff, caller census, and admitted runtime evidence, and must precede final caller/image retirement or rollout.
- `hc-cook --auto` is not an approval source and cannot bypass protected ABI, authority, signing, remote, physical, or production gates.
- Historical CELL-CONTRACT-0024 signoff and desktop completion do not approve unrelated Cellbox contracts.
- The existing 38-test `text-engine` result is host-only, existing-core evidence. It is not a Cellbox dependency probe, target build, image boot, GUI-terminal proof, or full-workspace qualification.
- No source/ABI edit, download, signing, image mutation, QEMU run, or lock release is established by these four artifacts.

## Honest shipment boundary

After the corrections above, the four documents can ship as a **Phase 01 preparation packet**:

- `command-catalog.md`: partial observed source/caller/package census.
- `contract-deltas.md`: candidate boundary and retained-gate inventory, with no ratified bytes or digest.
- `dependency-review.md`: local negative-evidence and runtime-constraint inventory, not an upstream availability or feasibility verdict.
- `baseline.md`: session-local dirty-tree and existing host-suite observation at the recorded HEAD, not reproducible Cellbox qualification.

They cannot honestly ship as Phase 01 completion, CBX-01/02 passage, CP1/CP2 approval, source admission, selected WIP, lock release, or permission to begin P02.

Overall correctness: incorrect  
Explanation: The packet is largely careful about its preparation-only ceiling, but it contains inaccurate approval/status wording, an incomplete desktop caller census, a test-history contradiction, an unsupported license blocker, and non-durable evidence claims. With those corrected, it is suitable only as incomplete preparation.  
Confidence: 0.99