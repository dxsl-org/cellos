#!/usr/bin/env python3
"""Archive actual B2 unsigned results, offline pin audit and checked-signing refusal."""
import hashlib,json,shutil,subprocess
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); B=W/'baseline-worktree'; R=Path('/home/dmin/cellos'); D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence'
if not (D/'lock-stop-summary.json').exists():shutil.copy2(D/'summary.json',D/'lock-stop-summary.json')
if not (D/'lock-stop-source').exists():shutil.copytree(D/'repaired-source',D/'lock-stop-source')
for n in ['logs-reconciled','logs-lock','canonical']:shutil.copytree(W/n,D/n,dirs_exist_ok=True)
for n in ['build-b2.py','preserve.py','reconcile-lock.py','measure-b2.py','archive-b2.py','b2-measurement.json','lock-audit.json','lock-command.json','b2-lock.patch','lock-before-reconciliation.lock','build-commands-reconciled.json']:shutil.copy2(W/n,D/n)
src=D/'reconciled-source';src.mkdir(exist_ok=True)
for name in ['Cargo.toml','Cargo.lock','scripts/unsafe-allowlist.toml','scripts/cellos_sign/signing.py','scripts/cellos_sign/cli.py','scripts/sign-cell.py','cells/services/ocel-quickjs/src/native_layout.rs']:
    p=B/name;out=src/name;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out)
patch=subprocess.run(['git','diff','--','Cargo.toml','Cargo.lock'],cwd=B,capture_output=True,text=True,check=True).stdout
(D/'b2-all-authorized-deltas.patch').write_text(patch)
b2=json.loads((W/'b2-measurement.json').read_text());proof=json.loads((W/'canonical/comparison-after-b2.json').read_text())
record={'stage':'S10-4','status':b2['status'],'candidate_sha256':'4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821','static_measurement':'static/measurement.json','retained_behavior':'behavior.json','b2_measurement':'b2-measurement.json','offline_lock_audit':'lock-audit.json','all_original407_lock_package_blocks_byte_identical':b2['all_existing_lock_blocks_byte_identical'],'added_local_lock_packages':b2['added_local_lock_packages'],'registry_version_source_changes':b2['registry_version_source_changes'],'build_completed':True,'checked_signing_completed':False,'signing_refusal':b2['signing_refusal'],'unsigned_staging_copies_are_not_signed_results':True,'runtime_runs':0,'runtime_memory_peaks_measured':False,'cbx27_met':False,'phase10_criterion4_met':False,'result_cp2_requested':False,'canonical_promotion':False,'worktree_retained_pending_signing_disposition':True,'final_preservation':proof,'prior_stops_preserved':['initial-summary.json','lock-stop-summary.json']}
(D/'summary.json').write_text(json.dumps(record,indent=2)+'\n')
checks={str(p.relative_to(D)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in D.rglob('*') if p.is_file() and p.name!='evidence.sha256'}
(D/'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k,v in sorted(checks.items())))
print(json.dumps({'status':record['status'],'b2_unsigned_bytes':b2['six_unsigned_bytes'],'preservation':proof,'archive':str(D)},indent=2))
