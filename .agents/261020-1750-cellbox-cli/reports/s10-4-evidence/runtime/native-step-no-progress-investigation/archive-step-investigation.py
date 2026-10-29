from pathlib import Path
import json,hashlib,urllib.request,shutil
R=Path('/home/dmin/cellos');W=Path('/tmp/cellbox-s10-4/runtime');D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence/runtime/native-step-no-progress-investigation';D.mkdir(parents=True,exist_ok=True)
sources={'gdbstub/gdbstub.c':['static void handle_step','cpu_single_step(gdbserver_state.c_cpu','sstep_flags = SSTEP_ENABLE'],'gdbstub/system.c':['if (cpu->watchpoint_hit)','cpu->watchpoint_hit = NULL','cpu_single_step(cpu, 0)'],'system/watchpoint.c':['if (cpu->watchpoint_hit)','cpu_interrupt(cpu, CPU_INTERRUPT_DEBUG)'],'accel/tcg/cpu-exec.c':['if (interrupt_request & CPU_INTERRUPT_DEBUG)','cpu->interrupt_request &= ~CPU_INTERRUPT_DEBUG','if (unlikely(cpu->singlestep_enabled) && cpu->exception_index == -1)'],'target/i386/tcg/translate.c':['HF_RF_MASK','gen_reset_eflags(s, RF_MASK)','s->flags & HF_TF_MASK']}
evidence=[]
for name,needles in sources.items():
    url='https://raw.githubusercontent.com/qemu/qemu/v8.2.2/'+name;raw=urllib.request.urlopen(url,timeout=30).read();lines=raw.decode().splitlines();hits=[]
    for needle in needles:
        found=[n for n,line in enumerate(lines,1) if needle in line];assert found,(name,needle)
        for n in found:hits.append({'needle':needle,'line':n,'first':max(1,n-5),'text':lines[max(0,n-6):n+9]})
    evidence.append({'url':url,'sha256':hashlib.sha256(raw).hexdigest(),'excerpts':hits})
for name in ['watch-step-repro.S','watch-step-loop.S','watch-step-investigate.py','watch-step-loop-investigate.py']:
    shutil.copy2(W/name,D/name)
for folder in ['watch-step-investigation','watch-step-investigation-ready','watch-step-loop-investigation']:
    out=D/folder;out.mkdir(exist_ok=True)
    for p in (W/folder).iterdir():
        if p.is_file():shutil.copy2(p,out/p.name)
actual=Path('/tmp/cellbox-s10-4-runtime-headless-observed-1-7e63d8/evidence/rsp-traffic.jsonl');lines=[]
with actual.open() as f:
    for n,line in enumerate(f,1):
        if n>=17123:lines.append(json.loads(line))
report={'status':'UNAVAILABLE_PROVEN_SAFE_OBSERVER_STEP_COMPLETION_PREREQUISITE','product_defect':False,'candidate_retry_performed':False,'native_step_no_progress':{'actual_rip':'0x1080e40b1','instruction':'eb02 jmp 0x1080e40b5','pre_stop':'T05thread:01;watch:1088a80c0;','step_stop':'T05thread:01;','same_pc':True,'restoration_OK':True,'heap_aligned_used_at_stop':38104,'packets':lines,'raw_trace_sha256':hashlib.sha256(actual.read_bytes()).hexdigest()},'source_evidence':evidence,'interpretation':{'confirmed':'QEMU gdbstub clears watchpoint_hit after reporting watched stop, and CPU_INTERRUPT_DEBUG can be consumed before an instruction even in native single-step; bare T05 and unchangedPC are not affirmative instruction-retirement proof. Host single-step is CPUState/cflags, not a P/G register-write or guest-TF setter. RF/TF guest flags were not sampled at failed event; no claim of their actual values.','not_proven':'Cannot establish hidden pending-debug/cpu-exit state or specific ordering that caused THIS observed samePC from existing allowed reads/stop packet. Do not call the trap a completed instruction, auto-retry extra steps, drop a watch guard, or infer product hang.','minimal_falsification':'Isolated actual QEMU8.2.2 Q35/TCG bootsector watchedadd+postexecuteBP branches correctly on first single-step with/without preexisting executeBP. Repetitive isolated version completes1000 watched writes/firststeps, no samePC. Generic writewatch+postBP is insufficient to reproduce actual failure.'},'next_requirement':'Need independently observable instruction retirement/pending debug-state proof or an approved bounded semantic amendment for additional native control; no candidate change/full observed rerun authorized. Keep observed workloads incomplete, headless partial captured values labeled only partial interval, desktop observed unrun; static/timing complete; CBX27/criterion4 remain unmet.'}
(D/'diagnosis.json').write_text(json.dumps(report,indent=2)+'\n');shutil.copy2(Path(__file__),D/Path(__file__).name)
print(json.dumps({'status':report['status'],'isolated_single_steps':1002,'isolated_no_progress_reproductions':0,'candidate_retries':0,'source_urls':[x['url'] for x in evidence]}))
