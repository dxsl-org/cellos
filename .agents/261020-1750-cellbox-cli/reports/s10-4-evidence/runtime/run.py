#!/usr/bin/env python3
"""S10-4.4 exact approved measurement schedule; no candidate build/sign/write."""
import json, secrets, time, traceback
from pathlib import Path
from common import Machine, Surface, W, Stop, dump, need
from observer import Observer
OUT=W/'runtime/execution-contract-correct'
SCHEDULE=[(p,'untraced',i) for p in ['headless','desktop'] for i in range(1,4)]+[(p,'observed',1) for p in ['headless','desktop']]
def run_one(profile,mode,number):
    work=Path('/tmp')/f'cellbox-s10-4-runtime-{profile}-{mode}-{number}-{secrets.token_hex(3)}'
    record={'profile':profile,'mode':mode,'number':number,'work':str(work),'status':'STARTING','start_monotonic_ns':time.monotonic_ns()};m=None;obs=None;s=None
    try:
        m=Machine(profile,work,mode=='observed');m.start();record['qemu_start_monotonic_ns']=m.start_ns;record['qemu_pid']=m.proc.pid
        if mode=='observed':
            obs=Observer(m);obs.start()
            while not obs.ready.wait(.1):need(time.monotonic_ns()-m.start_ns<900_000_000_000,'observer setup exceeded900s')
            obs.check()
        s=Surface(m,obs);s.ready();record['timestamps']=s.timestamps
        record['idle']=s.sample_idle('ordinary-prompt-idle')
        if profile=='headless' or mode=='observed':record['long_stream']=s.streams()
        record['top_idle']=s.top_idle();s.phase('complete');s.snapshot('complete');record['status']='COMPLETE'
    except Exception as e:
        record.update(status='STOP',error=f'{type(e).__name__}: {e}',traceback=traceback.format_exc())
    finally:
        if s:record['surface_events']=s.events
        if obs:
            try:obs.shutdown()
            except Exception as e:
                record.update(status='STOP',observer_error=f'{type(e).__name__}: {e}')
        if m:
            try:m.stop()
            except Exception as e:record.update(status='STOP',cleanup_error=f'{type(e).__name__}: {e}')
        record['end_monotonic_ns']=time.monotonic_ns()
        if work.exists():dump(work/'evidence/runtime-result.json',record)
        if record['status']=='COMPLETE':
            # Only our disposable cloned disk; original fixture and candidate untouched.
            (work/'disk.img').unlink()
    return record

def main():
    OUT.mkdir(parents=True,exist_ok=False);records=[]
    dump(OUT/'schedule.json',{'approved':'user2026-10-28 D4-Obs1/2/3 A','schedule':SCHEDULE})
    for profile,mode,i in SCHEDULE:
        print(f'BEGIN {profile} {mode} {i}',flush=True)
        result=run_one(profile,mode,i);records.append(result)
        dump(OUT/'execution.json',{'records':records,'remaining_unrun':[list(x) for x in SCHEDULE[len(records):]],'status':result['status']})
        print(json.dumps({k:result.get(k) for k in ['profile','mode','number','status','error','observer_error','work']}),flush=True)
        if result['status']!='COMPLETE':return 1
    return 0
if __name__=='__main__':raise SystemExit(main())
