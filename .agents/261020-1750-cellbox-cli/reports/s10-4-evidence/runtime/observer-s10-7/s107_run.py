"""S10-7.4 measurement runners (ONE run per set; RUN_CONSUMED-<set> claimed with O_EXCL before any VM exists).

Entry points (see the three `run_*.py` files):

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_desktop.py  --preflight | --run
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_headless.py --preflight | --run
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_b2_headless.py   --preflight | --run

`--preflight` only evaluates the go/no-go; `--run` re-evaluates it, claims the token, runs ONCE (no retry, no rerun logic exists),
and archives everything under reports/s10-7-evidence/measurement-runs/<set>/.  Guest memory is never written; no new guest
control, input or command is introduced beyond the retained schedule (candidate) or schedule-b2.json (B2).
"""
import hashlib
import json
import shutil
import sys
import time
import traceback
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402
import s107_preflight as PF  # noqa: E402
import s107_retained as RET  # noqa: E402
import s107_memory as MEM  # noqa: E402
import s107_machine as MACH  # noqa: E402
import s107_b2 as B2  # noqa: E402
import run_hook as RH  # noqa: E402  (retained hook_facts; importing defines functions only)

X, HO, HS, A, R, C = RET.X, RET.HO, RET.HS, RET.A, RET.R, RET.C
M = RET.M

# Snapshot names the retained schedule takes (surface.ready, sample_idle, streams, top_idle, complete), in order.
CANDIDATE_SNAPSHOTS = ('initial-prompt', 'ordinary-prompt-idle', 'finite-stream', 'collected-cat-buffered', 'collected-cat-cancelled',
                       'terminal-stream-live', 'terminal-stream-cancelled', 'rg-cancelled', 'interactive-top-idle', 'complete')


def b2_schedule(path=HERE / 'schedule-b2.json'):
    return json.loads(Path(path).read_text())


def b2_snapshots(schedule):
    return tuple(p['name'] for p in schedule['phases'] if p.get('snapshot'))


# ------------------------------------------------------------------ observer classes
class MemoryHookObserver(MEM.MemoryMixin, HO.HookObserver):
    SKIP_SNAPSHOT_NAMES = (HO.DISARM,)


class MemoryPlainObserver(MEM.MemoryMixin, RET.OBS.Observer):
    SKIP_SNAPSHOT_NAMES = ()


MeasuredDesktopSurface, MeasuredHeadlessSurface = MACH.surface_classes(HS.HookSurface)


class B2Surface(MeasuredHeadlessSurface):
    """Shared-subset schedule from schedule-b2.json on the retained headless Surface (ready/idle/command/status unchanged)."""

    def shared_subset(self, schedule):
        results = []
        for phase in schedule['phases']:
            if phase['kind'] != 'command':
                continue
            self.phase(phase['name'])
            rows = self.command(phase['name'], phase['command'], ENV.DEADLINES['command_s'])
            status = self.status()
            text = '\n'.join(rows)
            entry = {'phase': phase['name'], 'command': phase['command'], 'status': status, 'expected_status': phase['expect_status'],
                     'status_ok': status == phase['expect_status'], 'row_expected': phase.get('expect_row'),
                     'row_seen': (phase['expect_row'] in text) if phase.get('expect_row') else None, 'output_rows': rows[:200],
                     'output_rows_total': len(rows)}
            results.append(entry)
            if phase.get('snapshot'):
                self.snapshot(phase['name'])
        return results


# ------------------------------------------------------------------ classification
def classify_run(*, status, expected_snapshots, taken, entries_ok, ready, kind, snapshot_ok, snapshot_exact, allocator_measured):
    """Pre-declared harness classes (A = all phases + all memory elements non-null and exact).

    A all phases complete, every snapshot ok and exact, allocator peak measured, comparator-independent (candidate sets)
    B all phases complete, every snapshot ok, but some exactness flag false or an element not measurable (e.g. B2 allocator)
    C workload stopped before `complete` after >=1 memory snapshot (partial phases)
    D boot reached the prompt but the memory wiring produced no snapshot (entry/range discovery failure)
    E boot/ready never reached (panic, no prompt)
    """
    missing = [name for name in expected_snapshots if name not in taken]
    if not ready:
        return {'class': 'E', 'reason': 'ready/prompt never reached', 'missing_snapshots': missing}
    if not taken:
        return {'class': 'D', 'reason': 'no memory snapshot was taken' + ('' if entries_ok else ' (entry-stop range discovery failed)'),
                'missing_snapshots': missing}
    if status != 'COMPLETE' or missing:
        return {'class': 'C', 'reason': 'workload did not complete every snapshot phase', 'missing_snapshots': missing}
    if snapshot_ok and snapshot_exact and allocator_measured:
        return {'class': 'A', 'reason': 'all snapshots ok and exact; allocator peak measured', 'missing_snapshots': []}
    why = []
    if not snapshot_ok:
        why.append('a snapshot record has ok=false')
    if not snapshot_exact:
        why.append('not every stack depth is pre-run-proven exact')
    if not allocator_measured:
        why.append('allocator peak not measured')
    return {'class': 'B', 'reason': '; '.join(why), 'missing_snapshots': []}


def summarize_memory(evidence_dir):
    """Per-phase table from the memory-NN-*.json records written by the observer (candidate and B2 shapes)."""
    table, root = [], Path(evidence_dir)
    for path in sorted(root.glob('memory-[0-9][0-9]-*.json')):
        document = json.loads(path.read_text())
        records = document.get('records')
        if records is None:
            records = [document]
        for record in records:
            resident = record.get('resident') or {}
            total = resident.get('total') or {}
            label = (record.get('spawn') or {}).get('name') or record.get('instance')
            table.append({'file': path.name, 'phase': record.get('phase'), 'task': label, 'ok': record.get('ok'),
                          'exact': record.get('all_stack_depths_exact'), 'stack_depth_bytes': record.get('stack_depth_bytes'),
                          'distinct_frame_bytes': total.get('distinct_frame_bytes'),
                          'distinct_frame_bytes_excluding_heap_arena': total.get('distinct_frame_bytes_excluding_heap_arena'),
                          'errors': record.get('errors')})
    return table


def facts_from_table(table, task_filter=None):
    rows = [r for r in table if task_filter is None or r['task'] == task_filter]
    return {'taken': sorted({r['phase'] for r in rows}), 'ok': bool(rows) and all(r['ok'] for r in rows),
            'exact': bool(rows) and all(r['exact'] for r in rows)}


# ------------------------------------------------------------------ run scaffolding
def chain(error):
    seen = []
    while error is not None and error not in seen:
        seen.append(error)
        error = error.__cause__ or error.__context__
    return seen


def trace_argv(machine):
    machine.argv += ['-D', str(machine.work / 'evidence/qemu-trace.log'), '-msg', 'timestamp=on']
    for event in X.BASE_EVENTS:
        machine.argv += ['-trace', f'enable={event}']
    path = machine.work / 'evidence/input-provenance.json'
    provenance = json.loads(path.read_text())
    provenance.update(argv=machine.argv)
    X.dump(path, provenance)


def memory_context(spec):
    provenance = ENV.provenance(spec.name)
    if spec.kind == 'candidate':
        _record, files = ENV.candidate_files(spec.profile)
        arena = MEM.heap_arena_symbol(files['/bin/cellbox']['file'], kind='cellbox')
    else:
        arena = None
    return MEM.MemoryContext(provenance=provenance, kernel_path=spec.kernel, heap_arena_rel=arena,
                             user_pages=spec.user_stack_pages, kernel_pages=spec.user_stack_pages)


def archive(work, destination, machine):
    for path in work.iterdir():
        if path.is_file():
            shutil.copy2(path, destination / path.name)
    if machine and (machine.work / 'evidence').exists():
        shutil.copytree(machine.work / 'evidence', destination / 'evidence', dirs_exist_ok=False)
    if machine and (machine.work / 'disk.img').exists():
        (machine.work / 'disk.img').unlink()


def write_manifest(root):
    entries = {str(p.relative_to(root)): ENV.sha256_file(p) for p in root.rglob('*')
               if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts}
    (root / 'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k, v in sorted(entries.items())))


def run_desktop(spec, health, binding, destination, work):
    machine = observer = surface = control = gate = None
    result = {'set': spec.name, 'profile': 'desktop', 'status': 'STARTING', 'candidate_runs': 0, 'mode': 'S10-7.4-desktop-memory-armed',
              'start_ns': time.monotonic_ns(), 'original_window_seconds': ENV.DEADLINES['total_window_s'], 'command_deadlines_unchanged': True,
              'deadlines': ENV.DEADLINES}
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
        surface = MeasuredDesktopSurface(machine, observer, gate)
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
                surface.d.retain_failure(error, {'runner': 'S10-7.4 desktop stopped before cleanup'})
            except Exception as retention_error:  # noqa: BLE001
                result['retention_error'] = str(retention_error)
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps)
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
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0)
        X.dump(work / 'runtime-result.json', result)
        archive(work, destination, machine)
    return result


def run_headless(spec, health, destination, work):
    """Candidate headless (retained S10-4.11 recurring harness) or B2 headless (shared subset), same machinery."""
    machine = observer = surface = control = None
    b2 = spec.kind == 'b2'
    schedule = b2_schedule() if b2 else None
    result = {'set': spec.name, 'profile': 'headless', 'status': 'STARTING', 'candidate_runs': 0,
              'mode': 'S10-7.4-b2-headless-shared-subset' if b2 else 'S10-7.4-headless-memory-armed',
              'start_ns': time.monotonic_ns(), 'original_window_seconds': ENV.DEADLINES['total_window_s'],
              'command_deadlines_unchanged': True, 'deadlines': ENV.DEADLINES}
    try:
        machine = MACH.MeasurementMachine(spec, work / 'vm', observed=True, with_data_disk=not b2)
        trace_argv(machine)
        result['candidate_runs'] = 1
        machine.start()
        machine.qmp.sock.settimeout(5)
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv)
        control = A.AmendedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        context = memory_context(spec)
        if b2:
            observer = B2.B2MemoryObserver(machine, B2.B2Targets(), context)
        else:
            observer = MemoryPlainObserver(machine, memory=context)
        observer.client = R.RecurringClient(observer, control)
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = (B2Surface if b2 else MeasuredHeadlessSurface)(machine, observer)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        if b2:
            result['shared_subset'] = surface.shared_subset(schedule)
        else:
            result['streams'] = surface.streams()
            result['top_idle'] = surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:  # noqa: BLE001
        result.update(status='STOP', error=f'{type(error).__name__}: {error}', traceback=traceback.format_exc())
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps)
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
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0)
        X.dump(work / 'runtime-result.json', result)
        archive(work, destination, machine)
    return result


def finish(spec, result, destination, before):
    census_after = PF.census_now(before['census_paths'])
    comparison = {'census_unchanged': census_after == before['census'], 'census_paths': len(before['census_paths']),
                  'frozen_sources': PF.check_frozen_sources(), 'prior_archives': PF.check_prior_archives()}
    table = summarize_memory(destination / 'evidence')
    if spec.kind == 'b2':
        expected, task, allocator = b2_snapshots(b2_schedule()), 'shell', False
    else:
        expected, task, allocator = CANDIDATE_SNAPSHOTS, None, True
    facts = facts_from_table(table, task)
    timestamps = result.get('timestamps') or {}
    ready = bool(timestamps.get('exact_prompt_ns') or timestamps.get('desktop_ready_ns'))
    entries = (destination / 'evidence/memory-summary.json').exists() or any(destination.glob('evidence/memory-entry-*.json'))
    classification = classify_run(status=result['status'], expected_snapshots=expected, taken=facts['taken'], entries_ok=entries, ready=ready,
                                  kind=spec.kind, snapshot_ok=facts['ok'], snapshot_exact=facts['exact'], allocator_measured=allocator)
    summary = {'set': spec.name, 'classification': classification, 'status': result['status'], 'error': result.get('error'),
               'observer_error': result.get('observer_error'), 'cleanup_error': result.get('cleanup_error'),
               'kernel_provenance': ENV.provenance(spec.name), 'memory_table': table, 'preservation': comparison,
               'candidate_sha256': ENV.CAND_SHA, 'retained_redirects': RET.REDIRECTS, 'rerun_authorized': False,
               'cbx27_met': False, 'phase10_criterion4_met': False}
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
    work = spec.dir / f'run-{stamp}'
    work.mkdir(mode=0o700)
    destination = ENV.RUN_EVIDENCE / set_name
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    result = None
    try:
        if spec.name == 'cand-desktop':
            result = run_desktop(spec, health, binding, destination, work)
        else:
            result = run_headless(spec, health, destination, work)
    finally:
        summary = finish(spec, result or {'status': 'NOT_STARTED'}, destination, before)
        print(json.dumps({'set': set_name, 'status': summary['status'], 'class': summary['classification'],
                          'preservation': {k: summary['preservation'][k] for k in ('census_unchanged',)}}, indent=2), flush=True)
    return 0
