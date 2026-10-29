#!/usr/bin/env python3
"""Generate S10-4 actual combined evidence; never turn missing metrics into success."""
import hashlib,json,re,shutil,statistics,sys
from pathlib import Path
W=Path('/tmp/cellbox-s10-4');R=Path('/home/dmin/cellos');D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence'
sha=lambda p:hashlib.file_digest(Path(p).open('rb'),'sha256').hexdigest()
# Preserve the actual synchronous tool stdout instead of rerunning successful tests for logging.
history=Path('/home/dmin/.omp/agent/sessions/-cellos/2026-10-24T04-34-37-499Z_01a16c30-753b-761c-9ba5-945324b1493f/S103cResume.jsonl')
unit=None
for line in history.open():
    try:row=json.loads(line)
    except json.JSONDecodeError:continue
    msg=row.get('message',{})
    if msg.get('role')=='toolResult' and msg.get('toolName')=='bash':
        text=''.join(x.get('text','') for x in msg.get('content',[]) if x.get('type')=='text')
        if re.search(r'Ran 43 tests in [0-9.]+s\n\nOK\n',text):unit=(row,text);break
assert unit,'actual host unit-test output not found'
(D/'runtime').mkdir(exist_ok=True)
(D/'runtime/host-tests-tool-result.json').write_text(json.dumps(unit[0],indent=2)+'\n')
(D/'runtime/host-tests.log').write_text(unit[1])
for line in history.open():
    try:row=json.loads(line)
    except json.JSONDecodeError:continue
    msg=row.get('message',{})
    if msg.get('role')=='toolResult' and msg.get('toolName')=='bash':
        text=''.join(x.get('text','') for x in msg.get('content',[]) if x.get('type')=='text')
        if re.search(r'Ran 3 tests in [0-9.]+s\n\nOK\n',text) and 'StartupObservationTests' in text:
            (D/'runtime/startup-correction-tests-tool-result.json').write_text(json.dumps(row,indent=2)+'\n')
            (D/'runtime/startup-correction-tests.log').write_text(text)
            break
else:raise AssertionError('actual startup-correction test output not found')
initial=json.loads((W/'runtime/execution/execution.json').read_text())
dumpdir=D/'runtime/initial-startup-observation-failure';dumpdir.mkdir(exist_ok=True)
shutil.copy2(W/'runtime/execution/execution.json',dumpdir/'execution.json')
for rec in initial['records']:
    shutil.copytree(Path(rec['work'])/'evidence',dumpdir/'evidence',dirs_exist_ok=True)
assumption=json.loads((W/'runtime/execution-corrected/execution.json').read_text())
assumptiondir=D/'runtime/buffered-cat-observer-assumption-failure';assumptiondir.mkdir(exist_ok=True)
shutil.copy2(W/'runtime/execution-corrected/execution.json',assumptiondir/'execution.json')
for rec in assumption['records']:
    shutil.copytree(Path(rec['work'])/'evidence',assumptiondir/'evidence',dirs_exist_ok=True)
xml_prior=json.loads((W/'runtime/execution-contract-correct/execution.json').read_text())['records'][6]
xml_dir=D/'runtime/qemu-xml-namespace-observer-failure';xml_dir.mkdir(exist_ok=True)
shutil.copytree(Path(xml_prior['work'])/'evidence',xml_dir/'evidence',dirs_exist_ok=True)
for line in history.open():
    row=json.loads(line);msg=row.get('message',{})
    if msg.get('role')=='toolResult' and msg.get('toolName')=='bash':
        text=''.join(x.get('text','') for x in msg.get('content',[]) if x.get('type')=='text')
        if 'Ran 2 tests in 0.002s\n\nOK\n' in text and 'test_captured_qemu_target_without_xi_namespace' in text:
            (D/'runtime/qemu-xml-correction-tests-tool-result.json').write_text(json.dumps(row,indent=2)+'\n');break
else:raise AssertionError('actual captured XML correction test output absent')
native_prior=json.loads((W/'runtime/execution-observed-corrected/execution.json').read_text())['records'][6]
echo_prior=json.loads((W/'runtime/execution-step-amended/execution.json').read_text())['records'][6]
echo_dir=D/'runtime/interleaved-echo-observer-failure';echo_dir.mkdir(exist_ok=True)
shutil.copytree(Path(echo_prior['work'])/'evidence',echo_dir/'evidence',dirs_exist_ok=True)
for count,needle,name in [(7,'test_native_step_only_current_breakpoint_restored_before_return','native-step'),(4,'test_exact_fragments_across_existing_service_line_retained','interleaved-echo')]:
    for line in history.open():
        row=json.loads(line);msg=row.get('message',{})
        if msg.get('role')=='toolResult' and msg.get('toolName')=='bash':
            text=''.join(x.get('text','') for x in msg.get('content',[]) if x.get('type')=='text')
            if re.search(rf'Ran {count} tests in [0-9.]+s\n\nOK\n',text) and needle in text:
                (D/f'runtime/{name}-tests-tool-result.json').write_text(json.dumps(row,indent=2)+'\n');break
    else:raise AssertionError(f'actual {name} test output absent')
static=json.loads((W/'static/measurement.json').read_text())
signed=json.loads((W/'signing-evidence-verified/summary.json').read_text());scope=json.loads((W/'signing-evidence-verified/scope-comparison.json').read_text())
assert signed['status']=='COMPLETE_SIX_VALID_SCOPED_DEV_SIGNATURES'
b2signed=sum(x['signed']['bytes'] for x in signed['artifacts']);b2unsigned=sum(x['unsigned']['bytes'] for x in signed['artifacts'])
assert b2signed==980528
b1={Path(x['path']).name:x for x in static['b1']['six_elfs']}
b2={'scoped_signing_status':signed['status'],'six_signed_bytes':b2signed,'six_original_unsigned_bytes':b2unsigned,'suite_signed_bytes':signed['artifacts'][0]['signed']['bytes'],'worker_signed_bytes':sum(x['signed']['bytes'] for x in signed['artifacts'][1:]),'artifacts':signed['artifacts'],'byte_identical_to_b1':{x['name']:x['signed']['sha256']==b1[x['name']]['sha256'] for x in signed['artifacts']},'scope_comparison':scope,'non_pristine':True,'lock_audit':'lock-audit.json','whole_workspace_F1_refusal_retained':True}
(D/'b2-final-measurement.json').write_text(json.dumps(b2,indent=2)+'\n')
execution=json.loads((W/'runtime/execution-echo-corrected/execution.json').read_text());records=execution['records'];summary=[]
for rec in records:
    work=Path(rec['work']);out=D/'runtime'/f'{rec["profile"]}-{rec["mode"]}-{rec["number"]}';out.mkdir(exist_ok=True)
    for p in (work/'evidence').rglob('*'):
        if p.is_file():
            dst=out/p.relative_to(work/'evidence');dst.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dst)
    short={k:rec.get(k) for k in ['profile','mode','number','status','error','observer_error','cleanup_error','work']}
    if rec.get('timestamps'):
        ts=rec['timestamps'];t0=rec['qemu_start_monotonic_ns'];short['fresh_VM_to_exact_prompt_ms']=(ts['exact_prompt_ns']-t0)/1e6
        if rec['profile']=='desktop':
            short['fresh_VM_to_desktop_ready_ms']=(ts['desktop_ready_ns']-t0)/1e6;short['click_to_exact_Terminal_prompt_ms']=(ts['exact_prompt_ns']-ts['terminal_click_ns'])/1e6
    for key in ['idle','top_idle']:
        if key in rec:
            x=rec[key];short[key]={'samples':len(x['samples']),'elapsed_ms':(x['end_ns']-x['start_ns'])/1e6,'content_change_frames':sum(bool(s.get('changed_pixels')) for s in x['samples']) if rec['profile']=='desktop' else None,'qemu_cpu_seconds':x['qemu_cpu_ticks_delta']/x['host_ticks_per_second'],'serial_bytes':x['serial_bytes'],'frame_scope':'GUI sampled content differences, not draw calls' if rec['profile']=='desktop' else 'UART-only; no framebuffer change metric'}
    if 'long_stream' in rec:short['long_stream']=rec['long_stream']
    if (work/'evidence/observation.json').exists():short['observation']=json.loads((work/'evidence/observation.json').read_text())
    short['qualified_complete_observed_workload']=bool(rec['status']=='COMPLETE' and rec['mode']=='observed' and short.get('observation',{}).get('instances') and short.get('observation',{}).get('heap_post_event_count'))
    summary.append(short)
for name in ['rsp.py','test_rsp.py','common.py','observer.py','run.py','test_measurement.py','resume-observed.py']:shutil.copy2(W/'runtime'/name,D/'runtime'/name)
shutil.copy2(W/'runtime/execution-echo-corrected/schedule.json',D/'runtime/schedule.json');shutil.copy2(W/'runtime/execution-echo-corrected/execution.json',D/'runtime/execution.json');shutil.copy2(Path(__file__),D/'final-report.py')
statistics_by_profile={}
for profile in ['headless','desktop']:
    values=[r['fresh_VM_to_exact_prompt_ms'] for r in summary if r['profile']==profile and r['mode']=='untraced' and 'fresh_VM_to_exact_prompt_ms' in r]
    statistics_by_profile[profile]={'count':len(values),'values_ms':values,'min_ms':min(values) if values else None,'median_ms':statistics.median(values) if values else None,'max_ms':max(values) if values else None}
proof_path=W/'canonical/comparison-after-amended-observation.json'
proof=json.loads(proof_path.read_text()) if proof_path.exists() else None
if proof:shutil.copytree(W/'canonical',D/'canonical',dirs_exist_ok=True)
result={'stage':'S10-4','status':'MEASUREMENTS_COMPLETE_CP2_REQUESTED' if len(records)==8 and all(r['status']=='COMPLETE' for r in records) else 'STOP_RUNTIME_MEASUREMENT','candidate_sha256':'4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821','approval':'user2026-10-28 D4-Obs1/2/3 A + D4-Sign1 A','host_tests':{'count':43,'pass':True,'source_tool_result_id':unit[0]['id'],'log':'runtime/host-tests.log'},'B1':{'signed_six_bytes':980528,'runtime_baseline':None},'B2':{'signed_six_bytes':b2signed,'original_unsigned_six_bytes':b2unsigned,'details':'b2-final-measurement.json','scoped_signing':True,'non_pristine':True},'static_profiles':{p:{k:v[k] for k in ['suite_only_signed_bytes','workers_signed_bytes','six_elf_set_signed_bytes','seven_packaged_elf_bytes_including_alias','physical_alias_copy_bytes','all_file_payload_bytes','image','kernel','iso']} for p,v in static['profiles'].items()},'runtime':summary,'untraced_startup_statistics':statistics_by_profile,'remaining_unrun':execution['remaining_unrun'],'final_preservation':proof,'feature_equivalent':False,'historical_runtime_comparator':None,'exact_RSP_stack_peak':None,'kernel_stack_peak':None,'RSS':None,'CBX27_met':False,'phase10_criterion4_met':False,'canonical_promotion':False,'containment_lift':False,'limitations':'Fresh-VM TCG only. Peak means aligned live allocator counter with complete hardware coverage; stack metric only CPU write-attempt page bounds; backing not RSS. Observer timings separate. No matched baseline runtime; no universal optimization.'}
result['cleanup']=json.loads((W/'cleanup.json').read_text())
result['startup_statistics_scope']='Completed startup predicates only; later workload status is retained separately and not converted to a complete run.'
result['startup_harness_deviation']={'initial_attempt':initial,'correction':'Evaluate every received genuine prompt prefix with existing Shell.idle(count1); receive timestamp contains boundary; preserve failed900s attempt with normal prompt/product boot. No ignored-line/acceptance or product change.','new_host_behavioral_tests':3}
result['buffered_cat_harness_deviation']={'failed_attempt':assumption,'classification':'observer assumption error, not demonstrated product defect','existing_contract':'S10-3b.8 Collected sinks buffer until complete and return130/no bytes on interrupt; executor1875-1895/1919-1922, typed915-931/959-1001; qualified L0 explicitly0rows before cat CtrlC, separate no-cat streaming live check.','disposition':'Main explicitly authorized contract-correct10s zero-output cat cancellation plus separate no-cat live stream measurement; preserve120s failed observation, no product/acceptance change.'}
result['qemu_xml_harness_deviation']={'failed_attempt':xml_prior,'classification':'host parser compatibility bug before guest resume; QEMU target.xml xi:include without xmlns:xi','correction':'Bind only known xi prefix on otherwise unbound target root; same entity/include/depth/path/register/packet policies. Two captured XML/unknown-prefix/bad-path regression tests pass; original bytes retained; completed timing lanes not rerun.'}
result['native_step_amendment']={'prior_unsupported_attempt':native_prior,'approval':'user2026-10-28 exact one native instruction with current hardware execute breakpoint removed/restored before continue; all other breakpoints/write guards remain, no guest writes/Z0/injection','diagnosis':'runtime/hardware-resume-observer-failure/diagnosis.json','host_tests':7,'timing_reruns':0}
result['interleaved_echo_harness_deviation']={'failed_attempt':echo_prior,'classification':'exact command echo split by existing approved service log; guest completed actual two frames/new prompt','correction':'Reassemble exact command prefix fragments only across original SERVICE_LINE/KERNEL_LINE, preserve background records, same new prompt/deadline; arbitrary/truncated text still fails','host_tests':4}
result['total_distinct_observer_host_tests']=43+3+2+7+4
result['desktop_component_statistics']={}
for metric in ['fresh_VM_to_desktop_ready_ms','click_to_exact_Terminal_prompt_ms']:
    values=[r[metric] for r in summary if r['profile']=='desktop' and r['mode']=='untraced' and metric in r]
    result['desktop_component_statistics'][metric]={'count':len(values),'values_ms':values,'min_ms':min(values) if values else None,'median_ms':statistics.median(values) if values else None,'max_ms':max(values) if values else None}
result['native_step_completion_investigation']=json.loads((D/'runtime/native-step-no-progress-investigation/diagnosis.json').read_text())
result['observed_scope']='Headless partial interval only; complete observed workload failed native-step no-progress guard, desktop observed unrun. Never infer workload-wide peak, zero memory, matched baseline, exact stack/RSS/kernel-stack or performance from observed runs.'
result['known_unmet']=['CBX27','Phase10 criterion4','matched historical runtime baseline','complete headless observed workload peak','desktop observed metrics','exact user stack/RSS/kernel-stack peak']
proposal=(R/'.agents/261020-1750-cellbox-cli/phase-10-s0-proposal.md').read_text()
approved='#### S10-4.4 '+proposal.split('#### S10-4.4 ',1)[1].split('#### S10-4.5 ',1)[0]
(D/'runtime/cp1-approved.txt').write_text(approved)
result['approved_cp1_snapshot_sha256']=hashlib.sha256(approved.encode()).hexdigest()
(D/'summary.json').write_text(json.dumps(result,indent=2)+'\n');(W/'execution-summary.json').write_text(json.dumps(result,indent=2)+'\n')
checks={str(p.relative_to(D)):sha(p) for p in D.rglob('*') if p.is_file() and p.name!='evidence.sha256'}
(D/'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k,v in sorted(checks.items())))
print(json.dumps({'status':result['status'],'runtime_runs':len(records),'untraced_startup_statistics':statistics_by_profile,'remaining_unrun':result['remaining_unrun'],'preservation':proof},indent=2))
