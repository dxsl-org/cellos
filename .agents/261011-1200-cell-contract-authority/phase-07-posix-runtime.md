---
phase: 7
title: "Optional POSIX personality and native C/C++ runtime"
status: pending
priority: P1
tier: thinking
dependencies: [4, 5]
---

# P07 — POSIX outside native core

## Deliverable and requirements

AC-19. Native boot/services/apps work without POSIX linkage or a POSIX broker. Existing selected C/C++ and mlibc profiles retain their declared behavior through explicit optional local adapters and domain-selected native services.

## Related files / ownership

- `libs/api/src/{lib.rs,services.rs,services/posix.rs,services/posix/}`; manifests/features found by P01.
- `libs/port-platform/`, `libs/mlibc-shim/`, `third_party/mlibc/sysdeps/vicell/`.
- `cells/services/ocel-quickjs/src/ffi.rs`, selected C/C++/mlibc/pthread test consumers and generated C symbol contract tools.
- `kernel/src/task/syscall.rs`, `kernel/src/fs.rs`, loader BootFS path; SDK VFS/NET/native stream clients.
- Build/image configuration and native runtime documentation; new optional runtime/personality package names fixed by P01 rather than speculative crate proliferation.

## Implementation steps

1. Census every automatic Tier-A symbol consumer, mlibc suppression feature, fd syscall/pipe user and BootFS dependency. Freeze the declared compatibility subset per profile; C/C++ is not inherently POSIX and source compatibility is not Linux binary ABI.
2. Extract or rehome C ABI allocation/string/math/C++/TLS glue into explicitly linked runtime. Make API schemas independent of that symbol provider. All affected consumers select one runtime; prevent duplicate libc allocation/free ownership and symbol providers.
3. Keep errno/TLS, FILE/jmp_buf/pthread layout, allocator and fd-number map local to each client. Services receive typed resources, never client pointers to ABI objects.
4. Route filesystem/network/process operations through the client's granted native services and scoped lifecycle authority. A personality Cell implements only the process/job/session subset required by selected profiles; unsupported operations stay explicitly unsupported, not counterfeit successful no-ops.
5. Migrate mlibc direct-kernel fd/time/TLS/futex paths to approved neutral primitives/service clients, including real wait/wake and context-managed TLS where required. Fix runtime allocation lifetime for the selected workload; do not hide unsupported behavior behind bump-only leaks.
6. Migrate loader/VFS boot reads to the retained minimal bootstrap reader or approved native service path without a boot dependency cycle. Remove legacy global fd syscall implementation only after every consumer is migrated; frozen removed numbers stay reserved according to ABI policy. Do not remove TLS/futex/stream mechanisms solely because libc uses them.
7. Update generated symbol contract, headers, packaging, runtime profile docs and all callers in one cutover. Preserve Tier-2 requirement for FFI; no SAS fallback if runtime admission fails.

## Verification / success criteria

- [ ] Build and boot an Intel native-only image; symbol/dependency inspection plus real file/network/lifecycle interaction prove no POSIX provider linked/started.
- [ ] Separate compatibility image runs existing cpp-smoke, mlibc-smoke and multithreaded native C profile using declared APIs; two Cells using the same fd numbers do not share resources or errno/TLS.
- [ ] A native C/C++ caller exercises C2C without the POSIX personality. Personality crash affects only its compatibility domain, not native VFS/NET/init.
- [ ] Negative unavailable-service/stale-resource/permission cases map to documented C errors without widening authority or blind replay.
- [ ] All replaced direct kernel fd callers removed and boot recovery still works; migration ledger includes generated bindings, tests and docs.

Correction 2026-10-16: prior checkmarks and cited standalone-model/storage-boot receipts did not establish these criteria. Acceptance remains pending; historical observations are not runtime qualification.

## Security and rollback

Optional linkage must not bypass authority or admit unsafe C into Tier 1. A POSIX personality is not global root and cannot grant more than its domain ceiling. Cold-boot the known-safe complete image on rollback; never mix old libc with new ABI allocator/handle layout. Application files already changed remain changed; binary revert is not data rollback.

## Assumptions and deviation log

QuickJS currently relies on ambient API symbols, and mlibc has separate assumptions; both require explicit migration even if not the demonstration workload. Full signals/fork/dynamic linking/hosted C++ are not part of this phase without a separately approved profile. Deviation log: none.
