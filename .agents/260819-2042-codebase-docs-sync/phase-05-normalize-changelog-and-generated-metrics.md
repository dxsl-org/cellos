---
phase: 5
title: "Normalize Changelog And Generated Metrics"
status: completed
priority: P2
effort: "2h"
dependencies: [1]
tier: fast
---

# Phase 5: Normalize Changelog And Generated Metrics

## Overview

Keep the changelog chronological and generated metrics canonical while docs are moved. This phase prevents hand-edited metrics or duplicated history from spreading.

## Requirements

- Functional: add one concise changelog entry for docs resync; regenerate or preserve generated metrics according to script output.
- Non-functional: do not make changelog a second roadmap; do not hand-edit generated metric values.

## Architecture

Data flow: completed doc diff and metrics script output enter changelog/metrics; output is a dated docs entry and a generated metrics file that matches `scripts/generate-code-metrics.py`.

## Assumptions

- **Claim:** Metrics script can run in this host.
  **Confidence:** medium
  **How to verify:** `python scripts/generate-code-metrics.py --check` or the repository's documented equivalent.

## Related Files

- Modify: `docs/project-changelog.md`
- Modify only via generator: `docs/code-metrics.generated.md`

## Implementation Steps

1. Check whether docs-only changes require metrics regeneration; if not, leave `docs/code-metrics.generated.md` untouched.
2. If Phase 3 changes generated metrics references only, run `python scripts/generate-code-metrics.py --check`.
3. If generated metrics are stale from source reality, run the generator and review the diff.
4. Add a top changelog entry summarizing docs resync and roadmap split; link new roadmap files.
5. Keep older changelog entries unchanged except link fixes required by moved roadmap paths.

## Success Criteria

- [ ] Changelog has exactly one new 2026-08-19 docs-resync entry.
- [ ] `docs/code-metrics.generated.md:3` generated-file contract is preserved.
- [ ] Metrics check passes or the inability to run it is recorded as host-gated.

## Security Considerations

Do not remove past security caveats from changelog history while compressing references.

## Risk Notes

- Low likelihood x medium impact: generator changes due to source drift unrelated to docs. Mitigation: review generated diff separately and do not hand-normalize.
- Medium likelihood x low impact: changelog file remains large. Mitigation: defer changelog archiving as unresolved unless user approves.
- Rollback: revert changelog and regenerated metrics diff. Irreversible part: none.

## Deviation Log

The changelog received the docs-sync entry. Generated metrics were preserved
as generated artifacts; no hand-edited metric values were introduced.
