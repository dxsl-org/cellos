# ADR-0003: Standardize application tiers, runtime profiles, and SDK modules

**Date**: 2026-08-19
**Status**: Accepted
**Deciders**: Cellos maintainer

## Context

Cellos docs used `tier`, `layer`, and suffixes such as `1b`/`3b` for several
different concepts: execution isolation, runtime language profile, SDK
ergonomics, POSIX coverage, and product-stage roadmap. That made the app model
hard to explain and caused a direct conflict with Spec 18, where Tier 2 is the
future native MMU domain, not merely "unsigned code".

The codebase also has a Manifest v2 field named `tier`. In current code that
field is an x86 PKU protection-key request in
`libs/api/src/abi/manifest_flags.rs`; it is not the same concept as the
product-facing application execution tiers in this ADR. Renaming that field
directly would risk ABI churn without changing the actual Tier-2 mechanism.

The documentation also had a historical Silo classification as a VM tier, a
WASM "tier" discussion, and numbered SDK labels such as SDK L1/L2. Leaving
those terms mixed together would keep producing contradictory guidance for
which app path to use: Rust `no_std`, planned Rust `std`, C/POSIX, Lua, Tier 2
native domains, or Linux guests.

## Decision Drivers

- Keep `tier` reserved for execution and isolation boundaries.
- Keep runtime/toolchain choices separate from containment mechanisms.
- Preserve Manifest v2 ABI compatibility and historical guide links.
- Make SDK documentation useful without overloading tier numbers.
- Keep product roadmap stages G1-G5 independent from application tier names.

## Considered Options

### Option A: Keep the existing tier/layer vocabulary

This option would leave Tier 1, Tier 1b, Tier 2, Tier 3b, SDK L1/L2, Silo
layers, and roadmap stages as partially overlapping terms.

- **Pro**: No migration cost for existing docs or code comments.
- **Pro**: Existing filenames and historical references remain literal.
- **Con**: The same word continues to mean isolation class, language profile,
  POSIX coverage, SDK depth, and roadmap maturity.
- **Rejected because**: It preserves the exact ambiguity that caused Tier 2 to
  be misremembered as "Tier 1 but unsigned" instead of a private-page-table
  native domain.

### Option B: Keep Tier 1b and Tier 3b as first-class execution tiers

This option would define C/POSIX, Lua, and Linux guests as their own numbered
tier variants beside Tier 1, Tier 2, and Tier 3.

- **Pro**: It matches older guide titles and many roadmap/research references.
- **Pro**: It gives each user-facing app route a short label.
- **Con**: C/POSIX and Lua do not add a new execution boundary; they run as
  trusted Tier-1 runtime profiles.
- **Rejected because**: It makes language/runtime choice look like a memory
  containment decision and conflicts with Spec 18's three execution tiers.

### Option C: Use `layer` for app classes

This option would rename the user-facing app classes to application layers and
avoid overloading the existing Manifest `tier` field.

- **Pro**: It avoids direct name collision with Manifest v2 `tier`.
- **Pro**: It sounds natural when discussing stacked SDK pieces.
- **Con**: Cellos already uses `layer` for internal architecture and hardware
  isolation layers, including Spec 19 Layer A/B/C.
- **Rejected because**: It moves the ambiguity from `tier` to `layer` and makes
  hardware-isolation docs harder to read.

### Option D (chosen): Three execution tiers plus runtime profiles and SDK modules

This option reserves Tier 1/2/3 for execution boundaries, makes Tier 1b and
Tier 3b legacy aliases, and uses runtime profiles for Rust `no_std`, planned
Rust `std`, POSIX/FFI, Lua, WASM-hosted workloads, and Linux guests.

- **Pro**: It matches Spec 18 and Spec 19: Tier 2 is a native MMU domain, not a
  signature status.
- **Pro**: It keeps Rust `std`, C/POSIX, Zig, Lua, and Linux as runtime/profile
  choices rather than containment claims.
- **Con**: Existing code and docs still contain the old names during migration.
- **Chosen because**: It separates the load-bearing isolation mechanism from
  language/runtime and SDK packaging choices.

## Decision

We decided on **three execution tiers plus runtime profiles and SDK modules**
because it keeps containment, language/runtime, SDK ergonomics, and roadmap
maturity as separate axes.

| Tier | Canonical name | Meaning | Current status |
|---|---|---|---|
| Tier 1 | Trusted SAS Cell | Trusted native Cell in the shared single address space. LBI and signing/admission policy are load-bearing. | Shipped for `no_std` Rust; Rust `std` is planned; FFI/POSIX/Lua are trusted runtime profiles, not separate tiers. |
| Tier 2 | Native Domain Cell | Native Cell in a private MMU protection domain. Same Cell shape and native speed class, copied/domain-explicit IPC. | Accepted design, not implemented. Do not present it as available containment. |
| Tier 3 | VM Guest | Whole guest OS in a VM, normally Linux, behind Stage-2/hypervisor isolation. | ARM64 path exists; platform coverage and persistence remain tracked elsewhere. |

Use **runtime profile** for language/runtime variants inside a tier:

- `rust-no-std` profile: current Tier 1 default.
- `rust-std` profile: planned G4 pure-Rust PAL/custom target work; not `std` over mlibc.
- `ffi-posix` profile: C/C++/Zig linked into a trusted Tier 1 Cell through the
  POSIX shim or mlibc. Historical `Tier 1b` means this profile.
- `lua` profile: Lua interpreter Cell as a trusted Tier 1 runtime profile.
- `linux-guest` profile: Tier 3 guest profile. Historical `Tier 3b` means this.

Use **SDK module** or **SDK layer** for developer API packaging. The SDK is not
numbered by tier. Canonical SDK grouping is Foundation, Runtime profiles,
Service clients, UI/graphics, Middleware/helpers, Tooling, and Guest integration.

Use **Stage G1-G5** only for product roadmap overlays. Stages do not rename the
application tiers.

## Consequences

### Positive

- Tier 1, Tier 2, and Tier 3 now describe only execution and isolation
  boundaries.
- Tier 2 remains tied to its actual mechanism: private page tables and explicit
  cross-domain sharing.
- The SDK can grow by named modules without inventing SDK tiers.
- Silo is documented as a Tier-1-facing hardware-backed capability, not a VM
  tier.

### Negative / Risks

- Existing code still exposes `CellManifest.tier` and `TIER_*` names that now
  conflict with product-facing app tiers.
- Historical docs, research notes, changelog entries, and guide filenames still
  contain `Tier 1b`, `Tier 3b`, and SDK L1/L2 language.
- Future contributors may still confuse signature/admission state with Tier 2
  until the code-level aliases make the distinction visible.

### Neutral

- No Manifest v2 ABI change is made by this decision.
- `Tier 1b` remains an allowed legacy alias when referring to old filenames or
  historical text; new normative text should say Tier 1 `ffi-posix` or Tier 1
  `lua`.
- WASM is not an execution tier. It may be discussed as a runtime/tool-hosting
  path, but not as a first-class containment class.
- Full Rust `std` remains a planned Tier 1 runtime profile under G4.

## Related Decisions

- Related: `docs/specs/05-application.md` — user-facing app taxonomy.
- Related: `docs/specs/18-cell-trust-tiers.md` — canonical trust/admission and
  Tier-2 decision.
- Related: `docs/specs/19-hardware-isolation-layers.md` — Layer A/B/C hardware
  mechanism taxonomy.
- Related: `docs/app-development-guide.md` — developer-facing route selection.
- Related: `docs/project-roadmap.md` — G4 Rust `std` and G1-G5 product-stage
  terminology.
