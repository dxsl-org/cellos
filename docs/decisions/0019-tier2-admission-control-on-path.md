# ADR-0019 — One Tier 2 admission control, on the path

> **Status**: Accepted 2026-09-22.
> **Supersedes**: None. Amends Spec 22 §4 and the Tier 2 statements in
> `docs/app-development-guide.md`.

## 1. Context

Three documents and the code currently describe **different** controls for the same decision.

| Source | What it says |
|---|---|
| `docs/specs/22-native-domain-cell-implementation-gate.md:§4` | "A separate boot-provisioned `native-domain-admission` policy is default-off and is the **sole persisted enablement control**", with a `DRAINING` emergency rollback |
| `kernel/src/loader/domain_admission.rs` | The policy module exists and is tested (`S22-RV64-ADMISSION-DENY`, `S22-RV64-ADMISSION-DRAIN`), but it is `#![allow(dead_code)]`, "internal-only", and states "No public loader or manifest path constructs a request" |
| `kernel/src/loader/governed_spawn.rs:60-81` + `kernel/src/task/launch.rs:182-221` | The **live** route: a signed cell whose `protection_class` is `FFI` or `UNTRUSTED`, or any unsigned cell, sets `is_domain` and creates a private `AddressSpace`. It never consults the policy |
| `kernel/Cargo.toml:83` | `native-domains` is in the kernel's **default** features |
| `docs/app-development-guide.md` | "Tier 2 … internal test-hooks-only, with no application admission/loader route" |
| `docs/roadmap/current-focus.md` | "Tier 2 GGML (CP-2) stays blocked on the Tier 2 admission route" |
| `docs/app-tier-acceptance-matrix.md` | "Tier-2 admission … remain blocked" (a claims/qualification statement) |

Verified state of the code: `evaluate_domain_admission` has **no caller** outside its own
module; the only invocation in the tree is the `test-hooks` selftest
(`kernel/src/main.rs:1029`). The policy therefore gates nothing, while the class-based route is
active by default — including on AArch64 and x86_64, where the policy module would refuse
admission outright because it is RV64-gated
(`domain_admission.rs`, `DomainAdmissionDenial::UnsupportedArchitecture`).

Consequences of the divergence: a reviewer cannot tell which mechanism gates Tier 2; the
emergency drain that Spec 22 §4 promises cannot be invoked; the application guide tells
developers Tier 2 is unavailable while `ADR-0017` ships an application to it; and
Spec 24 CP-2 (Tier 2 GGML) is recorded as blocked on a "route" that is in fact live but
undocumented.

## 2. Decision

### 2.1 There is exactly one control, and it is on the path

The admission decision for a Tier 2 launch is taken by a single policy object consulted by the
code that creates the domain, before any task or domain is published. The loader must hold a
policy generation lease across every fallible step of admission and re-check it before
publication, so a concurrent `DRAINING` linearizes either before or after the admission —
never inside it. A denial is final: it never falls back to SAS.

### 2.2 The cargo feature selects capability, never admission

`native-domains` means "this build contains a qualified domain backend". It is not an
admission decision, and a build with the backend but without an enabled policy denies
domain-class artifacts rather than admitting them anywhere.

### 2.3 The default posture is explicit, per build profile

- Development/G1 builds **enable** Tier 2 admission explicitly, which preserves today's
  observable behaviour (unsigned and `FFI`/`UNTRUSTED` cells run in private domains) and makes
  it a stated posture instead of an accident of a default feature.
- Fleet-secure profiles must state the policy explicitly; an absent policy is fail-closed.
- `DRAINING` remains a boot-local, one-way rollback that rejects new admissions while existing
  domains drain. It is never a silent downgrade.

### 2.4 The control's predicates must match the route it gates

Architecture coverage, eligible artifact classes, resource quota, copied-IPC readiness, and
the enforceable-capability ceiling must describe the same set of launches the route supports.
A policy that is RV64-only cannot be the control for a three-architecture route: either its
architecture gate is widened to the route's coverage, or AArch64/x86_64 Tier 2 is declared
unsupported by policy and the route refuses it. The two must not disagree silently.

### 2.5 Documentation is part of the decision

The application guide, Spec 22 §4, the acceptance ledger, and the roadmap are updated in the
same change that wires or removes the control. Until the wiring lands, the documents state
that Tier 2 routing is class-based and the policy is not on the path — they do not describe a
control that does not exist.

### 2.6 Tier 2 is the intended landing zone for contained native applications

Subject to §2.1–§2.4, Tier 2 application deployment (ADR-0018 porting lanes, ADR-0017 Ocel,
Spec 24 CP-2) is the intended destination. Qualification claims (`PASS`) remain gated by the
acceptance ledger and its negative tests; this ADR authorizes the route, not the claim.

## 3. Rejected alternatives

- **Keep the class-based route as the only control.** It has no emergency drain, no quota or
  artifact-eligibility predicate, no copied-IPC readiness check, and no capability ceiling;
  it contradicts Spec 22 §4, whose gate list is the reviewed security contract. Rejected.
- **Leave both mechanisms in place.** A documented policy that gates nothing plus a live route
  that is documented nowhere is the current defect, not a design. Rejected.
- **Make the policy default-off everywhere.** Under ADR-0015 an unsigned cell must be contained
  (Tier 2) or denied — never admitted to SAS. Default-off would deny every unsigned artifact in
  development lanes, breaking the shipped dev posture and the FFI/UNTRUSTED cells
  (`doom`, `tetris-c`, `posix-shim-test`, `tier2-smoke`) without adding safety. Rejected.
- **Delete the policy module and document the feature + class as the control.** Cheapest
  option, but it discards the tested `DRAINING` linearization and the predicate set that
  Spec 22 §4 requires, and leaves no way to stop new domain admissions without a reboot.
  Rejected.
- **Gate admission in the installer/manifest (manifest v3 execution class).** Spec 22 §2.7
  already deferred manifest-v3 execution classes until the runtime mechanism is proven, and
  this ADR does not reopen that. Rejected for now.

## 4. Consequences

- One place to look, one place to test, one place to drain.
- The `native-domains` feature stops being a de-facto admission switch; images that relied on
  the default feature must state their admission posture.
- Existing Tier 2 integration tests (`tests/integration/tests/tier2_fault_isolation.rs`,
  `aarch64-boot.rs`, `x86_64-boot.rs`) become the positive/negative corpus for the wired
  control: they must pass with the policy enabled, and the drain case must be witnessed by an
  explicit denial test.
- The application guide's Tier 2 paragraph is replaced by the actual gate: eligible artifact
  class, admission posture, and quota — not "not available".
- Spec 24 CP-2 and ADR-0017's Ocel gain a defined route to depend on instead of an undefined
  one.

## 5. Cross-references

| Topic | Document |
|---|---|
| Tier 2 implementation gate (amended §4) | `docs/specs/22-native-domain-cell-implementation-gate.md` |
| Tier definitions and admission truth | `docs/specs/18-cell-trust-tiers.md` |
| Tier 2 mechanism as shipped | `docs/specs/19-hardware-isolation-layers.md` (Layer B) |
| Portability strategy and profiles | `docs/decisions/0018-cell-native-portability-and-runtime-profiles.md` |
| Dual-mode hybrid architecture | `docs/decisions/0015-dual-mode-hybrid-architecture.md` |
| Application guide (Tier 2 paragraph) | `docs/app-development-guide.md` |
| Qualification claims | `docs/app-tier-acceptance-ledger.json`, `docs/app-tier-acceptance-matrix.md` |
| Implementation plan | `.agents/260922-1549-cell-native-portability-program/plan.md` (phase 01) |
