#!/usr/bin/env python3
"""Generate investigation evidence from trace/DMA files, keeping raw verdicts untouched."""
import collections
import hashlib
import json
from pathlib import Path
import shutil
import struct
B=Path('/tmp/cellbox-s10-3d-investigation')
R=Path('/home/dmin/cellos')
D=R/'.agents/261020-1750-cellbox-cli/reports/s10-3d-investigation'
D.mkdir(exist_ok=True)
sha=lambda p:hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
summary={'diagnostic_only':True,'probes':{},'canonical_changed':[],'product_source_changed':[]}
for run in sorted(B.glob('probe-*')):
    if not run.is_dir():continue
    target=D/run.name;target.mkdir(exist_ok=True)
    entry={}
    capture=run/'capture.json'
    if capture.exists():entry.update(json.loads(capture.read_text()))
    for name in ['admin-sq','admin-cq','io-sq','io-cq','identify','nvme-pci-config']:
        p=run/(name+'.bin')
        if p.exists():
            data=p.read_bytes()
            value={'sha256':sha(p),'nonzero_bytes':sum(x!=0 for x in data),'first64_hex':data[:64].hex()}
            if name.endswith('cq'):value['first_four_cqes']=[struct.unpack_from('<IIHHHH',data,i*16) for i in range(4)]
            entry[name]=value
    for p in run.iterdir():
        if p.is_file() and p.suffix in {'.json','.log','.trace','.bin'} and p.stat().st_size<16<<20:
            shutil.copy2(p,target/p.name)
    huge=run/'qemu.stderr.log'
    if huge.exists() and huge.stat().st_size>=16<<20:
        digest=hashlib.sha256();count=0;last=collections.deque(maxlen=20)
        with huge.open('rb') as src,(target/'qemu-trace-first10000.log').open('wb') as out:
            for line in src:
                digest.update(line);count+=1;last.append(line)
                if count<=10000:out.write(line)
        (target/'qemu-trace-last20.log').write_bytes(b''.join(last))
        entry['large_trace']={'raw_retained_path':str(huge),'bytes':huge.stat().st_size,'lines':count,'sha256':digest.hexdigest(),
                              'archive_excerpt':'first10000 and last20; raw remains outside repo'}
    summary['probes'][run.name]=entry
for root in ['platform-probe','platform-probe-io','nvme-probe']:
    dest=D/root;dest.mkdir(exist_ok=True)
    for p in (B/root).rglob('*'):
        if p.is_file() and (p.suffix=='.rs' or p.name in ['Cargo.toml','Cargo.lock']):
            q=dest/p.relative_to(B/root);q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,q)
for root in B.glob('instrumented*'):
    if root.is_dir():
        dest=D/root.name;dest.mkdir(exist_ok=True)
        for p in root.iterdir():
            if p.is_file() and p.suffix in {'.json','.log','.rs'}:shutil.copy2(p,dest/p.name)
for p in B.glob('*.py'):shutil.copy2(p,D/p.name)
canonical=json.loads(Path('/tmp/cellbox-s10-3c-resume/canonical/before.json').read_text())
for name,value in canonical.items():
    if sha(R/name)!=value['sha256']:summary['canonical_changed'].append(name)
source=json.loads(Path('/tmp/cellbox-s10-3c-resume/logs/execution.json').read_text())['source_hashes']
for name,value in source.items():
    if sha(R/name)!=value:summary['product_source_changed'].append(name)
summary['kernel_main_sha256']=sha(R/'kernel/src/main.rs')
summary['candidate_sha256']=sha(Path('/tmp/cellbox-s10-3c-resume/cand/candidate.json'))
summary['canonical_file_count']=len(canonical)
(D/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:v for k,v in summary.items() if k!='probes'},indent=2))
print('Archived',len(summary['probes']),'diagnostic attempts to',D)
