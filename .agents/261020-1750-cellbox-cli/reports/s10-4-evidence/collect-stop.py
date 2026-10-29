#!/usr/bin/env python3
"""Record exact B2 prerequisite failure without changing source or measuring runtime."""
import hashlib,json,subprocess,tomllib
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); R=W/'baseline-worktree'
workspace=tomllib.loads((R/'Cargo.toml').read_text())['workspace']
missing=[m for m in workspace['members'] if not (R/m/'Cargo.toml').is_file()]
commands=json.loads((W/'build-commands.json').read_text())
rows={}
for member in missing:
    p=subprocess.run(['git','ls-tree','-r','3471028bc8b4db14275553b4740c17e24fa4f32b',member],cwd=R,capture_output=True,text=True,check=True)
    rows[member]=p.stdout
result={'status':'STOP_B2_PREREQUISITE','candidate':'4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821','baseline_head':'3471028bc8b4db14275553b4740c17e24fa4f32b','missing_workspace_members':missing,'tracked_tree_entries':rows,'commands':commands,'b2_build_exit':commands[-1]['returncode'],'b2_signed_unsigned_elf_results':None,'runtime_runs':0,'product_or_worktree_source_edits':False,'scope_change_authorized':False,'unmeasured':['per-ELF matched B2 sizes','new startup','heap/stack runtime high-water','idle redraw cost'],'disposition':'Await explicit Main/user choice; no changed revision, stub member, manifest removal, rebuild, signing bypass, candidate change, acceptance flip or promotion.'}
(W/'stop.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
