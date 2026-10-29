#!/usr/bin/env python3
"""Archive protocol design prerequisites only; never start a VM or invoke new signing scope."""
import hashlib,json,re,shutil,subprocess
from pathlib import Path
W=Path('/tmp/cellbox-s10-4');R=Path('/home/dmin/cellos');D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence';O=D/'protocol'
O.mkdir(exist_ok=True)
for p in (W/'observability').iterdir():
    if p.is_file() and not p.name.endswith('.disassembly.txt'):shutil.copy2(p,O/p.name)
for n in ['inspect-observability.py','inspect-heap-writes.py','inspect-signing-scope.py','archive-protocol.py']:shutil.copy2(W/n,O/n)
lanes=R/'.agents/261020-1750-cellbox-cli/reports/s10-3d-evidence/rerun/lanes';timing=[]
for i in range(1,7):
    path=lanes/f'L0-headless-run{i}/verdict.json';data=json.loads(path.read_text());m=data['markers']['P10C-boot'];timing.append({'lane':f'L0-headless-run{i}','source':str(path),'sha256':hashlib.file_digest(path.open('rb'),'sha256').hexdigest(),'prompt_detail':m['checks']['prompt-reached']['detail'],'marker_pass':m['pass'],'metric':'Retained time.time() to first raw Cellos > substring, rounded0.1s; not monotonic/exact-idle protocol and not total fixture wall time.'})
(O/'retained-startup.json').write_text(json.dumps(timing,indent=2)+'\n')
source_paths=['libs/ostd/src/heap.rs','cells/tools/cellbox/src/main.rs','kernel/src/task/stack.rs','kernel/src/task.rs','kernel/src/task/syscall.rs','kernel/src/task/telemetry.rs','kernel/src/loader/va_alloc.rs','kernel/src/loader/elf.rs','kernel/src/loader/launch_profile/mod.rs','kernel/src/loader/launch_profile/profiles.rs','scripts/cellos_sign/cli.py','scripts/cellos_sign/signing.py','scripts/cellos_sign/policy.py','scripts/cellos_sign/scan.py','scripts/cellos_sign/toolchain.py']
source_hashes={}
for n in source_paths:
    p=R/n;out=O/'source'/n;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out);source_hashes[n]=hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
heap=Path('/home/dmin/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/linked_list_allocator-0.10.6/src/lib.rs');shutil.copy2(heap,O/'linked-list-allocator-0.10.6-lib.rs')
record={
    'status':'PROPOSED_SEPARATE_CP1_ONLY',
    'decision_ids':['D4-Obs1','D4-Obs2','D4-Obs3','D4-Sign1'],
    'runtime_observer_implemented':False,
    'new_vm_runs':0,
    'new_scoped_f1_f5_invocations':0,
    'new_signatures':0,
    'gdb_binary_available':shutil.which('gdb') is not None,
    'qemu_version_observed':'8.2.2 Debian1:8.2.2+ds-0ubuntu1.17',
    'primary_sources':[
        'https://github.com/qemu/qemu/blob/v8.2.2/docs/system/gdb.rst',
        'https://github.com/qemu/qemu/blob/v8.2.2/gdbstub/system.c',
    ],
    'qemu_documented_capability':'TCG system emulation advertises hardware-assisted breakpoints/watchpoints; use private Unix gdbstub socket. Watch stop reports configured watchpoint.vaddr, not a byte-precise effective access.',
    'ELF_observability':'B1/B2 shell and candidate .symtab retained, no DWARF; HEAP is0x30B with proven used-word at offset0, no persistent peak counter. B1/B2 ps have no retained heap symbols.',
    'heap_metric':'Max aligned live allocator-used counter after every add/sub through hardware-execute sites, plus8B counter write-watch coverage guard rejecting unknown writer PC/semantics; not payload, whole-cell memory or RSS. Not exercised.',
    'stack_metric':'First CPU write-attempt per4KiB usable-stack page; disarm after first hit, record page/depth bounds and workload completion separately. Not retired-store proof, exact RSP/byte peak, kernel stack or DMA coverage; no sentinel/guest writes. Not exercised.',
    'baseline_limit':'No approved matched runtime carrier/launch/policy for legacy B1/B2 shell; do not boot unsafe canonical image or rename/replace cellbox. Runtime-before/after comparability and CBX27/criterion4 unmet.',
    'signing_proposal':'Existing CLI --repo against explicitly reviewed source-exact38-Rust-file/12-root release-tarball-style mirror;8 total local crate directories, unchanged original allowlist/app-shell exception. New scoped input approval required. Current named routes do not scope F1 and cannot authenticate historical shell recipe. No allowlist/guard edits.',
    'source_sha256':source_hashes,
    'allocator_source_sha256':hashlib.file_digest(heap.open('rb'),'sha256').hexdigest(),
    'retained_startup_notes':timing,
}
proposal=(R/'.agents/261020-1750-cellbox-cli/phase-10-s0-proposal.md').read_text()
cp1='#### S10-4.4 '+proposal.split('#### S10-4.4 ',1)[1].split('### S10-5 ',1)[0]
(O/'cp1-proposal.txt').write_text(cp1)
record['cp1_proposal_sha256']=hashlib.sha256(cp1.encode()).hexdigest()
proof=json.loads((W/'canonical/comparison-after-protocol.json').read_text())
record['preservation']=proof
shutil.copytree(W/'canonical',D/'canonical',dirs_exist_ok=True)
summary=json.loads((D/'summary.json').read_text())
summary['protocol_cp1']='protocol/proposal-evidence.json'
summary['protocol_status']='PROPOSED_SEPARATE_CP1_ONLY'
summary['final_preservation']=proof
(D/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
(O/'proposal-evidence.json').write_text(json.dumps(record,indent=2)+'\n')
checks={str(p.relative_to(D)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in D.rglob('*') if p.is_file() and p.name!='evidence.sha256'}
(D/'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k,v in sorted(checks.items())))
print(json.dumps({'status':record['status'],'new_vm_runs':0,'new_signatures':0,'gdb_available':record['gdb_binary_available'],'retained_first_prompt_details':[x['prompt_detail'] for x in timing],'archive':str(O)},indent=2))
