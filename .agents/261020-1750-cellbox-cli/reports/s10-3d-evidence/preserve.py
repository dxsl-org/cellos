#!/usr/bin/env python3
"""Generate canonical comparison and archive harness evidence without modifying any verdict."""
import hashlib
import json
import shutil
from pathlib import Path
W = Path('/tmp/cellbox-s10-3d-resume')
R = Path('/home/dmin/cellos')
D = R / '.agents/261020-1750-cellbox-cli/reports/s10-3d-evidence'
sha = lambda p: hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()
before = json.loads((W / 'canonical/before.json').read_text())
after = {name: {'sha256': sha(R / name), 'bytes': (R / name).stat().st_size} for name in before}
(W / 'canonical/after.json').write_text(json.dumps(after, indent=2) + '\n')
main_before = (W / 'canonical/kernel-main-before.sha256').read_text().split()[0]
comparison = {'files': len(before), 'changed': [n for n in before if before[n] != after[n]],
              'kernel_main_before': main_before, 'kernel_main_after': sha(R / 'kernel/src/main.rs')}
comparison['unchanged'] = not comparison['changed'] and comparison['kernel_main_before'] == comparison['kernel_main_after']
(W / 'canonical/comparison.json').write_text(json.dumps(comparison, indent=2) + '\n')
TEXT = {'.json', '.jsonl', '.log', '.out', '.txt', '.sha256', '.py', '.sh', '.bin', '.trace'}
def copy_text(src, dst):
    if not src.exists(): return
    for p in src.rglob('*'):
        if p.is_file() and p.suffix in TEXT and p.stat().st_size <= 8 << 20:
            out = dst / p.relative_to(src)
            out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(p, out)
for name in ['canonical', 'logs', 'controlled']:
    copy_text(W / name, D / name)
for name in ['lanes', 'l6', 'audit']:
    copy_text(W / name, D / 'rerun' / name)
for name in ['build-candidate.sh', 'candidate_json.py', 'verify.sh', 'preserve.py', 'battery.py',
             'build-controlled.py', 'analyze-controlled.py', 'execution_report.py']:
    shutil.copy2(W / name, D / name)
scripts = D / 'harness'
scripts.mkdir(exist_ok=True)
for name in ['phase10_canonical_drive.py', 'phase10_l0_stream.py', 'test_phase10_canonical_drive.py',
             'phase09_ssh_fixture.sh', 'qemu-phase10-canonical-test.sh']:
    shutil.copy2(R / 'scripts' / name, scripts / name)
shutil.copy2(Path('/tmp/cellbox-s10-3c-harness/classify_l0.py'), scripts / 'classify_l0.py')
sources = D / 'source'
for name in ['cells/drivers/nvme/src/controller.rs', 'cells/drivers/nvme/src/queue.rs',
             'cells/drivers/nvme/src/deadline.rs', 'cells/tools/init/src/boot.rs',
             'cells/tools/init/src/main.rs']:
    dest = sources / name
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(R / name, dest)
summary = W / 'lanes/summary.jsonl'
if summary.exists():
    for line in summary.read_text().splitlines():
        record = json.loads(line)
        copy_text(Path(record['work']) / 'evidence', D / 'rerun/lanes' / record['lane'])
if (W / 'cand/candidate.json').exists():
    (D / 'rerun/candidate').mkdir(parents=True, exist_ok=True)
    shutil.copy2(W / 'cand/candidate.json', D / 'rerun/candidate/candidate.json')
    for profile in ['headless', 'desktop']:
        shutil.copy2(W / 'cand' / profile / 'manifest.json', D / 'rerun/candidate' / f'manifest-{profile}.json')
    for p in (W / 'cand').rglob('*'):
        if p.is_file(): p.chmod(p.stat().st_mode & ~0o222)
print(json.dumps(comparison, indent=2))
print('Evidence archived:', D)
