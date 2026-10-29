#!/usr/bin/env python3
"""S10-4 canonical/candidate proof; before and after records are generated only."""
import hashlib, json, sys
from pathlib import Path
R=Path('/home/dmin/cellos'); W=Path('/tmp/cellbox-s10-4')
C=Path('/tmp/cellbox-s10-3d-resume/cand')
def record(p):
    return {'size':p.stat().st_size,'sha256':hashlib.file_digest(p.open('rb'),'sha256').hexdigest()}
phase=sys.argv[1]
prior=json.loads(Path('/tmp/cellbox-s10-3d-resume/canonical/before.json').read_text())
paths={str(R/n):record(R/n) for n in prior}
paths[str(R/'kernel/src/main.rs')]=record(R/'kernel/src/main.rs')
paths.update({str(p):record(p) for p in C.rglob('*') if p.is_file()})
out=W/'canonical'; out.mkdir(exist_ok=True)
if phase=='before':
    if (out/'before.json').exists(): raise SystemExit('refuse overwrite before record')
    expected='4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821'
    assert paths[str(C/'candidate.json')]['sha256']==expected
    (out/'before.json').write_text(json.dumps(paths,indent=2)+'\n')
else:
    (out/f'{phase}.json').write_text(json.dumps(paths,indent=2)+'\n')
    before=json.loads((out/'before.json').read_text())
    changed=[n for n in before if before[n]!=paths.get(n)]
    result={'unchanged':not changed,'files':len(before),'canonical_files':len(prior),'changed':changed}
    name='comparison' if phase=='after' else f'comparison-{phase}'
    (out/f'{name}.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))
    if changed: raise SystemExit(1)
print(phase, len(paths), 'files recorded')
