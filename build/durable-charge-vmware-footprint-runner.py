#!/usr/bin/env python3
"""Disposable VMware P01 diagnostic capture; never QEMU qualification or G3 proof."""
import argparse, hashlib, importlib.util, json, pathlib, re, shutil, subprocess, sys, tempfile, time, uuid
ROOT=pathlib.Path('/home/dmin/cellos')
sys.path.insert(0,str(ROOT/'scripts/x86'))
import p01_trace
spec=importlib.util.spec_from_file_location('vmboot',ROOT/'scripts/x86/vmware-boot-test.py')
v=importlib.util.module_from_spec(spec);spec.loader.exec_module(v)
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def save(p,obj): p.write_text(json.dumps(obj,indent=2)+'\n')
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--counts',nargs='+',default=['0']);ap.add_argument('--replicas',type=int,default=1);args=ap.parse_args()
 vmrun=v.find_vmrun(None)
 if not v.IS_WSL_WIN: raise RuntimeError('This disposable capture selects the observed WSL/Windows VMware tuple only')
 build=ROOT/'build/p01-budget-stage-ownership';manifest=json.loads((build/'build.json').read_text())
 parent=pathlib.Path('/mnt/c/Temp/cellos-vmware');parent.mkdir(parents=True,exist_ok=True)
 batch=pathlib.Path(tempfile.mkdtemp(prefix='durable-charge-footprint-',dir=parent));rows=[]
 print('BATCH '+str(batch),flush=True)
 for count in args.counts:
  for replica in range(args.replicas):
   ws=batch/str(uuid.uuid4());ws.mkdir();serial=ws/'serial.log';vmx=ws/'cellos.vmx';started=False;start=None
   meta={'backend':'vmware-workstation','scope':'Pinned historical root-only P01 fixture diagnostic; not QEMU matrix, budget selection, G3, I/O or physical durability qualification','count':count,'replica':replica,'cpus':1,'memory_mib':256,'window_seconds':150,'build_sha256':sha(build/'build.json'),'parser_sha256':sha(ROOT/'scripts/x86/p01_trace.py'),'runner_sha256':sha(__file__),'manifest_hex':''.join(manifest['signed'][n] for n in ('app-init','bench','bench-probe')),'errors':[],'cleanup_confirmed':False}
   try:
    for name,digest in manifest['scenarios'][count].items():
     if sha(build/name)!=digest: raise RuntimeError('Pinned fixture changed: '+name)
    shutil.copyfile(build/count/'image.iso',ws/'cellos.iso');shutil.copyfile(build/'build.json',ws/'build.json')
    for name in ['autoload.toml','services.toml']:shutil.copyfile(build/count/'override'/name,ws/name)
    meta['iso_sha256']=sha(ws/'cellos.iso')
    text=v.make_vmx(v.to_host_path(ws/'cellos.iso'),v.to_host_path(serial),1,256)
    vmx.write_text(text);(ws/'requested.vmx').write_text(text)
    command=[str(vmrun),'-T','ws','start',v.to_host_path(vmx),'nogui'];meta['command']=command
    start=time.monotonic();started=True
    code=v.run_logged(command,ws/'vmrun-start.log',30)
    if code:raise RuntimeError('VM start failed: '+str(code))
    # Observe the entire bounded window; terminal workload markers do not end capture early.
    while time.monotonic()-start<150:
     raw=v.read_serial(serial)
     if re.search(rb'(?:panicked at|KERNEL PANIC|triple fault|P01-FAULT|\[fault\] Cell)',raw,re.I):raise RuntimeError('Guest fault; retain full serial evidence')
     time.sleep(min(.2,max(0,150-(time.monotonic()-start))))
   except Exception as e:meta['errors'].append(str(e))
   finally:
    if started:
     try:v.stopped_vm(vmrun,vmx,ws,30);meta['cleanup_confirmed']=True
     except Exception as e:meta['errors'].append('cleanup: '+str(e))
    raw=v.read_serial(serial);meta['raw_sha256']=hashlib.sha256(raw).hexdigest();meta['elapsed_seconds']=time.monotonic()-start if start else None
    try:
     if re.search(rb'(?:panicked at|KERNEL PANIC|triple fault|P01-FAULT|\[fault\] Cell)',raw,re.I):raise RuntimeError('Guest fault in final retained serial')
     evidence=p01_trace.validate_stream(raw,meta)
     save(ws/'stream-evidence.json',evidence)
     meta['stream_structurally_complete']=evidence.get('structurally_complete');meta['stream_fully_settled']=evidence.get('fully_settled');meta['stream_blockers']=evidence.get('qualification_blockers',[])
    except Exception as e:meta['errors'].append('stream diagnostic: '+str(e))
    log=ws/'vmware.log'
    if log.exists():
     match=re.search(r'Log for VMware Workstation[^\r\n]*',log.read_text(errors='replace'))
     if match:meta['backend_version']=match.group(0)
    save(ws/'result.json',meta);rows.append({'path':str(ws),'result':meta});save(batch/'matrix.json',{'scope':'VMware diagnostics only; no production or QEMU qualification','runs':rows})
    print(json.dumps({'count':count,'replica':replica,'errors':meta['errors'],'fully_settled':meta.get('stream_fully_settled'),'cleanup':meta['cleanup_confirmed'],'path':str(ws)}),flush=True)
   if meta['errors'] or not meta['cleanup_confirmed']:return 1
 return 0
if __name__=='__main__':sys.exit(main())
