#!/usr/bin/env python3
"""Census only: do not invoke F1/F5 check, mint signatures, or change signing scope."""
import hashlib,json,subprocess,tomllib
from pathlib import Path
W=Path('/tmp/cellbox-s10-4');B=W/'baseline-worktree';R=Path('/home/dmin/cellos');O=W/'observability'
raw=subprocess.run(['git','ls-files','-z','--','cells/tools/shell','cells/tools/sys-tools'],cwd=B,capture_output=True,check=True).stdout
files=[Path(p.decode()) for p in raw.split(b'\0') if p]
def rec(path):return {'path':str(path),'sha256':hashlib.file_digest(path.open('rb'),'sha256').hexdigest(),'bytes':path.stat().st_size}
sources=[rec(B/p) for p in files if p.suffix=='.rs' and (B/p).is_file()]
allow=tomllib.loads((B/'scripts/unsafe-allowlist.toml').read_text())
entries=[e for e in allow.get('crate',[]) if e.get('name') in ['app-shell','app-sys-tools']]
fileentries=[e for e in allow.get('file',[]) if e.get('path','').startswith(('cells/tools/shell/','cells/tools/sys-tools/'))]
closure=json.loads((W/'dependency-closure.json').read_text())
record={'kind':'proposed scoped snapshot input census, NOT scoped F1/F5 verdict or authorization','selected_cell_crates':['app-shell','app-sys-tools'],'selected_artifacts':['shell','ls','cat','echo','ps','kill'],'cell_source_files':sources,'cell_source_count':len(sources),'existing_crate_allowlist_entries':entries,'existing_file_allowlist_entries':fileentries,'local_dependency_manifests':closure['manifests'],'tcb_boundary':'libs/* and registry dependencies remain outside the unchanged F1 cell-source policy; source hashes and resolved graph identify them, not certify memory safety.','existing_cli_capability':'--repo ROOT with release-tarball filesystem fallback; default F1 remains whole cells/ within supplied root. No built-in --package/--scope flag; existing named candidate routes still run whole-root check and cannot authenticate this historical shell recipe.','historical_policy_failure_preserved':'Unrelated ocel-quickjs unsafe in complete historical workspace; scoped success would not change that failed verdict.','source_tools':{str(p.relative_to(R)):rec(p) for p in [R/'scripts/cellos-sign',R/'scripts/cellos_sign/cli.py',R/'scripts/cellos_sign/signing.py',R/'scripts/cellos_sign/policy.py',R/'scripts/cellos_sign/scan.py',R/'scripts/cellos_sign/toolchain.py']},'executed_new_scoped_check':False,'executed_new_signing':False}
(O/'signing-scope-census.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({'cell_source_count':len(sources),'allowlisted_crates':entries,'file_allowlist_count':len(fileentries),'new_scoped_check_or_signing':False},indent=2))
