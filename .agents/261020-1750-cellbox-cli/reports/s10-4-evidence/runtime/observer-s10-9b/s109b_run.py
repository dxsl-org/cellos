"""S10-9b B2 desktop measurement runner (ONE run; RUN_CONSUMED-s109-b2-desktop claimed with O_EXCL before any VM exists).

Entry point (see `run_b2_desktop.py`):

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_b2_desktop.py --preflight | --run

`--preflight` only evaluates the go/no-go; `--run` re-evaluates it, claims the token, runs ONCE (no retry, no rerun logic exists, whatever
the result) and archives everything under reports/s10-9-evidence/b2-desktop-run/measurement-run/.  Guest memory is never written; the only
guest-facing actions are the retained GUI machinery's QMP input (taskbar click, typed keys) and screendumps, plus the retained
read-only RSP `p`/`m` packets and hardware `Z1` breakpoints.  The QEMU argv is the retained desktop `D.Machine` argv of S10-7.4/S10-8/S10-9
(q35, TCG, 256M, one vCPU, restricted user netdev without forwards, ISO cdrom, virtio keyboard + tablet, -display none, private serial/QMP/gdb
unix sockets) with the NVMe data disk dropped (the B2 shared subset needs none; same rule as the b2-headless run) and the ISO read from a
private copy.
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
import s109b_env as ENV  # noqa: E402
import s109b_preflight as PF  # noqa: E402
import s109b_b2d as B2D  # noqa: E402
import s107_retained as RET  # noqa: E402
import s107_memory as MEM  # noqa: E402
import s108_run as RUN8  # noqa: E402  (classify/summarize/archive/census helpers: unchanged S10-7/S10-8 code)

X, HS, C = RET.X, RET.HS, RET.C

classify_run = RUN8.classify_run
summarize_memory = RUN8.summarize_memory
facts_from_table = RUN8.facts_from_table
chain = RUN8.chain
trace_argv = RUN8.trace_argv
archive = RUN8.archive
write_manifest = RUN8.write_manifest
census_file = RUN8.census_file

COMPARATOR_NOTES = {
    'gui_terminal': 'the B2 GUI terminal interprets a few builtins in-process with fixed outputs (no shell, no spawn, no VFS access, no exit status)',
    'allocator_peak': 'not measured: no Cellbox heapwatch/allocator guard hooks in the B2 terminal/shell',
    'status': 'not applicable (no exit status in the B2 GUI terminal)',
    'recovery_10s_criterion': 'not applicable (no cancellable command, no recovery wait engaged)',
    'ceiling': 'QEMU/TCG guest numbers on a hooks measurement kernel; non-pristine historical B2 desktop image (services rebuilt from source at 3471028bc, '
               'a few tracked historical bytes kept); only terminal and shell are memory-measured',
}


def memory_context(spec):
    """MemoryContext for the B2 desktop: no single heap arena (each task carries its own, resolved per target by the observer)."""
    return MEM.MemoryContext(provenance=ENV.provenance(spec), kernel_path=spec.kernel, heap_arena_rel=None,
                             user_pages=spec.user_stack_pages, kernel_pages=spec.user_stack_pages)


def run_b2_desktop(spec, health, binding, destination, work):
    machine = observer = surface = control = gate = None
    schedule = ENV.schedule()
    result = {'set': spec.name, 'profile': 'desktop', 'status': 'STARTING', 'b2_runs': 0, 'mode': 'S10-9b-b2-desktop-shared-subset-memory-armed',
              'start_ns': time.monotonic_ns(), 'original_window_seconds': ENV.DEADLINES['total_window_s'], 'command_deadlines_unchanged': True,
              'deadlines': ENV.DEADLINES, 'comparator_notes': COMPARATOR_NOTES, 'schedule': str(ENV.SCHEDULE),
              'rsp_client': 'run_atomic.AtomicClient (AtomicReceiveMixin over CoordinatedClient)'}
    try:
        machine = B2D.B2DesktopMachine(spec, work / 'vm', observed=True)
        trace_argv(machine)
        result['b2_runs'] = 1
        targets = B2D.B2DesktopTargets()
        context = memory_context(spec)
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
        observer = B2D.B2DesktopObserver(machine, gate, targets, context)
        observer.client = RET.F.AtomicClient(observer, control, gate)
        gate.health = observer.check
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = B2D.B2DesktopSurface(machine, observer, gate, schedule)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        result['shared_subset'] = surface.shared_subset()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:  # noqa: BLE001 - every exit is classified, never retried
        result.update(status='STOP', error=f'{type(error).__name__}: {error}', traceback=traceback.format_exc())
        if surface and surface.d:
            try:
                surface.d.retain_failure(error, {'runner': 'S10-9b B2 desktop stopped before cleanup'})
            except Exception as retention_error:  # noqa: BLE001
                result['retention_error'] = str(retention_error)
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps, command_results=surface.command_results)
            if surface.d:
                result.update(gui_temporal_epochs=surface.d.temporal_epochs, gui_failures=surface.d.failure_records,
                              oracle_checks=surface.d.checks, oracle_notes=surface.d.notes)
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
        result.update(end_ns=time.monotonic_ns(), workload_complete=result['status'] == 'COMPLETE',
                      decisions=observer.client.decisions if observer else [],
                      native_scope_count=len(control.steps) if control else 0,
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0,
                      observer_pause_ns_total=getattr(observer, 'pause_ns', None))
        X.dump(work / 'runtime-result.json', result)
        archive(work, destination, machine)
    return result


# ------------------------------------------------------------------ reporting
def memory_report(evidence_dir):
    """Region / stack / heap-arena table of every memory-NN-*.json record (B2 shape: `records` list, one per observed task)."""
    rows = []
    for path in sorted(Path(evidence_dir).glob('memory-[0-9][0-9]-*.json')):
        document = json.loads(path.read_text())
        for record in document.get('records', []):
            resident = record.get('resident') or {}
            rows.append({
                'file': path.name, 'phase': record.get('phase'), 'task': (record.get('spawn') or {}).get('name'), 'ordinal': (record.get('spawn') or {}).get('ordinal'),
                'ok': record.get('ok'), 'all_stack_depths_exact': record.get('all_stack_depths_exact'), 'stack_depth_bytes': record.get('stack_depth_bytes'),
                'regions': [{'label': r.get('label'), 'kind': r.get('kind'), 'size_bytes': r.get('size_bytes'), 'mapped_backed_bytes': r.get('mapped_backed_bytes'),
                             'fully_mapped': r.get('fully_mapped')} for r in resident.get('regions', [])],
                'resident_total': resident.get('total'),
                'stacks': [{'label': s.get('label'), 'usable_bytes': s.get('usable_bytes'), 'depth_bytes': s.get('depth_bytes'), 'exact': s.get('exact'),
                            'status': s.get('status'), 'prime_evidence': s.get('prime_evidence'), 'depth_valid': s.get('depth_valid'),
                            'untouched_prefix_bytes': s.get('untouched_prefix_bytes')} for s in record.get('stacks', [])],
                'heap_arena': record.get('heap_arena'), 'errors': record.get('errors')})
        for item in document.get('unobserved', []):
            rows.append({'file': path.name, 'phase': document.get('phase'), 'task': item.get('name'), 'unobserved': item})
    return rows


def idle_facts(result):
    idle = result.get('ordinary_idle') or {}
    samples = idle.get('samples') or []
    return {'samples': len(samples), 'samples_expected': 100, 'frames_with_changed_pixels': sum(1 for s in samples if s.get('changed_pixels')),
            'qemu_cpu_ticks_delta': idle.get('qemu_cpu_ticks_delta'), 'serial_bytes': idle.get('serial_bytes'),
            'start_ns': idle.get('start_ns'), 'end_ns': idle.get('end_ns')}


def phase_facts(result, schedule):
    seen = [e['phase'] for e in result.get('surface_events') or [] if 'phase' in e]
    wanted = [p['name'] for p in schedule['phases']]
    commands = [{'phase': c['phase'], 'command': c['command'], 'exact_screen_match': c['exact_screen_match'], 'seconds': c['seconds'],
                 'model_rows_after': c['model_rows_after']} for c in result.get('command_results') or []]
    return {'phases_entered': seen, 'phases_expected': wanted, 'phases_not_entered': [name for name in wanted if name not in seen],
            'commands': commands}


def finish(spec, result, destination, before):
    census_after = PF.census_now(before['census_paths'])
    census_file(destination / 'census-after.json', before['census_paths'], census_after)
    comparison = {'census_unchanged': census_after == before['census'], 'census_paths': len(before['census_paths']),
                  'census_before_file': 'census-before.json', 'census_after_file': 'census-after.json',
                  'frozen_sources': PF.check_frozen_sources(), 'frozen_s107_sources': PF.check_frozen_s107(), 'frozen_s108_sources': PF.check_frozen_s108(),
                  'frozen_s109_sources': PF.check_frozen_s109(), 'prior_archives': PF.check_prior_archives(),
                  'qemu_pids_after_run': PF.qemu_pids()}
    drift = sorted(p for p, h in json.loads(PF.NEW_FROZEN.read_text()).items() if not Path(p).is_file() or ENV.sha256_file(p) != h) \
        if PF.NEW_FROZEN.exists() else ['frozen-sources-s10-9b.sha256 missing']
    comparison['frozen_s109b_sources'] = {'ok': not drift, 'detail': {'drift': drift}}
    hashes_after = ENV.verify_set_files(spec)
    ENV.dump(destination / 'hashes-after.json', hashes_after)
    comparison['kernel_iso_qcow2_hashes'] = {'file': 'hashes-after.json', 'all_match': hashes_after['all_match'], 'match': hashes_after['match']}
    schedule = ENV.schedule()
    table = summarize_memory(destination / 'evidence')
    expected = ENV.schedule_snapshots(schedule)
    facts = facts_from_table(table, B2D.FOCUS)
    timestamps = result.get('timestamps') or {}
    ready = bool(timestamps.get('exact_prompt_ns') or timestamps.get('desktop_ready_ns'))
    entries = any(destination.glob('evidence/memory-entry-*.json'))
    classification = classify_run(status=result['status'], expected_snapshots=expected, taken=facts['taken'], entries_ok=entries, ready=ready,
                                  kind=spec.kind, snapshot_ok=facts['ok'], snapshot_exact=facts['exact'], allocator_measured=False)
    summary = {'set': spec.name, 'classification': classification, 'status': result['status'], 'error': result.get('error'),
               'observer_error': result.get('observer_error'), 'cleanup_error': result.get('cleanup_error'),
               'expected_class': 'B (every phase and snapshot complete, allocator peak not measurable in B2)',
               'kernel_provenance': ENV.provenance(spec), 'argv': result.get('argv'),
               'focus_task': B2D.FOCUS, 'memory_table': table, 'memory_report': memory_report(destination / 'evidence'),
               'phases': phase_facts(result, schedule), 'idle': idle_facts(result), 'comparator_notes': COMPARATOR_NOTES,
               'observer_pause_ns_total': result.get('observer_pause_ns_total'), 'runner_seconds': (result.get('end_ns', 0) - result.get('start_ns', 0)) / 1e9 if result.get('end_ns') else None,
               'preservation': comparison, 'candidate_sha256': ENV.E7.CAND_SHA, 'retained_redirects': RET.REDIRECTS,
               'rerun_authorized': False, 'cbx27_met': False, 'phase10_criterion4_met': False,
               'b2_headless': 'class B COMPLETE in S10-7.4, not rerun'}
    X.dump(destination / 'summary.json', summary)
    write_manifest(destination)
    return summary


def write_top_level(summary, destination):
    """Top-level copies next to the harness dir: summary.json and an evidence.sha256 over everything under b2-desktop-run/."""
    root = ENV.EVIDENCE
    X.dump(root / 'summary.json', summary)
    entries = {str(p.relative_to(root)): ENV.sha256_file(p) for p in root.rglob('*')
               if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts}
    (root / 'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k, v in sorted(entries.items())))


def main(argv):
    spec = ENV.SPEC
    if '--preflight' in argv:
        outcome, _health, _binding = PF.preflight()
        (HERE / 'go-no-go-preflight-b2-desktop.json').write_text(json.dumps(outcome, indent=2) + '\n')
        print(json.dumps({'set': spec.name, 'go': outcome['go_by_set'][spec.name],
                          'failed': {k: v['detail'] for k, v in list(outcome['checks'].items()) + list(outcome['sets'][spec.name].items()) if not v['ok']}},
                         indent=1)[:4000])
        return 0 if outcome['go_by_set'][spec.name] else 2
    if '--run' not in argv:
        print(__doc__)
        return 64
    outcome, health, binding = PF.preflight()
    (HERE / 'go-no-go-run-b2-desktop.json').write_text(json.dumps(outcome, indent=2) + '\n')
    if not outcome['go_by_set'][spec.name]:
        print(json.dumps({'NO_GO': spec.name}))
        return 2
    prior_paths = list(json.loads(PF.PRIOR_CENSUS.read_text()))
    before = {'census_paths': prior_paths, 'census': PF.census_now(prior_paths)}
    ENV.claim_run()                    # permanent, BEFORE any VM exists
    stamp = hashlib.sha256(str(time.time_ns()).encode()).hexdigest()[:6]
    work = ENV.WORK_ROOT / f'run-b2-desktop-{stamp}'
    work.mkdir(mode=0o700, parents=True)
    destination = ENV.RUN_EVIDENCE
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    census_file(destination / 'census-before.json', prior_paths, before['census'])
    ENV.dump(destination / 'hashes-before.json', ENV.verify_set_files(spec))
    ENV.dump(destination / 'qemu-pids-before.json', {'qemu_pids': PF.qemu_pids(), 'rule': 'no other VM may run (also a go/no-go gate)'})
    result = None
    try:
        result = run_b2_desktop(spec, health, binding, destination, work)
    finally:
        summary = finish(spec, result or {'status': 'NOT_STARTED'}, destination, before)
        write_top_level(summary, destination)
        print(json.dumps({'set': spec.name, 'status': summary['status'], 'class': summary['classification'], 'error': summary['error'],
                          'preservation': {k: summary['preservation'][k] for k in ('census_unchanged',)}}, indent=2), flush=True)
    return 0
