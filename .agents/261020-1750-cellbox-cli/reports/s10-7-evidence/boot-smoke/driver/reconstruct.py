import re,json,os,sys
SET=sys.argv[1]
out=f'/home/dmin/cellos/.agents/261020-1750-cellbox-cli/reports/s10-7-evidence/boot-smoke/{SET}'
rec=json.load(open(out+'/command.json'))
raw=open(out+'/serial.log','rb').read()
lines=raw.decode('utf-8','replace').replace('\r','').split('\n')
def sel(p): return [l for l in lines if re.search(p,l)]
t0=os.stat(out+'/command.json').st_mtime; t1=os.stat(out+'/serial.log').st_mtime
ready=bool(re.search(rb'Cellos > ',raw)) and not re.search(rb'KERNEL PANIC|panicked at|BOOT-FATAL',raw)
rec.update({'status':'READY' if ready else 'FAILED','reason':'',
 'result_reconstructed_from_serial_log':'smoke.py raised SameFileError in its post-processing (after the single boot attempt had reached ready and QEMU had been killed) and wrote no result.json; no boot was repeated. Timing derived from file mtimes: command.json written just before QEMU spawn, serial.log last write at kill (kill happens <=0.25 s after the prompt was seen).',
 'wall_time_to_ready_s_upper_bound':round(t1-t0,2),'wall_time_total_s':round(t1-t0,2),'start_epoch':t0,'timeout_s':900,'serial_bytes':len(raw),
 'markers':{'S22-X86-ADMISSION-ENABLED':sel(r'S22-X86-ADMISSION-ENABLED'),'stack-baseline':sel(r'\[stack-baseline\]'),
  'panic':sel(r'KERNEL PANIC|panicked|\bPANIC\b|BOOT-FATAL|[Aa]ssertion'),'FAIL':sel(r'\bFAIL\b'),'ready_line':sel(r'Cellos > $')[:3]},
 'final_60_serial_lines':lines[-60:]})
json.dump(rec,open(out+'/result.json','w'),indent=2)
print(rec['status'],rec['wall_time_to_ready_s_upper_bound'])
