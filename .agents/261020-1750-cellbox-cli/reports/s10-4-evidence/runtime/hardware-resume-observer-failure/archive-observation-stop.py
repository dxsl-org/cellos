from pathlib import Path
import hashlib,json,gzip,shutil
W=Path('/tmp/cellbox-s10-4-runtime-headless-observed-1-f1172c');D=Path('/home/dmin/cellos/.agents/261020-1750-cellbox-cli/reports/s10-4-evidence/runtime/hardware-resume-observer-failure');D.mkdir(parents=True,exist_ok=True)
source=W/'evidence/rsp-traffic.jsonl';dst=Path('/tmp/cellbox-s10-4/runtime/failed-hardware-resume-trace.jsonl.gz');sha=hashlib.sha256();size=0
with source.open('rb') as inf:
    if dst.exists():
        while chunk:=inf.read(4*1024*1024):sha.update(chunk);size+=len(chunk)
    else:
        with dst.open('wb') as out,gzip.GzipFile(filename='',fileobj=out,mode='wb',compresslevel=1,mtime=0) as zipped:
            while chunk:=inf.read(4*1024*1024):sha.update(chunk);size+=len(chunk);zipped.write(chunk)
for p in (W/'evidence').iterdir():
    if p.is_file() and p!=source:shutil.copy2(p,D/p.name)
sample=[];pc=[];last=None
with source.open() as stream:
    for n,line in enumerate(stream,1):
        if n>10000:break
        row=json.loads(line)
        if n>=3100:sample.append(row)
        if row.get('event')=='packet_tx':last=bytes.fromhex(row['payload_hex'])
        elif row.get('event')=='packet_rx' and last==b'p10':pc.append(int.from_bytes(bytes.fromhex(bytes.fromhex(row['payload_hex']).decode()),'little'));last=None
obs=json.loads((W/'evidence/observation.json').read_text());run=json.loads((W/'evidence/runtime-result.json').read_text())
assert len(pc)>100 and set(pc)=={0x100000000,0x102000000} and not obs['instances'] and not obs['heap_post_event_count']
report={'classification':'hardware execute breakpoint resume semantics observation-method blocker; not product timeout evidence','raw_trace':{'source':str(source),'bytes':size,'sha256':sha.hexdigest(),'archive':str(dst),'compressed_bytes':dst.stat().st_size,'lossless_gzip':True},'first10000lines':{'rip_read_count':len(pc),'unique_rip_values':[hex(x) for x in sorted(set(pc))],'packets_from_line3100':sample},'runtime_stop':run['error'],'partial_observation':obs,'mechanism':'Retained hardware execute breakpoint at native Init entry retriggers each c without architectural advancement. Current policy excludes native s; guard prohibits silently dropping persistent coverage or broadening packet policy.','next_decision':'User2026-10-28 explicitly authorizes single native instruction step-over with only current hardware execute breakpoint removed/restored; retain all write-watch guards and other breakpoint/policy coverage. Two fresh observed lanes; no build/product/canonical changes.'}
(D/'diagnosis.json').write_text(json.dumps(report,indent=2)+'\n');shutil.copy2(Path(__file__),D/Path(__file__).name)
print(json.dumps({k:report[k] for k in ['classification','raw_trace']}))
