# Cellbox plan review

## Scope

Review covers accepted ADR-0028, the public Cellbox design, ten-phase implementation plan and CBX-01..CBX-29 ledger. It checks decision fidelity/excluded scope and architecture/security boundaries. Parent owns all integration edits; review agents are read-only and performed no builds/tests/source changes.

## Validation rules

- The maintainer has approved architecture/documentation/planning, not current code execution or concrete ABI bytes.
- One executable does not imply one process, shared objects across Cells, per-subcommand rights or mandatory inclusion of independently installed applications.
- Ratatui is selected for new first-party TUI; Terminal SDK/backend remain reusable outside cellbox and viUI.
- Required editor/search/top/SSH behavior cannot be silently reduced when a dependency or external fixture is unavailable.
- No IDE, language-server/build environment or general application-development program is admitted.
- All implementation phases and acceptance rows remain pending; only documentation checks are recorded in this delivery.

## Findings and parent dispositions

Two automated reviewer runs returned ten findings against the draft (six P1, four P2). All ten were accepted and integrated as documentation corrections below. Their original negative draft verdicts are not represented as a later reviewer approval; the parent owns these dispositions and final artifact checks. No independent human ratification or runtime evidence is claimed.

| ID | Lens / priority | Gap | Correction and verification owner |
|---|---|---|---|
| S1 | Scope / P1 | Final artifact cutover preceded real GUI shell bridge | P04 now proves/migrates GUI shell streams with init/identity/image paths before retirement; P05 only completes Ratatui/ANSI. CBX-14 belongs to P04. |
| S2 | Scope / P1 | CLI-first could pass without desktop operational parity | P01 inventories selected desktop backend operations and their CLI/shared-service paths; P04/CBX-12 demonstrate each. Presentation-only gestures do not create unrelated CLI scope. |
| S3 | Scope / P2 | P03 acceptance required producers implemented later | CBX-08 now covers generic real byte/frame infrastructure; actual monitoring frames and SSH raw streams are owned by P06/CBX-18 and P09/CBX-23. |
| S4 | Scope / P2 | No negative proof for selective TUI | Added CBX-29, P05/P10 scenarios and coverage: one-shot/piped commands stay non-full-screen/non-interactive, unsupported explicit TUI requests fail truthfully. |
| S5 | Scope / P2 | Headless SSH depended on Ratatui/GUI phase | P09 depends on P02/P04 only; GUI SSH integration is verified in P10 after P05. |
| B1 | Boundary / P1 | CP1 and CP2 gate sequencing ambiguous | Plan/P01/design specify per-delta CP1 digest before edits, admitted disposable evidence, exact diff/caller census and CP2 before final rollout; P10 audits closure. |
| B2 | Boundary / P1 | Rights checks covered only some privilege classes | P01/P04/CBX-11 require authenticated caller-edge × backend-right matrix, including network/DNS, telemetry and secret/key/trust access, with negative known-command tests. |
| B3 | Boundary / P1 | Inherited/duplicated/descendant endpoint lifetime unspecified | P01/P02/design/CBX-03/05 specify explicit non-default typed delegation, last-writer EOF, generations and operation-subtree revocation/wakeup without unrelated-session effects. |
| B4 | Boundary / P1 | Save As destination could race overwrite confirmation | P01/P08/design/CBX-22 require confirmed handle/version-bound conditional create/replace and existing/concurrently created/changed destination failure cases, preserving bytes and dirty state. |
| B5 | Boundary / P2 | Mixed pipelines and jobs/state semantics under-specified | P01/P03/CBX-09 freeze job/context/status/foreground/wait/cancel semantics; unsupported forms are rejected before any effects rather than silently extending or ignoring syntax. |

## Documentation validation

The saved-artifact validator checks local links/anchors, D01–D11 numbering/coverage, CBX-01..CBX-29 ownership and pending states, phase-table/frontmatter agreement, acyclic dependencies, phase length and no excluded application example. Runtime commands listed in verification are future instructions, not executed checks. Exact output from the final run is recorded in `verification.md`; implementation remains pending.
