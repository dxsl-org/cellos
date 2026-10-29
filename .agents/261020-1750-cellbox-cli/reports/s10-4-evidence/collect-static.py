#!/usr/bin/env python3
"""Archive actual static measurements and second stop; retain initial failure evidence."""
import hashlib,json,shutil,subprocess,tomllib
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); R=Path('/home/dmin/cellos'); B=W/'baseline-worktree'; D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence'
D.mkdir(exist_ok=True)
def closure(path,seen):
    path=path.resolve()
    if path in seen:return
    data=tomllib.loads(path.read_text());seen[path]=data.get('package',{}).get('name')
    tables=[data,*data.get('target',{}).values()]
    for t in tables:
        for kind in ['dependencies','build-dependencies','dev-dependencies']:
            for dep in t.get(kind,{}).values():
                if isinstance(dep,dict) and 'path' in dep:closure(path.parent/dep['path']/'Cargo.toml',seen)
seen={}
for p in ['cells/tools/shell/Cargo.toml','cells/tools/sys-tools/Cargo.toml']:closure(B/p,seen)
closure_result={'manifests':{str(k.relative_to(B)):v for k,v in seen.items()},'vm_runner_compile_dependency':any('vm-runner' in str(k) or v=='vm-runner' for k,v in seen.items()),'note':'Conservative recursive local path dependency census including optional/dev/build tables. Shell runtime spawn_external("vm-runner") string remains a separate user-app fallback; excluded member is not in selected ELF compile dependency closure.'}
assert not closure_result['vm_runner_compile_dependency']
(W/'dependency-closure.json').write_text(json.dumps(closure_result,indent=2)+'\n')
diff=subprocess.run(['git','diff','--','Cargo.toml'],cwd=B,capture_output=True,text=True,check=True).stdout
(W/'b2-manifest.patch').write_text(diff)
initial=D/'initial-summary.json'
if not initial.exists():shutil.copy2(D/'summary.json',initial)
for n in ['logs-repaired','static']: 
    out=D/n;out.mkdir(exist_ok=True)
    for p in (W/n).rglob('*'):
        if p.is_file() and p.suffix in ['.json','.log','.txt']:
            dst=out/p.relative_to(W/n);dst.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dst)
for n in ['build-b2.py','measure-static.py','collect-static.py','build-commands-repaired.json','dependency-closure.json','b2-manifest.patch']:shutil.copy2(W/n,D/n)
src=D/'repaired-source';src.mkdir(exist_ok=True);shutil.copy2(B/'Cargo.toml',src/'Cargo.toml');shutil.copy2(B/'Cargo.lock',src/'Cargo.lock')
shutil.copytree(W/'canonical',D/'canonical',dirs_exist_ok=True)
shutil.copy2(W/'preserve.py',D/'preserve.py')
record={'stage':'S10-4','status':'STOP_B2_LOCK_RECONCILIATION_REQUIRED','authorized_delta':'User2026-10-28 minimal disposable root Cargo.toml exclusion of nonexistent cells/apps/vm-runner only; non-pristine B2 comparator','dependency_closure':closure_result,'repaired_commands':json.loads((W/'build-commands-repaired.json').read_text()),'original_failure_preserved':True,'static_measurement':'static/measurement.json','b2_elf_results':None,'runtime_runs':0,'runtime_peaks':'not measured, reservation-only ceiling; observer decision awaits user','cbx27_met':False,'phase10_criterion4_met':False,'canonical_promotion':False,'worktree_retained_pending_disposition':True,'preservation_at_first_stop':json.loads((W/'canonical/comparison.json').read_text())}
record['retained_behavior']='behavior.json'
record['final_static_preservation']=json.loads((W/'canonical/comparison-after-static.json').read_text())
(D/'summary.json').write_text(json.dumps(record,indent=2)+'\n')
checks={str(p.relative_to(D)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in D.rglob('*') if p.is_file() and p.name!='evidence.sha256'}
(D/'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k,v in sorted(checks.items())))
print(json.dumps({'status':record['status'],'local_compile_manifests':len(seen),'vm_runner_compile_dependency':False,'archive':str(D)},indent=2))
