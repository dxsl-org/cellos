import hashlib,importlib.util,json,pathlib,shutil,subprocess,tempfile,time
R=pathlib.Path('/home/dmin/cellos')
s=importlib.util.spec_from_file_location('vm',R/'scripts/x86/vmware-boot-test.py');v=importlib.util.module_from_spec(s);s.loader.exec_module(v)
vmrun=v.find_vmrun(None);assert v.IS_WSL_WIN
ws=pathlib.Path(tempfile.mkdtemp(prefix='native-cellos-profile-',dir='/mnt/c/Temp/cellos-vmware'))
print('NATIVE_PROBE '+str(ws),flush=True)
shutil.copyfile(R/'build/vicell-x86.iso',ws/'cellos.iso')
subprocess.run(['qemu-img','convert','-f','raw','-O','vmdk','-o','subformat=monolithicSparse,adapter_type=ide',str(R/'build/durable-charge-vmware-lab/native-profile-probe.img'),str(ws/'native.vmdk')],check=True)
text=v.make_vmx(v.to_host_path(ws/'cellos.iso'),v.to_host_path(ws/'serial.log'),1,256)
text=text.replace('sata0.present = "FALSE"','sata0.present = "TRUE"')
text+='sata0:0.present = "TRUE"\nsata0:0.fileName = '+v.vmx_string(v.to_host_path(ws/'native.vmdk'))+'\n'
vmx=ws/'cellos.vmx';vmx.write_text(text);(ws/'requested.vmx').write_text(text)
result={'scope':'Actual native Cellos VMware AHCI/CellosFS transport/mount feasibility only; no DCO1/journal/authority/durability qualification','iso_sha256':hashlib.sha256((ws/'cellos.iso').read_bytes()).hexdigest(),'path':str(ws),'errors':[],'cleanup':False}
started=False
try:
 started=True
 code=v.run_logged([str(vmrun),'-T','ws','start',v.to_host_path(vmx),'nogui'],ws/'start.log',30)
 if code:raise RuntimeError('vmrun start failed')
 time.sleep(90)
finally:
 if started:
  try:v.stopped_vm(vmrun,vmx,ws,30);result['cleanup']=True
  except Exception as e:result['errors'].append(str(e))
 raw=v.read_serial(ws/'serial.log');result['serial_sha256']=hashlib.sha256(raw).hexdigest();result['markers']=[line for line in raw.decode(errors='replace').splitlines() if any(x in line.lower() for x in ['ahci','nvme','block_driver','block driver','cellosfs','[vfs]','panic'])]
 (ws/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps(result,indent=2),flush=True)
