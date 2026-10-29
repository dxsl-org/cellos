#!/usr/bin/env python3
"""Derive exercised verification and the stop finding from retained harness artifacts."""
import hashlib
import json
from pathlib import Path
import re
W = Path('/tmp/cellbox-s10-3c-resume')
R = Path('/home/dmin/cellos')
records = [json.loads(line) for line in (W / 'lanes/summary.jsonl').read_text().splitlines()]
host_log = (W / 'logs/host-tests.log').read_text()
host = []
for section in re.split(r'Running unittests src/main.rs ', host_log)[1:]:
    crate = re.search(r'/deps/([^\s]+)-[0-9a-f]+\)', section)
    result = re.search(r'test result: ok\. (\d+) passed; (\d+) failed;', section)
    if crate and result: host.append({'crate': crate[1], 'passed': int(result[1]), 'failed': int(result[2])})
result = {'candidate_sha256': hashlib.sha256((W / 'cand/candidate.json').read_bytes()).hexdigest(),
          'host': host, 'harness_runs': [], 'lanes': records, 'l0': json.loads((W / 'lanes/l0-classification.json').read_text()),
          'unrun_after_stop': ['L0 headless six-consecutive qualification', 'L0 desktop',
                              'L1 imported P02 session and kernel', 'L2 imported P03', 'L3 imported P04 terminal',
                              'L4 imported GUI six consecutive', 'L5 P09 no-faults', 'L5f P09 faults',
                              'L6 12 consumers before and after plus CI boot', 'CBX-01/02 audit',
                              'default P02 session, P03, GUI, P09'],
          'source_hashes': {name: hashlib.sha256((R / name).read_bytes()).hexdigest() for name in [
              'cells/drivers/nvme/src/controller.rs', 'cells/drivers/nvme/src/deadline.rs',
              'cells/drivers/nvme/src/main.rs', 'cells/tools/init/src/boot.rs', 'cells/tools/init/src/main.rs',
              '.github/workflows/ci.yml', 'scripts/phase10_canonical_drive.py', 'scripts/phase10_l0_stream.py',
              'scripts/test_phase10_canonical_drive.py', 'scripts/phase09_ssh_fixture.sh',
              'scripts/qemu-phase10-canonical-test.sh']}}
for name in ['harness-tests.log', 'harness-tests-after-tui-framing.log']:
    text = (W / 'logs' / name).read_text()
    count = re.search(r'Ran (\d+) tests', text)
    result['harness_runs'].append({'log': name, 'tests': int(count[1]), 'pass': text.rstrip().endswith('OK'),
                                   'resource_warning': 'ResourceWarning' in text})
result['stop'] = {'id': 'SC5', 'lane': 'L0-headless-run2', 'boot': 'ssh',
                  'evidence': '/tmp/cellbox-phase10-w5t1anar/evidence/serial-ssh.log:201-215',
                  'finding': 'Identify admin opcode 6 unanswered after 60000 ms; controller init IO; VFS unavailable',
                  'new_product_finding_not_waived': True}
(W / 'logs/execution.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({k: result[k] for k in ['candidate_sha256', 'host', 'harness_runs', 'stop', 'unrun_after_stop']}, indent=2))
