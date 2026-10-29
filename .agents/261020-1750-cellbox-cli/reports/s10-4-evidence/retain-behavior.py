#!/usr/bin/env python3
"""Carry existing behavioral verdicts forward at their original ceiling, never as new measurements."""
import hashlib,json,shutil
from pathlib import Path
R=Path('/home/dmin/cellos'); W=Path('/tmp/cellbox-s10-4'); D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence'
S=R/'.agents/261020-1750-cellbox-cli/reports/s10-3d-evidence/rerun/lanes'
source=S/'L3-p04-terminal/verdict.json'; data=json.loads(source.read_text())
terms=['endless','consumer-close','rg-fx-big','no-oom','redraw']
checks={k:v for k,v in data['checks'].items() if any(t in k for t in terms)}
assert all(v['pass'] for v in checks.values())
l0=S/'l0-classification.json'
record={'candidate_sha256':'4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821','new_runtime_runs':0,'l0':{'source':str(l0),'sha256':hashlib.file_digest(l0.open('rb'),'sha256').hexdigest(),'ceiling':'Existing qualified canonical six-consecutive headless/desktop behavior only; seconds fields are fixture wall time, not cold-start.'},'l3':{'source':str(source),'sha256':hashlib.file_digest(source.open('rb'),'sha256').hexdigest(),'checks':checks,'ceiling':'Private imported P04 fixture/payload-match behavior, not canonical-kernel runtime memory/throughput.'},'bounded_recovery':{'endless_local_sink_ctrl_c_to_prompt_ms':9,'consumer_close_ms':56,'scope':'Driver-recorded fixture values; not universal latency or performance ceilings.'},'rg_fixture':{'reported_elapsed_field':301519,'file_bytes':7340032,'scope':'Exact retained fixture detail; clock-unit caveats retained, not host/QEMU throughput.'},'idle_redraw':'Retained reattach/redraw correctness passes; idle redraw rate, CPU cost and latency not measured.','new_cold_start_ms':None,'resident_bytes':None,'peak_heap_bytes':None,'peak_stack_bytes':None,'no_optimization_claim':True}
(W/'behavior.json').write_text(json.dumps(record,indent=2)+'\n')
shutil.copy2(W/'behavior.json',D/'behavior.json');shutil.copy2(W/'retain-behavior.py',D/'retain-behavior.py')
(D/'retained').mkdir(exist_ok=True)
shutil.copy2(source,D/'retained/L3-verdict.json');shutil.copy2(l0,D/'retained/l0-classification.json')
print('Archived retained L0/L3 evidence at original behavioral ceiling; no new runtime measurements')
