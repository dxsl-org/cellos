#!/usr/bin/env python3
"""Archive actual S10-4 stopped-stage records without rewriting a verdict."""
import hashlib,json,shutil
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); R=Path('/home/dmin/cellos'); D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence'
D.mkdir(exist_ok=True)
for name in ['preserve.py','build-b2.py','collect-stop.py','archive-stop.py','build-commands.json','stop.json']:
    shutil.copy2(W/name,D/name)
for name in ['logs','canonical']:
    shutil.copytree(W/name,D/name,dirs_exist_ok=True)
S=D/'historical-source'; S.mkdir(exist_ok=True)
for src,dst in [('Cargo.toml','Cargo.toml'),('Cargo.lock','Cargo.lock'),('rust-toolchain.toml','rust-toolchain.toml'),('cells/tools/shell/Cargo.toml','shell-Cargo.toml'),('cells/tools/sys-tools/Cargo.toml','sys-tools-Cargo.toml')]:
    p=W/'baseline-worktree'/src
    if p.exists(): shutil.copy2(p,S/dst)
summary={'stage':'S10-4','status':'STOP_B2_PREREQUISITE','source':json.loads((W/'stop.json').read_text()),'preservation':json.loads((W/'canonical/comparison.json').read_text()),'new_footprint_measurements':False,'new_runtime_runs':0,'result_cp2_requested':False,'cbx27_met':False,'phase10_criterion4_met':False,'canonical_promotion':False,'worktree_retained_pending_disposition':True}
(D/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
checks={str(p.relative_to(D)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in D.rglob('*') if p.is_file() and p.name!='evidence.sha256'}
(D/'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k,v in sorted(checks.items())))
print(json.dumps({'archive':str(D),'status':summary['status'],'preservation':summary['preservation']},indent=2))
