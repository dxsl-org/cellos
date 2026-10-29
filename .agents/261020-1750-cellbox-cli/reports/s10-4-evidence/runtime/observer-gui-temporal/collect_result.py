#!/usr/bin/env python3
"""Retained-only S10-4.15 interpretation; no VM/QMP/input operations."""
import importlib.util
import json
from pathlib import Path

import gui_temporal as G

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('retained_atomic_collector', G.PREVIOUS / 'collect_result.py')
OLD = importlib.util.module_from_spec(spec)
spec.loader.exec_module(OLD)
OLD.ROOT = OLD.P.ROOT = OLD.P.P.ROOT = OLD.P.Q.ROOT = ROOT


def temporal_audit(runtime):
    epochs = runtime.get('gui_temporal_epochs', [])
    batches = {batch['id']: batch for batch in runtime.get('gui_batches', [])}
    audited = []
    finite_complete = status_complete = False
    for epoch in epochs:
        ledger = epoch.get('ledger') or {}
        commit = ledger.get('commit') or epoch.get('Enter_commit')
        samples = epoch.get('samples', [])
        batch = batches.get(commit['id']) if commit else None
        observed_ns = [sample['monotonic_ns'] for sample in samples]
        monotonic = all(a < b for a, b in zip(observed_ns, observed_ns[1:]))
        commit_matches = bool(commit and batch == commit and batch.get('outcome') == 'COMMITTED' and batch.get('commit_count') == 1)
        after_commit = bool(commit and samples and all(stamp > commit['finish'] * 1e9 for stamp in observed_ns))
        frames = ledger.get('frames', [])
        records = ledger.get('records', [])
        image_paths = [ROOT / 'desktop/evidence' / f'gui-epoch-{epoch["id"]:02d}' / f'{index:04d}.png'
                       for index in range(len(samples))]
        sample_files_present = bool(samples) and all(path.exists() for path in image_paths)
        complete = (epoch.get('state') == 'COMPLETE' and ledger.get('complete') is True and
                    commit_matches and after_commit and monotonic and sample_files_present)
        if epoch['command'] == G.FINITE:
            finite_complete = complete and len(frames) == 2
        elif epoch['command'] == G.STATUS:
            status_complete = complete and finite_complete and ledger.get('kind') == 'status'
        audited.append({'id': epoch['id'], 'name': epoch['name'], 'command': epoch['command'],
                        'state': epoch.get('state'), 'error': epoch.get('error'),
                        'samples': len(samples), 'all_sample_images_retained': sample_files_present,
                        'exact_reads': sum(sample.get('exact_read') is not None for sample in samples),
                        'single_original_Enter_commit_matches_journal': commit_matches,
                        'strictly_ordered_after_Enter': after_commit and monotonic,
                        'record_count': len(records), 'frame_states': frames,
                        'actual_begin_records': sum(record.get('frame') == 'begin' for record in records),
                        'actual_complete_end_records': sum(record.get('frame') == 'end' and record.get('complete') is True for record in records),
                        'ledger_complete': ledger.get('complete', False), 'ledger_reasons': ledger.get('reasons'),
                        'last_sample': samples[-1] if samples else None,
                        'proof_complete': complete,
                        'scope': 'Observed exact GUI output only; no inference from requested frames/status or native activity'})
    failures = []
    for index, failure in enumerate(runtime.get('gui_failures', []), 1):
        image = ROOT / 'desktop/evidence' / f'gui-failure-{index:02d}.png'
        raw = ROOT / 'desktop/evidence' / f'gui-failure-{index:02d}.json'
        failures.append({'index': index, 'error': failure.get('error'), 'context': failure.get('context'),
                         'last_frame_ns': failure.get('last_frame_ns'), 'retained_ns': failure.get('retained_ns'),
                         'raw_read': failure.get('last_exact_read'),
                         'epoch_id': (failure.get('epoch') or {}).get('id'),
                         'PNG_retained': image.exists(), 'raw_JSON_retained': raw.exists(),
                         'image_sha256': G.X.sha(image) if image.exists() else None,
                         'no_extra_guest_action': failure.get('scope')})
    return {'exercised': bool(epochs), 'epochs': audited, 'all_path_failure_retention': failures,
            'finite_two_frames_and_fresh_prompt_proved': finite_complete,
            'own_original_status_zero_and_newprompt_proved': status_complete,
            'finite_complete_contract': finite_complete and status_complete,
            'full_original_schedule_complete': runtime.get('workload_complete', False),
            'historical_S10_4_14_completion_reconstructed': False,
            'sampling_interval_seconds_unchanged': 1.5,
            'scope': 'Only complete uniquely covered records count. Sparse/coalesced missing rows remain observability-incomplete, not product failure.'}


def main():
    summary = json.loads((ROOT / 'summary.json').read_text())
    runtime = summary['desktop']
    desktop = OLD.P.P.profile_report('desktop', summary['preservation'])
    gui = OLD.P.gui_report(runtime)
    temporal = temporal_audit(runtime)
    report = {'status': summary['status'], 'desktop': desktop, 'GUI_input_admission': gui,
              'QMP_transaction_audit': OLD.P.Q.qmp_audit(), 'RSP_atomic_receive_audit': OLD.wire_audit(),
              'temporal_GUI_output_proof': temporal,
              'preservation': summary['preservation'],
              'verification': json.loads((ROOT / 'verification-summary.json').read_text()),
              'offline_renderer_proof': json.loads((ROOT / 'rendered-temporal-proof.json').read_text()),
              'headless_runs': 0, 'cbx27_met': False, 'phase10_criterion4_met': False,
              'historical_runtime_baseline': None, 'resident_bytes': None,
              'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None,
              'CP2_gate': 'Accept actual exercised temporal observation/timeout retention and exact workload/coverage or partial STOP only. No rerun, missing-output inference, baseline/RSS/exactstack or promotion/containment closure.'}
    G.X.dump(ROOT / 'interpretation.json', report)
    G.F.R.manifest(ROOT / 'desktop')
    G.F.R.manifest(ROOT)
    print(json.dumps({'status': report['status'], 'desktop': {key: desktop[key] for key in
        ('status', 'error', 'observer_error', 'cleanup_error', 'workload_complete', 'complete_event_coverage_for_approved_schedule',
         'native_step_packets', 'scope_records', 'original_same_pc_failures', 'additional_native_packets',
         'event_outcomes', 'instances', 'draw_entry_counts', 'checks')},
        'GUI_input_admission': {key: gui[key] for key in gui if key != 'batch_summaries'},
        'temporal_GUI_output_proof': {key: value for key,value in temporal.items() if key != 'epochs'},
        'temporal_epoch_summaries': [{key:value for key,value in epoch.items() if key not in ('last_sample','frame_states')} for epoch in temporal['epochs']],
        'RSP_atomic_receive_audit': report['RSP_atomic_receive_audit'],
        'QMP_transaction_audit': report['QMP_transaction_audit'], 'preservation': report['preservation']}, indent=2))


if __name__ == '__main__':
    main()
