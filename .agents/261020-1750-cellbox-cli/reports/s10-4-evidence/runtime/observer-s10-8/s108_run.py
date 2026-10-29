"""S10-8.3 measurement runners (ONE run per set; RUN_CONSUMED-s108-<set> claimed with O_EXCL before any VM exists).

Entry points (see the two `run_*.py` files):

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_headless.py --preflight | --run
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_desktop.py  --preflight | --run

`--preflight` only evaluates the go/no-go; `--run` re-evaluates it, claims the token, runs ONCE (no retry, no rerun logic exists),
and archives everything under reports/s10-8-evidence/measurement-runs/<set>/.  Guest memory is never written; no new guest
control, input or command is introduced: the schedule, the measurement kernels/ISOs/disks and the QEMU argv are the retained
S10-7.4 ones (`s107_machine.MeasurementMachine` is imported unchanged).  The only differences from S10-7.4 are (1) the fixed
`MemoryMixin` (`_wiring_stop`), (2) the pause-credit deadlines of the two original 10 s recovery waits (s108_credit) and
(3) the new tokens/evidence root.
"""
import hashlib
import json
import sys
import time
import traceback
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s108_env as ENV  # noqa: E402
import s107_env as E7  # noqa: E402
import s108_preflight as PF  # noqa: E402
import s107_retained as RET  # noqa: E402
import s108_memory as MEM  # noqa: E402
import s107_machine as MACH  # noqa: E402
import s108_surface as SURF  # noqa: E402
import s108_credit as CREDIT  # noqa: E402
import s107_run as RUN7  # noqa: E402  (classify_run / summarize_memory / archive helpers reused unchanged)
import run_hook as RH  # noqa: E402  (retained hook_facts; importing defines functions only)

X, HO, HS, A, R, C = RET.X, RET.HO, RET.HS, RET.A, RET.R, RET.C

CANDIDATE_SNAPSHOTS = RUN7.CANDIDATE_SNAPSHOTS
classify_run = RUN7.classify_run
summarize_memory = RUN7.summarize_memory
facts_from_table = RUN7.facts_from_table
chain = RUN7.chain
trace_argv = RUN7.trace_argv
memory_context = RUN7.memory_context
archive = RUN7.archive
write_manifest = RUN7.write_manifest


# ------------------------------------------------------------------ observer / surface classes
class MemoryHookObserver(MEM.MemoryMixin, HO.HookObserver):
    SKIP_SNAPSHOT_NAMES = (HO.DISARM,)


class MemoryPlainObserver(MEM.MemoryMixin, RET.OBS.Observer):
    SKIP_SNAPSHOT_NAMES = ()


CreditedDesktopSurface, CreditedHeadlessSurface = SURF.surface_classes(HS.HookSurface)

PAUSE_CREDIT_RULE = {
    'decision': 'D8-1=B (approved 2026-10-29)',
    'applies_to': ['headless: Surface.wait(label=new interrupt prompt, 10 s)', 'desktop: Desk.wait_for(what=runtime new GUI prompt, 10 s)'],
    'effective_deadline': 'min(start + 10 s + pause_in_wait, start + phase_max, machine_start + 900 s)',
    'pause_accounting': 'Observer.pause_ns difference (retained: sum of completed _stopped-handling intervals)',
    'phase_max_s': CREDIT.PHASE_MAX_S,
    'phase_max_interpretation': 'no per-phase table exists in the retained schedule; the largest single workload wait (120 s command/stream-flow) bounds a recovery wait',
    'global_window_s': CREDIT.GLOBAL_WINDOW_S,
    'unchanged': ['120 s finite/command wait', '60 s status wait', '180/240/300/900 s boot and drawing waits', 'every pause(N) observation duration'],
    'ceiling': 'not a product timing claim; measures recovery in 10 s of unpaused guest time',
}


# ------------------------------------------------------------------ run scaffolding
def run_desktop(spec, health, binding, destination, work):
    machine = observer = surface = control = gate = None
    result = {'set': spec.name, 'profile': 'desktop', 'status': 'STARTING', 'candidate_runs': 0, 'mode': 'S10-8-desktop-memory-armed-pause-credit',
              'start_ns': time.monotonic_ns(), 'original_window_seconds': E7.DEADLINES['total_window_s'], 'command_deadlines_unchanged': True,
              'deadlines': E7.DEADLINES, 'pause_credit_rule': PAUSE_CREDIT_RULE}
    try:
        machine = MACH.MeasurementMachine(spec, work / 'vm', observed=True)
        trace_argv(machine)
        result['candidate_runs'] = 1
        static = HO.load_static()
        machine.start()
        machine.qmp.sock.settimeout(5)
        gate = C.InputGate(machine, machine.work / 'evidence/gui-input-journal.jsonl', source_verified=binding['verified'])
        gate.install()
        X.dump(machine.work / 'evidence/qmp-lock-installation.json',
               {'actual_qmp_object_id': id(machine.qmp), 'shared_lock_id': id(gate.lock), 'lock': 'threading.RLock',
                'source_binding': binding, 'input_budget_seconds': 5, 'max_input_attempts_per_batch': 64})
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv, vm_start_ns=machine.start_ns)
        control = C.S.SerializedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = MemoryHookObserver(machine, gate, static, machine.work / 'evidence/feed-hook-ledger.jsonl', memory=memory_context(spec))
        observer.client = RET.F.AtomicClient(observer, control, gate)
        gate.health = observer.check
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = CreditedDesktopSurface(machine, observer, gate)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        result['streams'] = surface.streams()
        result['top_idle'] = surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:  # noqa: BLE001 - every exit is classified, never retried
        result.update(status='STOP', error=f'{type(error).__name__}: {error}', traceback=traceback.format_exc())
        for item in chain(error):
            if isinstance(item, HS.HookOracleFailure):
                result['oracle_failure'] = {'cls': item.cls, 'code': item.code, 'reason': item.reason}
                break
        if surface and surface.d:
            try:
                surface.d.retain_failure(error, {'runner': 'S10-8 desktop stopped before cleanup'})
            except Exception as retention_error:  # noqa: BLE001
                result['retention_error'] = str(retention_error)
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps, pause_credits=surface.__dict__.get('pause_credits', []))
            if surface.d:
                result.update(gui_temporal_epochs=surface.d.temporal_epochs, gui_failures=surface.d.failure_records)
        if observer:
            try:
                observer.shutdown()
            except Exception as error:  # noqa: BLE001
                result['observer_error'] = f'{type(error).__name__}: {error}'
                result['status'] = 'STOP'
        if gate:
            gate.terminate(result.get('error', 'owned desktop attempt ended'))
            result.update(gui_batches=gate.batches, gate_terminal_reason=gate.terminal_reason, natural_resume_epoch=gate.resume_epoch)
            X.dump(machine.work / 'evidence/gui-input-batches.json', gate.batches)
        if machine:
            try:
                machine.stop()
            except Exception as error:  # noqa: BLE001
                result.update(status='STOP', cleanup_error=f'{type(error).__name__}: {error}')
        facts = RH.hook_facts(observer, surface.d if surface else None) if observer else {}
        result.update(feed_hook=facts, end_ns=time.monotonic_ns(), workload_complete=result['status'] == 'COMPLETE',
                      decisions=observer.client.decisions if observer else [],
                      native_scope_count=len(control.steps) if control else 0,
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0,
                      observer_pause_ns_total=getattr(observer, 'pause_ns', None))
        X.dump(work / 'runtime-result.json', result)
        archive(work, destination, machine)
    return result


def run_headless(spec, health, destination, work):
    """Candidate headless (retained S10-4.11 recurring harness), same machinery as S10-7.4 with the credited surface."""
    machine = observer = surface = control = None
    result = {'set': spec.name, 'profile': 'headless', 'status': 'STARTING', 'candidate_runs': 0, 'mode': 'S10-8-headless-memory-armed-pause-credit',
              'start_ns': time.monotonic_ns(), 'original_window_seconds': E7.DEADLINES['total_window_s'],
              'command_deadlines_unchanged': True, 'deadlines': E7.DEADLINES, 'pause_credit_rule': PAUSE_CREDIT_RULE}
    try:
        machine = MACH.MeasurementMachine(spec, work / 'vm', observed=True, with_data_disk=True)
        trace_argv(machine)
        result['candidate_runs'] = 1
        machine.start()
        machine.qmp.sock.settimeout(5)
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv)
        control = A.AmendedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = MemoryPlainObserver(machine, memory=memory_context(spec))
        observer.client = R.RecurringClient(observer, control)
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = CreditedHeadlessSurface(machine, observer)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        result['streams'] = surface.streams()
        result['top_idle'] = surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:  # noqa: BLE001
        result.update(status='STOP', error=f'{type(error).__name__}: {error}', traceback=traceback.format_exc())
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps, pause_credits=surface.__dict__.get('pause_credits', []))
        if observer:
            try:
                observer.shutdown()
            except Exception as error:  # noqa: BLE001
                result.update(status='STOP', observer_error=f'{type(error).__name__}: {error}')
        if machine:
            try:
                machine.stop()
            except Exception as error:  # noqa: BLE001
                result.update(status='STOP', cleanup_error=f'{type(error).__name__}: {error}')
        result.update(end_ns=time.monotonic_ns(), workload_complete=result['status'] == 'COMPLETE',
                      decisions=observer.client.decisions if observer else [],
                      native_scope_count=len(control.steps) if control else 0,
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0,
                      observer_pause_ns_total=getattr(observer, 'pause_ns', None))
        X.dump(work / 'runtime-result.json', result)
        archive(work, destination, machine)
    return result


def census_file(path, paths, state):
    present, missing = state
    ENV.dump(path, {'paths': len(paths), 'method': 'protected census: {path: {size, sha256}} (same as common.protected_paths)',
                    'present': present, 'missing': missing})


def finish(spec, result, destination, before):
    census_after = PF.census_now(before['census_paths'])
    census_file(destination / 'census-after.json', before['census_paths'], census_after)
    comparison = {'census_unchanged': census_after == before['census'], 'census_paths': len(before['census_paths']),
                  'census_before_file': 'census-before.json', 'census_after_file': 'census-after.json',
                  'frozen_sources': PF.check_frozen_sources(), 'frozen_s107_sources': PF.check_frozen_s107(), 'prior_archives': PF.check_prior_archives()}
    drift = sorted(p for p, h in json.loads(PF.NEW_FROZEN.read_text()).items() if not Path(p).is_file() or ENV.sha256_file(p) != h) \
        if PF.NEW_FROZEN.exists() else ['frozen-sources-s10-8.sha256 missing']
    comparison['frozen_s108_sources'] = {'ok': not drift, 'detail': {'drift': drift}}
    comparison['kernel_set_hashes'] = E7.verify_set_files(spec.name)
    table = summarize_memory(destination / 'evidence')
    expected, allocator = CANDIDATE_SNAPSHOTS, True
    facts = facts_from_table(table, None)
    timestamps = result.get('timestamps') or {}
    ready = bool(timestamps.get('exact_prompt_ns') or timestamps.get('desktop_ready_ns'))
    entries = (destination / 'evidence/memory-summary.json').exists() or any(destination.glob('evidence/memory-entry-*.json'))
    classification = classify_run(status=result['status'], expected_snapshots=expected, taken=facts['taken'], entries_ok=entries, ready=ready,
                                  kind=spec.kind, snapshot_ok=facts['ok'], snapshot_exact=facts['exact'], allocator_measured=allocator)
    summary = {'set': spec.name, 'classification': classification, 'status': result['status'], 'error': result.get('error'),
               'observer_error': result.get('observer_error'), 'cleanup_error': result.get('cleanup_error'),
               'kernel_provenance': E7.provenance(spec.name), 'memory_table': table,
               'pause_credit_rule': PAUSE_CREDIT_RULE, 'pause_credits': result.get('pause_credits', []),
               'observer_pause_ns_total': result.get('observer_pause_ns_total'),
               'preservation': comparison, 'candidate_sha256': E7.CAND_SHA, 'retained_redirects': RET.REDIRECTS,
               'rerun_authorized': False, 'cbx27_met': False, 'phase10_criterion4_met': False,
               'b2_headless': ENV.NOT_RUN_SETS['b2-headless'], 'b2_desktop': ENV.NOT_RUN_SETS['b2-desktop']}
    X.dump(destination / 'summary.json', summary)
    write_manifest(destination)
    return summary


def main(set_name, argv):
    spec = ENV.SETS[set_name]
    if '--preflight' in argv:
        outcome, _health, _binding = PF.preflight((set_name,))
        (HERE / f'go-no-go-preflight-{set_name}.json').write_text(json.dumps(outcome, indent=2) + '\n')
        print(json.dumps({'set': set_name, 'go': outcome['go_by_set'][set_name],
                          'failed': {k: v['detail'] for k, v in list(outcome['checks'].items()) + list(outcome['sets'][set_name].items()) if not v['ok']}},
                         indent=1)[:4000])
        return 0 if outcome['go_by_set'][set_name] else 2
    if '--run' not in argv:
        print(__doc__)
        return 64
    outcome, health, binding = PF.preflight((set_name,))
    (HERE / f'go-no-go-run-{set_name}.json').write_text(json.dumps(outcome, indent=2) + '\n')
    if not outcome['go_by_set'][set_name]:
        print(json.dumps({'NO_GO': set_name}))
        return 2
    prior_paths = list(json.loads(PF.PRIOR_CENSUS.read_text()))
    before = {'census_paths': prior_paths, 'census': PF.census_now(prior_paths)}
    ENV.claim_run(set_name)           # permanent, BEFORE any VM exists
    stamp = hashlib.sha256(str(time.time_ns()).encode()).hexdigest()[:6]
    work = spec.dir / f'run-s108-{stamp}'
    work.mkdir(mode=0o700)
    destination = ENV.RUN_EVIDENCE / set_name
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    census_file(destination / 'census-before.json', prior_paths, before['census'])
    result = None
    try:
        if spec.name == 'cand-desktop':
            result = run_desktop(spec, health, binding, destination, work)
        else:
            result = run_headless(spec, health, destination, work)
    finally:
        summary = finish(spec, result or {'status': 'NOT_STARTED'}, destination, before)
        print(json.dumps({'set': set_name, 'status': summary['status'], 'class': summary['classification'], 'error': summary['error'],
                          'preservation': {k: summary['preservation'][k] for k in ('census_unchanged',)}}, indent=2), flush=True)
    return 0
