#!/usr/bin/env python3
"""Bounded device-side evidence collection; stop at the first captured timeout."""
import json
from pathlib import Path
import subprocess
import sys
base = Path('/tmp/cellbox-s10-3d-investigation')
for index in range(3, 9):
    subprocess.run([sys.executable, '-B', str(base / 'probe.py'), str(index)], check=True)
    result = json.loads((base / f'probe-{index}' / 'capture.json').read_text())
    if result['reason'] != 'booted-to-shell':
        print('Captured discriminating failure in probe', index, flush=True)
        break
