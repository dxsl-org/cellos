#!/usr/bin/env python3
"""Build fixed init/NVMe around the retained destructive-probe Platform, diagnostic only."""
import hashlib,json,os,re,shutil,struct,subprocess,sys
from pathlib import Path
R=Path('/home/dmin/cellos'); W=Path('/tmp/cellbox-s10-3d-resume'); O=W/'controlled'
C=Path('/tmp/cellbox-s10-3c-resume/cand')
env=dict(os.environ,CARGO_TARGET_DIR=str(W/'target'),RUSTFLAGS='-C relocation-model=pic')
env.pop('CARGO_ENCODED_RUSTFLAGS',None)
for name,args in [('nvme',['-p','driver-nvme']),('init',['-p','app-init','--features','input'])]:
    with (O/f'build-{name}.log').open('w') as out:
        subprocess.run(['cargo','build','--offline','--release','--target','x86_64-unknown-none','-Z','build-std=core,alloc',*args],cwd=R,env=env,stdout=out,stderr=subprocess.STDOUT,check=True)
embed=O/'embed';embed.mkdir()
for src,name in [('driver-nvme','nvme'),('app-init','init')]:
    shutil.copy2(W/'target/x86_64-unknown-none/release'/src,embed/name)
with (O/'sign.log').open('w') as out:
    subprocess.run([sys.executable,str(R/'scripts/cellos-sign'),'--sign',str(embed/'nvme'),str(embed/'init')],cwd=R,stdout=out,stderr=subprocess.STDOUT,check=True)
profile=json.loads((C/'candidate.json').read_text())['profiles']['headless']
platform=Path('/tmp/cellbox-s10-3d-investigation/instrumented/platform')
command=[sys.executable,str(R/'tools/mkfat32.py'),'--config-features','input','--config-output-dir',str(O/'boot-config'),str(embed/'kernel_fs.img')]
for destination,entry in profile['files'].items():
    if destination.startswith('/etc/cellos/'):continue
    replacement=platform if destination=='/bin/platform' else embed/'nvme' if destination=='/bin/nvme' else Path(entry['file'])
    command.extend([str(replacement),destination])
with (O/'image.log').open('w') as out:subprocess.run(command,cwd=R,stdout=out,stderr=subprocess.STDOUT,check=True)
kenv=dict(os.environ,CARGO_TARGET_DIR=str(O/'kernel-target'),EMBEDDED_OVERRIDE=str(embed))
kenv.pop('RUSTFLAGS',None);kenv.pop('CARGO_ENCODED_RUSTFLAGS',None)
with (O/'kernel.log').open('w') as out:
    subprocess.run(['cargo','build','--release','-p','cellos-kernel','--target','x86_64-unknown-none','-Z','build-std=core,alloc','--features','policy-required'],cwd=R,env=kenv,stdout=out,stderr=subprocess.STDOUT,check=True)
kernel=O/'kernel-target/x86_64-unknown-none/release/cellos-kernel'
with (O/'iso.log').open('w') as out:
    subprocess.run(['bash',str(R/'scripts/x86/make-iso-ci.sh'),str(O/'proof.iso')],cwd=R,env=dict(os.environ,X86_KERNEL=str(kernel),X86_ISO_ROOT=str(O/'iso-root')),stdout=out,stderr=subprocess.STDOUT,check=True)
# Reuse the earlier capture harness but give it this run's private output root.
probe=Path('/tmp/cellbox-s10-3d-investigation/probe.py').read_text().replace("BASE = Path('/tmp/cellbox-s10-3d-investigation')",f"BASE = Path('{O}')")
(O/'probe.py').write_text(probe)
subprocess.run([sys.executable,'-B',str(O/'probe.py'),'ordered',str(O/'proof.iso')],cwd=R,check=True)
run=O/'probe-ordered';serial=(run/'serial.log').read_text(errors='replace')
restore=serial.find('[probe] NVMe BAR probe resuming restore')
finished=serial.find('[platform] ECAM scan complete')
cap=serial.find('[nvme] CAP=')
result={'diagnostic_only':True,'probe_restored':restore>=0,'scan_complete_before_cap':0<=restore<finished<cap,
        'booted':json.loads((run/'capture.json').read_text())['reason']=='booted-to-shell',
        'no_nvme_timeout':'[nvme] admin timeout' not in serial and '[nvme] io timeout' not in serial,
        'platform_sha256':hashlib.sha256(platform.read_bytes()).hexdigest(),
        'init_sha256':hashlib.sha256((embed/'init').read_bytes()).hexdigest(),
        'nvme_sha256':hashlib.sha256((embed/'nvme').read_bytes()).hexdigest()}
sq=(run/'admin-sq.bin').read_bytes()
cq=(run/'admin-cq.bin').read_bytes()
identify=(run/'identify.bin').read_bytes()
first_cqe=struct.unpack_from('<IIHHHH',cq)
identify_address=int(re.search(r'identify=0x([0-9a-f]+)',serial)[1],16)
result['first_identify_submission']={
    'cdw0':struct.unpack_from('<I',sq)[0], 'prp1':struct.unpack_from('<Q',sq,24)[0],
    'cns':struct.unpack_from('<I',sq,40)[0], 'identify_address':identify_address}
result['first_identify_cqe']=first_cqe
result['intended_identify_completed']=(result['first_identify_submission']['cdw0']==0x10006
    and result['first_identify_submission']['prp1']==identify_address
    and result['first_identify_submission']['cns']==1
    and first_cqe[4:]==(1,1) and any(identify))
first_dma=serial.find('granted DMA BDF=')
result['scan_complete_before_dma']=0<=finished<first_dma
result['pass']=all(result[k] for k in ['probe_restored','scan_complete_before_cap',
    'scan_complete_before_dma','booted','no_nvme_timeout','intended_identify_completed'])
(O/'verdict.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
raise SystemExit(0 if result['pass'] else 1)
