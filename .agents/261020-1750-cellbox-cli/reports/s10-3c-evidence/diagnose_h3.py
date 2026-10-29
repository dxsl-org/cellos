#!/usr/bin/env python3
"""Re-evaluate retained UART prompt framing; never change any historical verdict or summary."""
import importlib.util
import json
from pathlib import Path
import re
import sys
R = Path('/home/dmin/cellos')
W = Path('/tmp/cellbox-s10-3c-resume')
sys.path.insert(0, str(R / 'scripts'))
import phase10_canonical_drive as drive
raw = Path('/tmp/cellbox-phase10-s6pjo8ow/evidence/serial-main.log').read_bytes()
pattern = re.compile(rb'\x1b\[\?1049l(?:\x1b\[[0-?]*[ -/]*[@-~])*Cellos > ')
events = []
for match in pattern.finditer(raw):
    sample = raw[:match.end()]
    original_lines = drive.logical_lines(drive.strip_ansi(sample))
    corrected_lines = drive.shell_lines(sample)
    events.append({'offset': match.start(), 'restore_and_prompt': repr(match.group()),
                   'original_last_line_tail': original_lines[-1][-180:],
                   'original_idle': drive.ends_on_prompt(original_lines),
                   'corrected_idle': drive.ends_on_prompt(corrected_lines),
                   'original_prompt_count': sum(l.startswith('Cellos > ') for l in original_lines),
                   'corrected_prompt_count': sum(l.startswith('Cellos > ') for l in corrected_lines)})
result = {'source': '/tmp/cellbox-phase10-s6pjo8ow/evidence/serial-main.log',
          'events': events,
          'all_rejected_originally': all(not e['original_idle'] for e in events),
          'all_detected_after_explicit_boundary_framing': all(e['corrected_idle'] for e in events)}
(W / 'logs/h3-tui-diagnosis.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result, indent=2))
