#!/usr/bin/env bash
# llms.txt is curated by hand: its "Active direction" header and the subset of
# docs it lists are editorial choices that no walk of docs/ reproduces. This
# script used to regenerate it from docs/ and would replace that content with a
# stale "ViOS" index, so it now refuses to write and only says where to edit.
#
# It exits 0 so the docs workflow step that calls it does not fail.
set -euo pipefail

cat <<'EOF'
gen-llms-txt.sh: not regenerating llms.txt.
llms.txt is hand-curated (ADR-0022 direction header plus a chosen docs index).
Edit llms.txt directly; when you add a doc that agents should find, add one line
"- [<first heading>](<path>)" in the matching section. Every docs/specs/*.md
belongs in the list.
EOF
