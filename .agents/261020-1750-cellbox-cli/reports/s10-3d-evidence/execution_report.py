#!/usr/bin/env python3
"""Derive exercised verification and the stop finding from retained harness artifacts."""
import hashlib
import json
from pathlib import Path
import re
W = Path('/tmp/cellbox-s10-3d-resume')
R = Path('/home/dmin/cellos')
records = [json.loads(line) for line in (W / 'lanes/summary.jsonl').read_text().splitlines()]
host_log = (W / 'logs/host-tests.log').read_text()
host = []
for section in re.split(r'Running unittests src/main.rs ', host_log)[1:]:
    crate = re.search(r'/deps/([^\s]+)-[0-9a-f]+\)', section)
    result = re.search(r'test result: ok\. (\d+) passed; (\d+) failed;', section)
    if crate and result: host.append({'crate': crate[1], 'passed': int(result[1]), 'failed': int(result[2])})
candidate = W / 'cand/candidate.json'
l0 = W / 'lanes/l0-classification.json'
result = {'candidate_sha256': hashlib.sha256(candidate.read_bytes()).hexdigest() if candidate.exists() else None,
          'host': host, 'harness_runs': [], 'lanes': records,
          'l0': json.loads(l0.read_text()) if l0.exists() else None,
          'unrun_after_stop': [],
          'source_hashes': {name: hashlib.sha256((R / name).read_bytes()).hexdigest() for name in [
              'cells/drivers/nvme/src/controller.rs', 'cells/drivers/nvme/src/deadline.rs',
              'cells/drivers/nvme/src/main.rs', 'cells/drivers/nvme/src/queue.rs', 'cells/tools/init/src/boot.rs', 'cells/tools/init/src/main.rs',
              '.github/workflows/ci.yml', 'scripts/phase10_canonical_drive.py', 'scripts/phase10_l0_stream.py',
              'scripts/test_phase10_canonical_drive.py', 'scripts/phase09_ssh_fixture.sh',
              'scripts/qemu-phase10-canonical-test.sh']}}
for name in ['harness-tests.log']:
    text = (W / 'logs' / name).read_text()
    count = re.search(r'Ran (\d+) tests', text)
    result['harness_runs'].append({'log': name, 'tests': int(count[1]), 'pass': text.rstrip().endswith('OK'),
                                   'resource_warning': 'ResourceWarning' in text})
expected = ['L1-p02-session', 'L1-p02-kernel', 'L2-p03-pipeline', 'L3-p04-terminal',
            'L5-p09-nofaults', 'L5f-p09-faults', 'default-p02-session',
            'default-p03-pipeline', 'default-p09-nofaults', 'default-p04-gui']
names = {record['lane'] for record in records}
result['unrun_after_stop'] = [name for name in expected if name not in names]
for profile in ['headless', 'desktop']:
    streak = 0
    for record in records:
        if record['lane'].startswith('L0-' + profile + '-') and record['exit'] != 3:
            streak = streak + 1 if record['exit'] == 0 else 0
    if streak < 6:
        result['unrun_after_stop'].append('L0-' + profile + '-six-consecutive-qualification')
gui = [record for record in records if record['lane'].startswith('L4-gui-')]
streak = 0
for record in gui:
    streak = streak + 1 if record['exit'] == 0 else 0
if streak < 6:
    result['unrun_after_stop'].append('L4-GUI-six-consecutive-qualification')
for path, label in [('l6/l6-classification.json', 'L6-before-after-12-consumers-and-CI'),
                    ('audit/audit.json', 'CBX-01/02-audit')]:
    if not (W / path).exists():
        result['unrun_after_stop'].append(label)
failed = [record for record in records if record['exit'] != 0]
result['stop'] = failed[-1] if failed else None
audit = W / 'audit/audit.json'
if audit.exists():
    result['audit'] = json.loads(audit.read_text())
    stale = W / 'audit/audit-stale-harness.json'
    result['audit_stale_harness'] = json.loads(stale.read_text()) if stale.exists() else None
    if result['audit']['verdict'] != 'PASS':
        result['stop'] = {'lane': 'CBX-01/02-audit', 'verdict': result['audit']['verdict']}
result['controlled'] = json.loads((W / 'controlled/dma-order-proof.json').read_text())
(W / 'logs/execution.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({k: result[k] for k in ['candidate_sha256', 'host', 'harness_runs', 'stop', 'unrun_after_stop']}, indent=2))
