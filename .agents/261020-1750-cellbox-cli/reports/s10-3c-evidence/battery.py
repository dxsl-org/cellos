#!/usr/bin/env python3
"""Execute approved S10-3 rerun; halt immediately on a new lane/qualification failure."""
import json
import os
from pathlib import Path
import subprocess
import sys
W = Path('/tmp/cellbox-s10-3c-resume')
R = Path('/home/dmin/cellos')
os.chdir(R)
sys.path.insert(0, str(R / 'scripts'))
import phase10_l0_stream as stream
sys.path.insert(0, '/tmp/cellbox-s10-3c-harness')
import classify_l0
C = W / 'cand/candidate.json'
def run(args, log):
    with (W / log).open('w') as out:
        status = subprocess.call(args, stdout=out, stderr=subprocess.STDOUT)
    print(log, 'exit', status, flush=True)
    if status: raise SystemExit(status)
def invoke(args, cwd):
    rc = subprocess.call(args, cwd=cwd)
    result = classify_l0.classify(W / 'lanes')
    (W / 'lanes/l0-classification.json').write_text(json.dumps(result, indent=2) + '\n')
    if rc not in (0, 3):
        print('STOP: counted L0 failure; inspect l0-classification.json', flush=True)
        raise SystemExit(rc)
    return rc
status = stream.orchestrate(C, W / 'lanes/run_lane.py', invoke=invoke)
if status: raise SystemExit(status)
run(['bash', str(W / 'lanes/stream_a.sh')], 'lanes/stream_a.out')
run(['bash', str(W / 'lanes/stream_b.sh')], 'lanes/stream_b.out')
# Validate the full six-run rule rather than trusting the shell stream's final echo.
records = stream.read_summary(W / 'lanes/summary.jsonl')
gui = [r for r in records if r['lane'].startswith('L4-gui-')]
streak = 0
for record in gui: streak = streak + 1 if record['exit'] == 0 else 0
if streak < 6:
    print('STOP: L4 six consecutive successes not reached', flush=True)
    raise SystemExit(1)
for phase in ['before', 'after']:
    run([sys.executable, str(W / 'l6/run_l6.py'), str(C), str(W / 'l6' / phase), phase], f'l6/{phase}.log')
run([sys.executable, str(W / 'l6/classify_l6.py'), str(W / 'l6')], 'l6/classify.log')
classification = json.loads((W / 'l6/l6-classification.json').read_text())
if classification['regressions']:
    print('STOP: L6 regressions', classification['regressions'], flush=True)
    raise SystemExit(1)
for name, args in [('cellbox', ['--manifest-path', 'cells/tools/cellbox/Cargo.toml']), ('workspace', [])]:
    command = ['cargo', 'deny', '--offline', '--locked', '--exclude-dev', *args, 'check', 'advisories', 'licenses', 'bans', 'sources']
    with (W / 'audit' / f'cargo-deny-{name}.log').open('w') as out:
        rc = subprocess.call(command, stdout=out, stderr=subprocess.STDOUT)
        out.write(f'exit {rc}\n')
    if name == 'cellbox' and rc:
        print('STOP: scoped cargo-deny failed', flush=True)
        raise SystemExit(rc)
run([sys.executable, str(W / 'audit/run_audit.py'), str(W / 'audit')], 'audit/run.log')
if json.loads((W / 'audit/audit.json').read_text())['verdict'] != 'PASS':
    print('STOP: CBX-01/02 audit failed', flush=True)
    raise SystemExit(1)
print('PASS: full S10-3 battery', flush=True)
