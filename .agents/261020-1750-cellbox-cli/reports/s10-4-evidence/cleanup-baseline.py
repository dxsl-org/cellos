#!/usr/bin/env python3
"""Remove only this approved disposable baseline checkout/mirror after evidence is secure."""
import hashlib,json,os,shutil,stat,subprocess
from pathlib import Path
W=Path('/tmp/cellbox-s10-4');R=Path('/home/dmin/cellos');D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence'
summary=json.loads((D/'signing/verified/summary.json').read_text())
assert summary['status']=='COMPLETE_SIX_VALID_SCOPED_DEV_SIGNATURES'
B=W/'baseline-worktree';M=W/'signing-review-root'
assert B.resolve()==B and M.resolve()==M and not B.is_symlink() and not M.is_symlink()
# Parent restored only its two approved disposable manifest changes via edit.
process=subprocess.run(['git','worktree','remove',str(B)],cwd=R,capture_output=True,text=True)
record={'command':['git','worktree','remove',str(B)],'exit':process.returncode,'stdout':process.stdout,'stderr':process.stderr,'worktree_removed':not B.exists(),'signed_evidence':str(D/'signing/verified/summary.json')}
if process.returncode:
    (W/'cleanup.json').write_text(json.dumps(record,indent=2)+'\n');raise SystemExit(json.dumps(record))
if M.exists():
    for root,dirs,files in os.walk(M):
        p=Path(root);assert not p.is_symlink();p.chmod(0o700)
        for f in files:
            file=p/f;assert not file.is_symlink() and file.stat().st_nlink==1
    shutil.rmtree(M)
record['mirror_removed']=not M.exists()
# Retain six signed/unsigned measurement originals, not disposable compilation cache.
cache=W/'baseline-target'
if cache.exists():shutil.rmtree(cache)
record['build_cache_removed']=not cache.exists()
(W/'cleanup.json').write_text(json.dumps(record,indent=2)+'\n');shutil.copy2(W/'cleanup.json',D/'cleanup.json');shutil.copy2(Path(__file__),D/'cleanup-baseline.py')
print(json.dumps(record,indent=2))
