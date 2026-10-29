#!/usr/bin/env python3
"""Read-only static B1/qualified-candidate accounting; no build or signature minting."""
import hashlib,importlib.util,json,re,shutil,struct,subprocess,sys,tomllib
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); R=Path('/home/dmin/cellos'); C=Path('/tmp/cellbox-s10-3d-resume/cand')
O=W/'static'; O.mkdir(exist_ok=True)
def sha(b):return hashlib.sha256(b).hexdigest()
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path); m=importlib.util.module_from_spec(spec); sys.modules[name]=m; spec.loader.exec_module(m); return m
reader=load('s104_image_reader',R/'scripts/generate-boot-config.py')
signer=load('s104_signer',R/'scripts/sign-cell.py')
candraw=(C/'candidate.json').read_bytes(); assert sha(candraw)=='4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821'
cand=json.loads(candraw); names=['shell','ls','cat','echo','ps','kill']; workers=names[1:]
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
pub=signer._pub_bytes(signer._priv_from_seed(signer.DEV_SEED))
def section_rows(data):
    bits,off,ents,count,strings=signer._section_table(data); assert bits==64
    rows=[struct.unpack_from('<IIQQQQIIQQ',data,off+i*ents) for i in range(count)]
    st=rows[strings]; text=data[st[4]:st[4]+st[5]]
    return [{'name':text[x[0]:].split(b'\0',1)[0].decode(),'type':x[1],'flags':x[2],'address':x[3],'offset':x[4],'bytes':x[5]} for x in rows]
def elf(path):
    data=path.read_bytes(); sections=section_rows(data); sig=[s for s in sections if s['name']=='__ViCell_sig']; valid=None
    if sig:
        try: Ed25519PublicKey.from_public_bytes(pub).verify(data[sig[0]['offset']:sig[0]['offset']+sig[0]['bytes']],signer._signed_payload(data)); valid=True
        except Exception: valid=False
    size=subprocess.run(['size','-B','-d',str(path)],capture_output=True,text=True,check=True).stdout
    values=size.splitlines()[1].split()
    return {'path':str(path),'file_bytes':len(data),'sha256':sha(data),'signature_sections':len(sig),'signature_payload_bytes':sum(s['bytes'] for s in sig),'dev_signature_valid':valid,'size_text_bytes':int(values[0]),'size_data_bytes':int(values[1]),'size_bss_bytes':int(values[2]),'sections':sections,'size_output':size}
def describe(label,name,data,retained_unsigned=None):
    d=O/label; d.mkdir(exist_ok=True); path=d/name; path.write_bytes(data); row=elf(path)
    if row['signature_sections']:
        unsigned=d/(name+'.derived-unsigned')
        p=subprocess.run(['objcopy','--remove-section=__ViCell_sig',str(path),str(unsigned)],capture_output=True,text=True,check=True)
        row['derived_unsigned']=elf(unsigned); row['derived_unsigned']['kind']='section-removed derivative, NOT original pre-sign ELF'
    else:row['derived_unsigned']=None
    row['original_unsigned']=None
    if retained_unsigned and retained_unsigned.exists() and row['signature_sections']:
        placeholder=d/(name+'.placeholder')
        signer._add_signature_placeholder(str(retained_unsigned),str(placeholder),'objcopy')
        match=signer._signed_payload(placeholder.read_bytes())==signer._signed_payload(data)
        row['retained_unsigned_payload_matches']=match
        if match: row['original_unsigned']=elf(retained_unsigned)
        placeholder.unlink()
    return row
baseline=R/'.agents/261020-1750-cellbox-cli/baseline.md'
expected={n:(int(sz),h) for n,sz,h in re.findall(r'\| `/bin/([^`]+)` \| (\d+) \| `([0-9a-f]{64})` \|',baseline.read_text())}
b1path=R/'kernel/src/embedded-x86_64/kernel_fs.img'; b1data=reader.image_files(b1path)
b1=[]
for n in names:
    data=b1data['/bin/'+n]; assert (len(data),sha(data))==expected[n],n
    b1.append(describe('b1',n,data))
assert sum(x['file_bytes'] for x in b1)==980528
profiles={}
for profile in ['headless','desktop']:
    p=cand['profiles'][profile]; img=Path(p['kernel_fs_img']['path']); data=reader.image_files(img)
    assert sha(img.read_bytes())==p['kernel_fs_img']['sha256']
    entries=[]
    for n in ['cellbox',*workers,'cellbox-ssh']:
        binary='cellbox' if n=='cellbox-ssh' else n
        raw=data['/bin/'+n]; assert sha(raw)==p['files']['/bin/'+n]['sha256']
        entries.append(describe(profile,n,raw,Path('/tmp/cellbox-s10-3d-resume/target/x86_64-unknown-none/release')/binary))
    assert data['/bin/cellbox']==data['/bin/cellbox-ssh']
    profiles[profile]={'elfs':entries,'suite_only_signed_bytes':entries[0]['file_bytes'],'workers_signed_bytes':sum(x['file_bytes'] for x in entries[1:6]),'six_elf_set_signed_bytes':sum(x['file_bytes'] for x in entries[:6]),'seven_packaged_elf_bytes_including_alias':sum(x['file_bytes'] for x in entries),'physical_alias_copy_bytes':entries[-1]['file_bytes'],'physical_alias_count':2,'packaged_file_count':len(data),'all_file_payload_bytes':sum(map(len,data.values())),'packaged_elf_count':sum(v.startswith(b'\x7fELF') for v in data.values()),'all_elf_file_bytes':sum(len(v) for v in data.values() if v.startswith(b'\x7fELF')),'image':p['kernel_fs_img'],'kernel':p['kernel'],'iso':p['iso'],'init_features':p['manifest']['init_features'],'kernel_features':p['kernel']['features']}
    profiles[profile]['six_set_signed_delta_vs_b1']=profiles[profile]['six_elf_set_signed_bytes']-980528
    profiles[profile]['physical_seven_set_delta_vs_b1']=profiles[profile]['seven_packaged_elf_bytes_including_alias']-980528
    with (O/f'inspect-fat-{profile}.log').open('w') as log:subprocess.run(['python3',str(R/'tools/inspect_fat.py'),str(img)],stdout=log,stderr=subprocess.STDOUT,check=True)
with (O/'inspect-fat-b1-current-container.log').open('w') as log:subprocess.run(['python3',str(R/'tools/inspect_fat.py'),str(b1path)],stdout=log,stderr=subprocess.STDOUT,check=True)
result={'candidate_sha256':sha(candraw),'candidate_source':cand['source'],'b1':{'recorded_six_bytes':980528,'recorded_shell_bytes':881120,'recorded_workers_bytes':99408,'six_elfs':b1,'source':'six original baseline ELFs read back byte-identical from current canonical447d0348 container; container is NOT original699e1df historical inventory','current_container_file_bytes':b1path.stat().st_size,'current_container_sha256':sha(b1path.read_bytes()),'all_current_container_file_payload_bytes':sum(map(len,b1data.values())),'original_signing_provenance':'unverified; observed signature/dev verification reported independently','runtime_baseline':'not available; never boot this unsafe legacy image as prerequisite'},'profiles':profiles,'pc':cand['profiles']['pc'],'unsigned_caveat':'Only retained unsigned artifacts whose stable signing payload equals selected packaged ELF are original matching unsigned measurements; all section-removed derivatives are explicitly not original pre-sign ELFs. Signed payload excludes64 signature bytes but is not unsigned ELF file size.','memory':{'cellbox_arena_reservation_bytes':6*1024*1024,'ostd_arena_reservation_bytes':1024*1024,'resident_bytes':None,'peak_heap_bytes':None,'peak_stack_bytes':None,'ceiling':'ELF section/arena reservations only; no runtime resident, allocator peak or stack high-water observation'},'feature_equivalence':False,'feature_differences':['rg','edit','SSH outbound client','top record/TUI','typed pipelines','Ratatui frontend','authority/save/deadline/bootstrap corrections'],'claim':'Feature sets and whole-system image profiles are not equivalent. All positive deltas are regressions in file footprint, not evidence of universal optimization or resident-memory use.'}
(O/'measurement.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'b1_bytes':980528,'profiles':{n:{k:p[k] for k in ['suite_only_signed_bytes','workers_signed_bytes','six_elf_set_signed_bytes','seven_packaged_elf_bytes_including_alias','six_set_signed_delta_vs_b1','physical_seven_set_delta_vs_b1']} for n,p in profiles.items()}},indent=2))
