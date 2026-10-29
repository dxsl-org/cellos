#!/usr/bin/env python3
"""Authorized fallback only: offline lock reconciliation and full package/graph delta."""
import json,os,shutil,subprocess,tomllib
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); B=W/'baseline-worktree'; L=W/'logs-lock';L.mkdir(exist_ok=True)
oldbytes=(B/'Cargo.lock').read_bytes();old=tomllib.loads(oldbytes.decode())
assert not any(p['name']=='vm-runner' for p in old['package']), 'preferred minimal package removal must be attempted if present'
(W/'lock-before-reconciliation.lock').write_bytes(oldbytes)
env=dict(os.environ);env['CARGO_NET_OFFLINE']='true';env['CARGO_TARGET_DIR']=str(W/'baseline-target')
command=['cargo','+nightly-2026-05-01','metadata','--offline','--format-version','1','--filter-platform','x86_64-unknown-none']
with (L/'metadata.json').open('w') as out,(L/'metadata.stderr.log').open('w') as err:
    process=subprocess.run(command,cwd=B,env=env,stdout=out,stderr=err)
(W/'lock-command.json').write_text(json.dumps({'command':command,'cwd':str(B),'offline':True,'exit':process.returncode,'preferred_step':'vm-runner package entry absent from original Cargo.lock; cannot delete nonexistent entry','selected_step':'authorized offline fallback'},indent=2)+'\n')
if process.returncode:raise SystemExit(f'STOP offline metadata exit {process.returncode}')
new=tomllib.loads((B/'Cargo.lock').read_text());metadata=json.loads((L/'metadata.json').read_text())
packages={p['id']:p for p in metadata['packages']};nodes={n['id']:n for n in metadata['resolve']['nodes']}
roots=[p['id'] for p in metadata['packages'] if p['name'] in ['app-shell','app-sys-tools']]
seen=set()
def visit(i):
    if i in seen:return
    seen.add(i)
    for d in nodes[i]['dependencies']:visit(d)
for i in roots:visit(i)
graph_names={packages[i]['name'] for i in seen}
def key(p):return (p['name'],p['version'],p.get('source'))
a={key(p):p for p in old['package']};b={key(p):p for p in new['package']}
removed=[{'package':a[k],'in_measured_graph_by_name':k[0] in graph_names} for k in sorted(a.keys()-b.keys(),key=str)]
added=[{'package':b[k],'in_measured_graph_by_name':k[0] in graph_names} for k in sorted(b.keys()-a.keys(),key=str)]
modified=[{'name':k[0],'version':k[1],'source':k[2],'old':a[k],'new':b[k],'in_measured_graph_by_name':k[0] in graph_names} for k in sorted(a.keys()&b.keys(),key=str) if a[k]!=b[k]]
diff=subprocess.run(['git','diff','--','Cargo.lock'],cwd=B,capture_output=True,text=True,check=True).stdout
(W/'b2-lock.patch').write_text(diff)
record={'selected_step':'offline fallback; preferred vm-runner package deletion impossible because original lock contains no such package','command':command,'removed':removed,'added':added,'same_version_package_metadata_changed':modified,'measured_graph':[{'name':packages[i]['name'],'version':packages[i]['version'],'source':packages[i]['source']} for i in sorted(seen)],'vm_runner_in_graph':any(packages[i]['name']=='vm-runner' for i in seen),'package_versions_sources_unchanged':not removed and not added,'old_lock_version':old['version'],'new_lock_version':new['version']}
assert not record['vm_runner_in_graph']
(W/'lock-audit.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps(record,indent=2))
