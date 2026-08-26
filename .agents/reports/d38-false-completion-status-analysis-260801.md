# D38 — Correct WASM/ePMP and Cell-to-Cell Anywhere status

**Status:** approved/applied 2026-08-01. Documentation/portfolio only.

## Finding

The docket's premise that WASM was removed is false. `Cargo.toml` still includes
`cells/drivers/wasm` and `cells/tools/wasm`, and both crates are tracked. Their product/
runtime qualification is unresolved, while ePMP requires an M-mode owner absent from the
current runtime. Spec 18 describes removal as a future action, not current fact.

Cell-to-Cell Anywhere has real foundation modules, but `dispatch` still lacks end-to-end
remote forwarding and remote lookup resolves locally. "COMPLETE" is therefore false at
the product/runtime level. "In progress" alone is also vague because no active integration
phase is currently executing.

## Recommended ruling [FINAL]

**Approve the status correction, but reject retirement on current evidence.**

1. Mark `260605-1406` partial/suspect: WASM implementation present, disposition and
   runtime qualification unresolved; ePMP blocked on an M-mode owner.
2. Mark `260624-cell-to-cell-anywhere` **partial — foundation complete, integration blocked**.
3. Require a two-node remote-call oracle before any future COMPLETE claim.
4. Keep Spec 20 Draft as the contract owner; this ruling changes no network code or ABI.
