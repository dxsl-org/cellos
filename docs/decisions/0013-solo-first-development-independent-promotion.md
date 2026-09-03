# ADR-0013: Use solo-first development with independent claim promotion

- **Status:** Accepted
- **Date:** 2026-09-03

## Context

Cellos currently has one active maintainer who may design, implement, execute,
review, and merge a development change. Existing plans sometimes express those
activities as separate roles, such as Evidence Runner, Ledger Steward, and
Independent Reviewer. A role describes a responsibility; it does not by itself
create another accountable actor. Assigning several role names, accounts,
aliases, AI agents, or subagents to work controlled by one person does not make
the work human-independent.

Automation has a different purpose. AI and subagents may analyze, implement,
challenge, or summarize work. CI may reproducibly build, test, validate policy,
and retain artifacts. These are automated assurance mechanisms. They can
increase confidence and expose defects, but they are not accountable human
identities and cannot approve their own output or satisfy a requirement for an
independent human decision.

A claim is promoted when repository language or governed state raises its
assurance class: for example, from diagnostic to qualifying evidence, from
`DEV_REFERENCE` to independently ratified, or from development evidence to a
production or external claim. Promotion is distinct from doing the work,
merging technically valid development changes, or recording evidence at its
truthful ceiling.

The AArch64 semihosting run illustrates the ambiguity. The local run established
diagnostic QEMU behavior, while [Issue #47](https://github.com/dxsl-org/cellos/issues/47)
and its Phase 01 plan treated three separately named roles as three distinct
accountable identities. Subagents in one operator session did not meet that
requirement, so no independent ratification occurred. The useful technical fact
and the missing independent decision must remain distinguishable. That ledger
promotion gate must not become a non-causal global block on unrelated POSIX,
kernel, QEMU, or local-runtime development.

This decision refines the lane separation established by
[ADR-0007](./0007-development-first-hardware-constrained-execution.md):
development may advance to the exact evidence ceiling supported by its actors,
assets, and execution environment, while independently ratified and production
claims remain fail-closed.

## Decision Drivers

- Permit one maintainer to perform all execution and ownership roles needed for
  development when the applicable technical gates pass.
- Preserve accountable-human independence wherever a claim explicitly requires
  independent promotion.
- Distinguish role separation from actor separation and automated assurance.
- Prevent unavailable reviewers or ledger ratification from serializing work
  that has no causal dependency on the promoted claim.
- Preserve exact host, QEMU, physical-device, service, external, and production
  evidence ceilings.
- Make independent decisions explicit, attributable, proposal-specific, and
  auditable in the repository's canonical collaboration system.
- Preserve Law 1's deliberate two-checkpoint protection for public interface and
  ABI changes without inventing a second maintainer.
- Honor external standards, contracts, or release rules that explicitly require
  additional parties.

## Considered Options

### Option A (chosen): Solo-first development with independent claim promotion

One maintainer may hold all development roles and merge work after its technical
gates pass. AI and CI provide assurance but no accountable-human independence.
A distinct repository member is required only when raising a claim that
explicitly requires independent ratification, production approval, or external
approval.

- **Pro:** Keeps useful development moving and makes the real human trust
  boundary explicit.
- **Pro:** Preserves fail-closed promotion and exact evidence ceilings without
  creating fictitious independence through role labels or automation.
- **Con:** Some qualifying, production, or external claims can remain blocked
  after the implementation and evidence are otherwise complete.
- **Con:** Documentation must state both what was proved and what higher claim
  remains unavailable.
- **Chosen because:** It separates causal technical gates from accountable claim
  promotion while retaining both.

### Option B: Require a different person for every named development role

Evidence Runner, Ledger Steward, implementer, reviewer, and merger would always
be different people, even for development-only work.

- **Pro:** Maximizes visible separation of duties.
- **Con:** Treats role count as actor count and makes repository throughput
  depend on people who may not be available.
- **Con:** Blocks unrelated implementation and evidence collection even when no
  independently promoted claim is being made.
- **Rejected because:** Universal personnel separation is not causally necessary
  for truthful development evidence and would contradict ADR-0007's lane-local
  gating. The consequence would be a largely idle project without stronger
  evidence for the blocked development lanes.

### Option C: Count AI, subagents, or CI as independent reviewers

Automated agents or CI jobs would fill separately named roles and their approval
would be recorded as independent ratification.

- **Pro:** Produces fast, repeatable analysis and apparent separation.
- **Con:** Automation has no independent human accountability and may share the
  same prompt, operator, source, permissions, assumptions, or failure modes.
- **Con:** A green check or generated review does not express a repository
  member's informed acceptance of an exact proposal and evidence set.
- **Rejected because:** This would relabel automated assurance as human
  independence. The consequence would be unsupported promotion of claims that
  explicitly require an accountable external decision.

### Option D: Let the sole maintainer promote every claim

The maintainer could implement, execute, merge, independently ratify, and make
production or external claims without another repository member.

- **Pro:** Removes all reviewer-availability delays.
- **Con:** Eliminates the control that independent ratification is intended to
  provide and permits self-approval of the exact evidence and governance
  mechanism under review.
- **Con:** Cannot satisfy external standards or agreements that mandate more
  than one accountable party.
- **Rejected because:** Development authority is not authority to waive an
  explicit independence requirement. The consequence would be fail-open
  promotion and ambiguous production assurance.

## Decision

Cellos adopts **solo-first development and independent claim promotion**.

1. **Separate roles from actors.** A role is a set of duties. An actor is the
   accountable identity performing them. One maintainer may be the implementer,
   Evidence Runner, Ledger Steward, development reviewer, and merger. Naming the
   roles separately does not imply actor independence.
2. **Treat AI and CI as automated assurance only.** AI, subagents, bots, CI jobs,
   test runners, and policy validators may produce analysis, changes, checks,
   and retained evidence. They never count as an accountable human, an
   independent repository member, or an independent approval.
3. **Allow solo development and merge.** The sole maintainer may implement and
   merge development work after all applicable technical, test, evidence,
   safety, and lane-local governance gates pass. No second person is required
   merely because a plan names multiple execution or ownership roles.
4. **Stop each result at its truthful evidence ceiling.** Host work remains host
   evidence; QEMU work remains software/QEMU evidence; exact-device exercise
   applies only to the exercised device; service evidence applies only to the
   authorized service context. Solo execution, AI review, CI success, or merge
   does not upgrade any of those ceilings or imply independent, physical,
   external, admissible, release, or production evidence.
5. **Define promotion explicitly.** A change requires independent promotion only
   when it would claim independent ratification, production readiness or
   admission, release approval, or an explicitly external assurance class. The
   absence of that approval blocks the higher claim and any state transition
   whose contract requires it; it does not invalidate truthful lower-ceiling
   evidence or block non-causal development lanes.
6. **Require one distinct repository member for independent promotion.** The
   approving member must be a human repository member distinct from the
   maintainer responsible for the proposal and evidence. Unless an external
   rule requires more parties, that one distinct member supplies the independent
   decision; the development roles need not be redistributed among additional
   people.
7. **Accept independent decisions only through GitHub.** A required independent
   member decision is valid only as an explicit `YES` or `NO` response in a
   GitHub issue or pull request, attributable to that member and bound to the
   exact proposal, commit or tree, and evidence under decision. `YES` authorizes
   only the named promotion; `NO` rejects it. Silence, timeout, reactions,
   inferred assent, AI or subagent output, CI status, alternate aliases, email,
   and chat do not count. Material changes to the bound proposal, source, or
   evidence require a new decision.
8. **Use two owner checkpoints for Law 1.** A Law 1 public interface or ABI change
   requires two explicit owner confirmations at separate checkpoints: first,
   confirmation of the exact proposed interface and compatibility consequences
   before implementation; second, confirmation of the implemented interface,
   migration impact, and bound verification evidence before it is accepted.
   The sole maintainer may provide both confirmations in that accountable owner
   capacity, but one message cannot satisfy both checkpoints and neither
   confirmation constitutes independent promotion.
9. **Apply the rule to AArch64 Issue #47.** The sole maintainer may perform the
   Evidence Runner and Ledger Steward duties, implement the append-only
   correction mechanism, and merge technically passing development changes.
   Existing or fresh artifacts remain diagnostic QEMU evidence until the exact
   evidence and correction proposal receive an explicit GitHub `YES` from one
   distinct repository member. Without that decision, the ledger must not be
   described as independently ratified or promoted. A `NO` keeps that promotion
   blocked. Unrelated lanes may proceed to their own truthful ceilings; their
   predecessor and phase-local technical gates remain unchanged.
10. **Preserve stronger external requirements.** If a law, standard, customer
    contract, platform policy, or release rule explicitly requires more people,
    different roles, or another approval mechanism, that requirement remains an
    external gate. This ADR does not reduce or reinterpret it.

## Consequences

### Positive

- Development execution and merge are not stalled solely by the number of
  available maintainers.
- Responsibility is auditable because role names no longer masquerade as
  independent actors.
- AI and CI remain useful assurance layers without being misrepresented as
  accountable humans.
- A missing independent reviewer blocks only the independently ratified,
  production, release, or explicitly external claim that needs the reviewer.
- AArch64 semihosting diagnostics and unrelated follow-up lanes can be described
  and advanced accurately without falsely closing the acceptance ledger.
- Law 1 retains two deliberate owner decisions while remaining operable in a
  solo-maintainer project.

### Negative / Risks

- A development change may be implemented and merged while its higher assurance
  claim remains unavailable; readers must not infer promotion from merge state.
- The distinct repository member can become a bottleneck for claims that truly
  require independent approval.
- Proposals must retain exact commit/tree and evidence bindings so a later
  `YES` or `NO` cannot be applied to changed work.
- The maintainer must keep owner confirmation, automated assurance, technical
  acceptance, and independent promotion visibly separate.
- Some external regimes may still require multiple people and may prevent
  production promotion despite complete development evidence.

## Review Rule

A process or claim violates this decision if it:

- requires multiple people merely because development duties have multiple role
  names, without an explicit independent-promotion or external requirement;
- treats AI, a subagent, bot, CI job, alias, or repeated action by one person as
  an independent accountable identity;
- promotes a claim without the required GitHub `YES` bound to the exact
  proposal, commit/tree, and evidence, or treats silence, a reaction, email, or
  chat as approval;
- lets a missing independent decision block unrelated work below its truthful
  evidence ceiling;
- infers physical, external, production, release, or independent assurance from
  solo execution, automation, CI success, or merge; or
- uses this ADR to bypass a stronger multi-party external requirement.

Review this decision when Cellos gains another active maintainer, adopts branch
protection or a release system that enforces accountable approvals, or becomes
subject to an external standard or contract with explicit separation-of-duty
rules. Any replacement must preserve historical claim/evidence bindings and may
strengthen future promotion gates without retroactively upgrading evidence.

## Links

- [ADR-0007: Use development-first hardware-constrained execution](./0007-development-first-hardware-constrained-execution.md) — lane-local execution and truthful evidence ceilings remain authoritative.
- [Issue #47: Ratify independent AArch64 semihosting evidence](https://github.com/dxsl-org/cellos/issues/47) — the concrete independent-promotion gate clarified by this decision.
- [Current focus: development-first, solo-first execution boundary](../roadmap/current-focus.md#development-first-solo-first-execution-boundary) — current diagnostic evidence and the affected follow-up lanes.
- [Project roadmap: capability lanes](../project-roadmap.md#capability-lanes) — AArch64 ledger promotion and independently executable follow-up routing.
