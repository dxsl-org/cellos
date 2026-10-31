#!/usr/bin/env bash
# ADR-0030: a copyleft (GPL/AGPL/LGPL) crate may exist in the workspace only as
# a leaf application Cell. Fails if any other crate depends on one, which would
# pull copyleft code into kernel/, hal/, libs/ or another Cell.
#
# The leaf list must match the copyleft [[licenses.exceptions]] in deny.toml;
# cargo-deny rejects any copyleft crate not listed there.
set -euo pipefail

LEAVES=(doom ocel-pdf)

status=0
for leaf in "${LEAVES[@]}"; do
    # Inverted tree, depth 1: the leaf itself plus its direct dependents.
    dependents=$(cargo tree --workspace --all-features --invert "$leaf" \
        --edges normal,build --depth 1 --prefix none \
        | sed -n '2,$p' | sort -u)
    if [[ -n "$dependents" ]]; then
        echo "error: copyleft leaf '$leaf' has dependents (ADR-0030):" >&2
        echo "$dependents" | sed 's/^/  /' >&2
        status=1
    fi
done

[[ $status -eq 0 ]] && echo "copyleft leaves OK: ${LEAVES[*]}"
exit $status
