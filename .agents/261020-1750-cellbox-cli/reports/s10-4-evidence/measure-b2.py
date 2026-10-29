#!/usr/bin/env python3
"""Measure real B2 products and audit the approved disposable baseline deviations."""
import hashlib,importlib.util,json,re,subprocess,sys,tomllib
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); B=W/'baseline-worktree'; R=Path('/home/dmin/cellos')
def sha(b):return hashlib.sha256(b).hexdigest()
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
signer=load('s104_b2_signer',B/'scripts/sign-cell.py')
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
pub=signer._pub_bytes(signer._priv_from_seed(signer.DEV_SEED))
assert sha(pub)=='7db0b01f39d179aacf7b03dbd7dd9d71f641598bed40fb9839d3c1e1117e666f'
def artifact(path,signed):
    b=path.read_bytes();out=subprocess.run(['size','-B','-d',str(path)],capture_output=True,text=True,check=True).stdout;v=out.splitlines()[1].split();row={'path':str(path),'bytes':len(b),'sha256':sha(b),'text_bytes':int(v[0]),'data_bytes':int(v[1]),'bss_bytes':int(v[2]),'size_output':out}
    bits,off,ents,count,strings=signer._section_table(b);sig=signer._find_section(b,signer.SIG_SECTION,off,ents,count,strings,bits)
    if signed:
        assert sig and len(sig)==64
        Ed25519PublicKey.from_public_bytes(pub).verify(sig,signer._signed_payload(b))
        row['dev_signature_valid']=True
    else:assert sig is None,'original unsigned has signature section'
    return row
rows=[]
for n in ['shell','ls','cat','echo','ps','kill']:
    u=artifact(W/'b2-unsigned'/n,False)
    staged=(W/'b2-signed'/n).read_bytes()
    bits,off,ents,count,strings=signer._section_table(staged)
    sig=signer._find_section(staged,signer.SIG_SECTION,off,ents,count,strings,bits)
    s=artifact(W/'b2-signed'/n,True) if sig else None
    rows.append({'name':n,'unsigned':u,'signed':s,'unsigned_staging_copy_not_signed':sig is None})
oldraw=(W/'lock-before-reconciliation.lock').read_text();newraw=(B/'Cargo.lock').read_text()
def blocks(raw):
    result={}
    for block in raw.split('[[package]]')[1:]:
        p=tomllib.loads('[[package]]'+block)['package'][0];result[(p['name'],p['version'],p.get('source'))]=block.rstrip('\n')
    return result
old,new=blocks(oldraw),blocks(newraw)
changed=[list(k) for k in old if old[k]!=new.get(k)];assert not changed,'existing lock entry changed'
audit=json.loads((W/'lock-audit.json').read_text());assert not audit['removed'] and not audit['same_version_package_metadata_changed'];assert all(not p['in_measured_graph_by_name'] and not p['package'].get('source') for p in audit['added'])
static=json.loads((W/'static/measurement.json').read_text())
complete=all(r['signed'] is not None for r in rows)
total_signed=sum(r['signed']['bytes'] for r in rows) if complete else None
total_unsigned=sum(r['unsigned']['bytes'] for r in rows)
result={'status':'B2_BUILD_SIGN_STATIC_COMPLETE' if complete else 'STOP_B2_CHECKED_SIGNING_F1','baseline_head':'3471028bc8b4db14275553b4740c17e24fa4f32b','non_pristine':True,'deviations':['User-authorized root workspace member vm-runner exclusion only; not in selected compile graph, runtime spawn-string nuance retained.','Preferred vm-runner lock entry removal impossible: no such entry. Explicitly authorized offline metadata fallback added only local app-chrome/files/terminal0.1.0; none in measured graph. All original lock package blocks byte-identical.'],'original_lock_blocks':len(old),'all_existing_lock_blocks_byte_identical':True,'added_local_lock_packages':[p['package']['name'] for p in audit['added']],'registry_version_source_changes':[],'dev_public_key_sha256':sha(pub),'artifacts':rows,'six_signed_bytes':total_signed,'six_unsigned_bytes':total_unsigned,'suite_unsigned_bytes':rows[0]['unsigned']['bytes'],'workers_unsigned_bytes':sum(r['unsigned']['bytes'] for r in rows[1:]),'suite_signed_bytes':rows[0]['signed']['bytes'] if complete else None,'workers_signed_bytes':sum(r['signed']['bytes'] for r in rows[1:]) if complete else None,'signed_delta_vs_b1':total_signed-980528 if complete else None,'after_vs_b2':None,'runtime_comparator_available':False,'feature_equivalent':False,'build_commands':json.loads((W/'build-commands-reconciled.json').read_text()),'lock_audit':audit}
desktop=static['profiles']['desktop']['elfs'][:6]
result['desktop_original_unsigned_six_bytes']=sum(r['original_unsigned']['file_bytes'] for r in desktop)
result['desktop_unsigned_six_delta_vs_b2']=result['desktop_original_unsigned_six_bytes']-total_unsigned
result['signing_refusal']='Historical workspace F1 unallowlisted unsafe token in unrelated ocel-quickjs/native_layout.rs; F5 passed. No bypass/allowlist edit or valid signed B2 result.' if not complete else None
(W/'b2-measurement.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:result[k] for k in ['status','six_signed_bytes','six_unsigned_bytes','suite_signed_bytes','workers_signed_bytes','signed_delta_vs_b1','after_vs_b2','all_existing_lock_blocks_byte_identical','added_local_lock_packages']},indent=2))
