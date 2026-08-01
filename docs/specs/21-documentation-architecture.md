# Spec 21 — Documentation Architecture: Anchored Specs, Generated Status (ADR)

> **Status**: Accepted 2026-07-30 — governs how Cellos records architecture. Adopted after
> an inventory found 39 unresolved or contradictory decisions across specs 00–20
> (`.agents/reports/spec-unresolved-inventory-260730.md`), the majority of them drift
> between a document and the code it describes.

## 1. Context — what actually causes drift

Two candidate diagnoses were considered.

**"Specs live too far from the code."** Partly true, but insufficient as a cure. Comments
drift too, and they drift *silently* because nothing reads them systematically. Worked
example in this repo: `cells/services/vfs/src/handle_table.rs:2` declares *"Per-cell open
file handle table"* and `:12` labels `owner` as *"for quota accounting"*, while `get_mut`
at `:54` looks up by `cap.0` alone and never compares `owner` — so the table is **not**
per-cell and one cell can read another's handle. The false claim sat 52 lines above the
code that refutes it, and only an adversarial review found it.

**"Nothing verifies the claim."** This is the operative cause. The invariants in this repo
that have *not* drifted are the ones a CI job checks: the cell-unsafe allowlist has never
grown silently, because a CI gate fails the build when it does. Everything unchecked
drifted, in `docs/` and in comments alike.

That same example also shows why a *reference* is not a verification. The gate described
above was `scripts/check-cells-unsafe-ratchet.py` when this ADR's first draft was written;
within the hour it was deleted and replaced by `scripts/cellos-sign --check --strict`, with
its rule and allowlist moved to `scripts/unsafe-allowlist.toml`. The **invariant** survived
the rewrite untouched — because CI enforced it — while this document's **prose** was stale
in under an hour. Anchors exist so that the second half of that sentence becomes a build
failure instead of a discovery.

Decisive against "move all specs into comments": **the worst drift is about things that do
not exist**, and absence has no line number to comment on. The Metadata Registry
(`02-memory.md §Registry`, depended on by four other specs), `catch_unwind`
(`01-core.md §5`), SASan (`10-testing.md §2`), and readiness notifications
(`17-ipc-wire-contract.md §10`, later corrected to Draft by
[ADR 0001](../decisions/0001-readiness-notifications-remain-draft-until-implemented.md))
were all specified and absent from the tree. In a comments-only world these would not be
*fixed*; they would be *invisible*.
Comments also cannot host rejected alternatives (no code location), cross-file invariants
(no single home), or hardware/certification constraints.

Roughly one fifth of the 39 findings were of a class an in-code comment could have
prevented. The remaining four fifths were status claims, cross-cutting policy, facts about
absent mechanisms, or facts about hardware.

## 2. Decision — three layers with a strict allocation rule

Overlap between layers is where drift breeds, so each fact has exactly one home.

### Layer 1 — Specs and ADRs (`docs/specs/`), hand-written, few, stable

Contains **only**: decisions and their rationale, **rejected alternatives**, invariants
that span files, hardware and certification constraints, and deliberate absences. This
content is true until a new decision changes it, so it does not drift with code.

**Specs MUST NOT contain status prose.** No "✅ COMPLETE", no "not implemented", no LOC
counts, no coverage percentages, no "works today" tables. Those belong to Layer 3.

### Layer 2 — Code comments, per the existing Rust standards

Contract, not narration: preconditions, invariants, non-obvious side effects, lock
ordering, `// SAFETY:`. One rule added by this ADR:

> A doc comment that asserts a *security or isolation property* MUST name the function or
> check that enforces it. If no such enforcement exists, the comment states the gap
> instead. `handle_table.rs:2` is the canonical violation.

### Layer 3 — Status, **generated, never hand-written**

`docs/spec-status.generated.md` is produced by `scripts/check-spec-anchors.py` and is not
edited by hand. It lists every anchored spec section, its anchor, and the resolved state.
Kernel LOC, cell-crate compliance counts, and "what works today" tables are derived the
same way. Any document that needs to state status **links** to the generated file.

## 3. The anchor mechanism

Every spec section that makes a normative claim carries exactly one `Anchor:` line
directly beneath its heading. The checker resolves it against the tree.

```
### 2.1 W^X after relocation

Anchor: impl kernel/src/loader/wx.rs::enforce
```

Anchor kinds:

| Kind | Form | Checker asserts |
|---|---|---|
| `impl` | `impl <path>::<symbol>` | file exists and declares `<symbol>` |
| `const` | `const <path>::<NAME>=<value>` | constant exists **and its value matches** |
| `test` | `test <path>::<test_name>` | test function exists |
| `planned` | `planned .agents/<plan-dir>[/phase-NN]` | the plan dir exists; **forbidden on a Ratified/Accepted/Definitive section** |
| `absent` | `absent <symbol>` | the symbol appears **nowhere** in `kernel/`, `libs/`, `cells/` |
| `design` | `design` | nothing — for pure rationale/rejected-alternative sections |

Multiple anchors are allowed on one line, separated by ` · `.

`absent` is the reverse-direction guard and is as important as `impl`: it pins a
deliberate non-feature (for example "no exactly-once delivery"), and CI fails if someone
implements it without amending the spec.

### Failure modes the checker catches

1. A section marked Ratified/Accepted/Definitive with **no** `Anchor:` line.
2. An anchor pointing at a missing file, symbol, or test — the mechanism was renamed or
   removed and the spec was not updated. *(This is the class behind D8 and ADR 0001:
   `17 §10` named `NotifyRegister`/`NotifyDeregister`, absent from `NetRequest`.)*
3. A `const` whose value drifted from the spec's number.
4. `planned` on a Ratified section — a spec claiming ratified status for unbuilt work.
   *(Would have caught the Metadata Registry, `catch_unwind`, and SASan.)*
5. An `absent` symbol that now exists.
6. Status prose in a spec: a small deny-list of markers (`✅`, "COMPLETE", "not
   implemented", "LOC") outside a fenced block.

The checker is Python, follows the repo convention (invoked as `python3 scripts/<name>` from
CI, alongside `check-cell-va-layout.py`, `check-cargo-config-parity.py`, and
`cellos-sign --check`), needs no toolchain, and is regex-based — it never compiles the tree,
so it runs on every push cheaply.

One deliberate limitation: an `impl` anchor proves a **symbol** exists, not that the symbol
still does what the section says. A rewrite that keeps the name and changes the meaning
passes. Anchors close the "mechanism vanished or never existed" class — which is where the
inventory found the damage — and leave semantic drift to review. Choosing anchors that name
a *test* rather than an implementation narrows the gap where it matters most.

### What the anchor deliberately does not do

It proves a mechanism **exists**, not that it is **correct** or **exercised at runtime**.
`test` anchors raise the bar to "a test names this behaviour"; they do not prove the test
ran on hardware. Runtime verification stays the job of the suite and the per-plan
acceptance criteria. Spec 19's Layer A is the live example: the implementation and its
test exist, while runtime verification is still pending — an honest anchor plus a
generated status row expresses exactly that, where a hand-written "✅" would not.

## 4. Rollout

1. **Checker first**, warn-only: `scripts/check-spec-anchors.py`, plus the generated
   status file. Nothing fails yet.
2. **Anchor the load-bearing specs** — 15 through 21, which carry the current
   architectural claims.
3. **Turn on enforcement** for those specs; failures block the build.
4. **Backfill 00–14** in batches. A spec that cannot be anchored honestly gets its status
   downgraded to Draft rather than a fabricated anchor — downgrading is the correct
   outcome, not a defeat.
5. **Retire hand-written status** from `README.md` and `docs/system-architecture.md`;
   replace those tables with links to the generated file.

Anchoring is not a licence to resolve the 39 open decisions silently. Where a spec and the
code disagree, the ruling on *which is wrong* stays with the architect
(`.agents/reports/decision-docket-260730.md`); the checker only makes the disagreement
impossible to ignore.

## 5. Rejected alternatives

- **Move all specs into code comments.** Cannot express absent mechanisms, rejected
  alternatives, cross-file invariants, or hardware constraints; and comments drift
  silently, as `handle_table.rs:2` demonstrates. It would have addressed roughly a fifth of
  the observed drift while hiding the worst class.
- **Keep specs as they are and rely on review discipline.** Already the status quo; it
  produced 39 findings across 21 specs.
- **Generate specs from code (literate/doc-extraction).** A generated document can only
  describe what exists — the same blind spot as comments, and it additionally cannot hold a
  decision.
- **A single monolithic architecture document.** Does not address verification at all, and
  makes the ownership of each fact less clear rather than more.

## 6. Cross-references

| Topic | Document |
|---|---|
| Code comment standards (Layer 2 detail) | `~/.claude/rules/haily-coding.md`, `docs/code-standards.md` |
| Existing CI invariant checkers (the pattern this follows) | `scripts/cellos-sign --check`, `scripts/check-cell-va-layout.py`, `scripts/check-cargo-config-parity.py` |
| The 39 open rulings this mechanism surfaces | `.agents/reports/decision-docket-260730.md` |
| Spec inventory that motivated this ADR | `.agents/reports/spec-unresolved-inventory-260730.md` |
