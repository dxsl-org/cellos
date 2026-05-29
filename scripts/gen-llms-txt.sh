#!/usr/bin/env bash
# Generate llms.txt from the docs/ directory index.
set -euo pipefail
python3 -c "
import os, re
lines = ['# ViOS', '', '> ViOS is a Rust no_std OS using Cellular SAS + LBI.', '', '## Docs', '']
for root, dirs, files in os.walk('docs'):
    dirs.sort()
    for f in sorted(files):
        if f.endswith('.md'):
            path = os.path.join(root, f)
            rel  = path.replace(chr(92), '/')
            with open(path, encoding='utf-8', errors='ignore') as fh:
                first = fh.readline().strip().lstrip('# ')
            lines.append(f'- [{first}]({rel})')
print('\n'.join(lines))
" > llms.txt
echo "llms.txt updated ($(wc -l < llms.txt) lines)"
