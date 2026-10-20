---
phase: 4
title: "One cellbox executable and complete caller/image migration"
status: pending
priority: P1
tier: thinking
dependencies: [1, 2, 3]
---
# Phase 04: Multicall cutover

## Overview and requirements

Deliver CBX-09..12 and CBX-14: one essential-suite artifact, real serial/GUI shell consumers, shared-backend behavior and no increased ambient authority. All P01 catalog/launch/identity approvals precede switching startup paths.

## Architecture and related files

Create `cells/tools/cellbox` with registry, shell module (`sh`), shared utility modules and applet entries. Migrate `cells/tools/shell` and selected sources from `cells/tools/sys-tools`/`net-tools`; reuse pure `libs/text-engine` logic. Update `Cargo.toml`, init launches, kernel launch profiles under their owner, scripts/image/sign lists, terminal launcher and every caller found in the catalog. Keep independent servers/workers/apps and related packages if still consumed.

## Implementation steps

1. Move rather than copy selected implementations into shared modules, retaining input/output/status behavior or the explicitly migrated contract. Keep cwd/variables/source/jobs/control flow inside the shell context.
2. Expose canonical `cellbox <command>` and `cellbox sh`; implement selected shell short-name resolution through the catalog. Preserve meaningful grammar aliases without forwarding ELF wrappers, basename assumptions or a second cellutils product.
3. Enforce P01's authenticated caller-edge × backend-right matrix under the common ELF ceiling: every command's file read/mutate, lifecycle/device, network/DNS, telemetry, credentials/key/trust and other rights need a real attenuating service handle or separately authorized worker. Exercise known privileged commands from restricted callers, not just unknown/forged modes. If a right cannot be enforced narrowly, keep its effect behind a separate worker/boundary rather than widening all invocations. An argv command/mode is never authorization.
4. Connect GUI Terminal to actual `cellbox sh`/foreground streams; prove real changing file/process/time output, two-session independence and the removal of fixed responses. Migrate init/foreground identities, argv/cwd/environment, path policy and image/sign lists as one reviewed set. Use admitted disposable candidate images for implementation evidence, then obtain each affected delta's CP2 from its exact diff/caller census/evidence before final caller/image retirement. Pending/blocked CP2 stops rollout; P05 later completes Ratatui/ANSI emulation, not the first real shell connection.
5. Update all affected retained image recipes, including compatibility maintenance of existing non-Intel build references if touched; this is not new platform qualification. Packaging must fail on build failure or missing fresh artifacts rather than embedding stale binaries.
6. Remove obsolete suite Cargo targets, source duplicates, wrapper binaries, dead paths/comments and incidental old wording tests after migrated behavior passes. Retain unrelated tools and standalone server modes with their own real lifecycle.
7. Boot private signed x86 images with desktop disabled and enabled. Exercise shell state/scripts, explicit/short invocation, real VFS/network/system observations, GUI shell sessions, external app launch and denied authority. Demonstrate each cataloged desktop backend operation through its corresponding CLI/shared typed service path; presentation-only window gestures are not new CLI business operations. Capture the artifact list and catalog coverage.

## Success criteria

- [ ] Every catalog entry/caller is migrated or explicitly retained outside the suite; no silent command loss.
- [ ] Shell state transitions and external byte pipelines behave correctly after the new startup identity/path is used.
- [ ] Every cataloged ungranted right is denied for known commands under restricted caller edges, including network/DNS, telemetry and secret/key/trust access; final cutover has all affected CP2 approvals.
- [ ] One suite executable is packaged; no fallback to an obsolete shell/utility artifact; independent apps/services still work.
- [ ] Headless boot/data commands need no viUI/compositor; every selected desktop backend operation has a demonstrated CLI/shared-service path, not a desktop-only implementation or CLI-table parser.
- [ ] GUI Terminal runs the real migrated shell before final path/image retirement; fake responses and the private command matcher are gone.

## Verification

Run behavior-focused existing affected tests plus the new path smoke once after edits. Compare signed artifact hashes with packaged bytes; compilation alone is not cutover proof. Status and actual mutations must be observed, not just command-help output.

## Assumptions

The approved suite can operate through safe scoped service contracts under one artifact ceiling. Verify the exact backend before merging a privileged command. If not, stop for the specific boundary decision; do not silently omit it or widen rights.

## Security and risk assessment

Path/caller identity is load-bearing in current policy. A separate invocation alone does not supply hardware isolation. Before rollout rollback is a matched previous image/policy plus cold boot; after file or external effects, preserve state and audit outcomes rather than replay. Do not introduce permanent shims to ease rollback.

## Deviation log

None; implementation pending. Log any non-reversible/contract-changing divergence for owner decision.
