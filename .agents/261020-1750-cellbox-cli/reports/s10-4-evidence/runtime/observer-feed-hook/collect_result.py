#!/usr/bin/env python3
"""Retained-only interpretation of the single S10-4.17 desktop run; no VM/QMP/RSP/guest operation."""
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_observer as HO  # noqa: E402
import run_hook as RH  # noqa: E402

X = HO.X
D = HERE / 'desktop'
E = D / 'evidence'


def main():
    summary = json.loads((HERE / 'summary.json').read_text())
    runtime = json.loads((D / 'runtime-result.json').read_text())
    facts = runtime['feed_hook']
    epochs = json.loads((E / 'gui-temporal-epochs.json').read_text())
    steps = [json.loads(line) for line in (E / 'steps.jsonl').read_text().splitlines()]
    hook_pc = facts['identity_gate']['hook_pc']
    hook_steps = [s for s in steps if s.get('addr') == hook_pc]
    ledger_rows = [json.loads(line) for line in (E / 'feed-hook-ledger.jsonl').read_text().splitlines()]
    traffic = [json.loads(line) for line in (E / 'rsp-traffic.jsonl').read_text().splitlines()]
    observation = json.loads((E / 'observation.json').read_text())
    epoch_audit = []
    for epoch in epochs:
        samples = epoch['samples']
        commit = epoch['Enter_commit']
        epoch_audit.append({
            'kind': epoch['kind'], 'state': epoch['state'], 'samples': len(samples),
            'all_samples_non_gating': all(s['gating'] is False for s in samples),
            'accepted_sample': next((s['index'] for s in samples if s['accepted']), None),
            'screen_match_samples': sum(bool(s.get('screen_match')) for s in samples),
            'enter_batch': commit['id'], 'enter_commit_count': commit['commit_count'],
            'typed_exact_before_enter': epoch['typed_grab_ns'] < commit['attempts'][-1]['start'] * 1e9,
            'armed_to_complete_seconds': (epoch['end_ns'] - epoch['armed_ns']) / 1e9,
            'original_deadline_seconds': epoch['timeout_seconds'],
            'last_hit_ns': epoch.get('oracle_final', {}).get('last_hit_ns')})
    tail_error = runtime['error']
    interpretation = {
        'scope': 'retained-only reading of the one S10-4.17 desktop result; no rerun, no fix, no VM operation',
        'classification': summary['classification']['class'], 'classification_name': summary['classification']['name'],
        'run': {'desktop_runs': summary['desktop_runs'], 'headless_runs': summary['headless_runs'],
                'rerun_authorized': summary['rerun_authorized'], 'status': runtime['status'],
                'wall_seconds': (runtime['end_ns'] - runtime['start_ns']) / 1e9,
                'original_window_seconds': 900, 'command_deadlines_unchanged': runtime['command_deadlines_unchanged']},
        'metric': {
            'proved': facts['metric_complete'],
            'finite': facts['metric']['finite']['oracle'] | {'commit_finish_ns': facts['metric']['finite']['commit_finish_ns']},
            'status': facts['metric']['status']['oracle'],
            'meaning': ('Frontend-consumed Terminal bytes (ByteSink::feed ledger) proved: two complete JSON frames, '
                        'fresh prompt, then the separate original echo st=$? epoch = st=0 and a new prompt; each epoch '
                        'cross-checked against the exact final GUI screen (two identical reads 1.5 s apart).')},
        'ledger': {'hits': facts['hits'], 'bytes': facts['bytes'], 'stdout_bytes': facts['stdout_bytes'],
                   'stderr_bytes': facts['stderr_bytes'], 'sha256_of_concatenated_bytes': facts['ledger_sha256'],
                   'violation': facts['violation'], 'jsonl_records': len(ledger_rows),
                   'jsonl_all_accepted': all(r['accepted'] for r in ledger_rows),
                   'jsonl_mode': oct((E / 'feed-hook-ledger.jsonl').stat().st_mode & 0o777),
                   'sites_seen': sorted({hex(r['ret_off']) for r in ledger_rows})},
        'identity_gate': facts['identity_gate'],
        'three_way_count': {'ledger_hits': facts['disarm']['ledger_hits'], 'hook_hits': facts['disarm']['hook_hits'],
                            'successful_scopes_at_hook': facts['disarm']['scopes']['successful_scopes'],
                            'with_hook_tb_entry': facts['disarm']['scopes']['successful_with_hook_tb_entry'],
                            'scopes_recorded_in_steps_jsonl': len(hook_steps),
                            'failed_primary_scopes': facts['disarm']['scopes']['failed_primary_scopes'],
                            'all_equal': len({facts['disarm']['ledger_hits'], facts['disarm']['hook_hits'],
                                              facts['disarm']['scopes']['successful_scopes'],
                                              facts['disarm']['scopes']['successful_with_hook_tb_entry'],
                                              len(hook_steps)}) == 1},
        'disarm': facts['disarm'],
        'epochs': epoch_audit,
        'tail_stop': {
            'error_head': tail_error[:60], 'observed_phase_sequence': [e['phase'] for e in runtime['surface_events'] if 'phase' in e],
            'classification_of_tail_stop': summary['classification'].get('tail_stop_class'),
            'cause': ('retained original-schedule check Surface.status(): `need(len(vals)==1)` over the WHOLE visible screen '
                      'after the collected-cat Ctrl-C status. The screen legitimately shows both the earlier finite '
                      '`st=0` row and the new `st=130` row, so the check raises "bad status rows". It is a latent defect '
                      'of the retained desktop schedule (S10-4.4 common.py), unreachable until the finite epoch could '
                      'complete; it is not a candidate failure and not a hook failure. Not fixed or rerun.'),
            'later_original_phases_not_run': ['endless-terminal-stream', 'rg-large', 'interactive-top', 'complete snapshot']},
        'transport_and_recovery': {
            'native_scopes': runtime['native_scope_count'], 'hook_scopes': len(hook_steps),
            'additional_native_packets': runtime['additional_native_packets'],
            'recoveries': [(d['outcome'], hex(d['site'])) for d in runtime['decisions']],
            'recoveries_all_advanced': all(d['outcome'] == 'ADDITIONAL_ADVANCED_CONTINUE' for d in runtime['decisions']),
            'recoveries_at_hook_pc': sum(d['site'] == hook_pc for d in runtime['decisions']),
            'gui_batches': len(runtime['gui_batches']),
            'gui_batches_all_committed_once': all(b['commit_count'] == 1 and b['outcome'] == 'COMMITTED' for b in runtime['gui_batches']),
            'rsp_packet_tx': sum(e['event'] == 'packet_tx' for e in traffic),
            'rsp_packet_rx': sum(e['event'] == 'packet_rx' for e in traffic),
            'observer_pause_seconds': observation['observer_pause_ns'] / 1e9},
        'memory_partial_bounds_only': {
            'scope': 'Aligned allocator events, CPU stack write-attempt 4 KiB bounds and mapped-backing snapshots for the phases actually run; NOT full workload, NOT RSS/exact/kernel stack',
            'instances': [{'role': i['role'], 'peak_aligned_used_bytes': i['peak_aligned_used_bytes'], 'last_value': i['last_value'],
                           'post_events': i['post_events'], 'phase_peaks': i['phase_peaks'],
                           'stack_write_attempt_depth_bound_bytes': i['stack_write_attempt_depth_bound_bytes']}
                          for i in observation['instances']],
            'snapshots': observation['snapshots'],
            'resident_bytes': None, 'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None,
            'historical_runtime_baseline': None},
        'preservation': summary['preservation'],
        'acceptance': {'cbx27_met': False, 'phase10_criterion4_met': False,
                       'reason': 'full workload not completed; RSS/exact stack/kernel stack/historical comparator unavailable'},
        'residual_limits': [
            'Entry-only hook: completion of the last feed is established by the exact final-screen cross-check, not a return hook.',
            'feed exposes no Cell/session/operation id; attribution rests on the single Terminal instance, epoch cut and payload self-identification.',
            'Terminal notice() text bypasses feed; none was visible (no [terminal/[shell row).',
            'A structure-preserving loss/duplication/reorder that is scrolled off the 26-row screen would not be detected (documented in T5).',
            'The hook was disarmed after the status epoch; Terminal output after that is unobserved by design.'],
    }
    path = HERE / 'interpretation.json'
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    os.write(fd, (json.dumps(interpretation, indent=2) + '\n').encode())
    os.close(fd)
    print(json.dumps({k: interpretation[k] for k in ('classification', 'classification_name')} |
                     {'three_way_all_equal': interpretation['three_way_count']['all_equal']}))


if __name__ == '__main__':
    main()
