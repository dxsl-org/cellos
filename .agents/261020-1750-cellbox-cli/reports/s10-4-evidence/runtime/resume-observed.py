from pathlib import Path
import json,sys,shutil
from common import W,dump,need
from run import run_one,SCHEDULE
OLD=W/'runtime/execution-step-amended';OUT=W/'runtime/execution-echo-corrected';OUT.mkdir(exist_ok=False)
prior=json.loads((OLD/'execution.json').read_text());need(len(prior['records'])==7,'unexpected prior schedule');need(all(x['status']=='COMPLETE' and x['mode']=='untraced' for x in prior['records'][:6]),'timing records incomplete');need(prior['records'][6]['error'].startswith("StepFailed: echo of 'top --records -d 1 --frames 2 | to jsonl | cat' not seen:"),'unexpected prior stop')
shutil.copy2(OLD/'execution.json',OUT/'before-interleaved-echo-correction.json');shutil.copy2(OLD/'schedule.json',OUT/'schedule.json');records=prior['records'][:6]
for profile in ['headless','desktop']:
    print(f'BEGIN {profile} observed1 exact interleaved echo correction',flush=True)
    result=run_one(profile,'observed',1);records.append(result)
    dump(OUT/'execution.json',{'records':records,'remaining_unrun':[list(x) for x in SCHEDULE[len(records):]],'status':result['status'],'prior_interleaved_echo_failure':prior['records'][6],'timing_reruns':0,'amendment':'user2026-10-28 single native instruction current-hardware-breakpoint step-over only; full guard coverage retained','harness_correction':'exact echo prefix fragments across existing SERVICE_LINE/KERNEL_LINE only; background records retained, same new-prompt requirement/deadlines'})
    print(json.dumps({k:result.get(k) for k in ['profile','mode','number','status','error','observer_error','work']}),flush=True)
    if result['status']!='COMPLETE':sys.exit(1)
