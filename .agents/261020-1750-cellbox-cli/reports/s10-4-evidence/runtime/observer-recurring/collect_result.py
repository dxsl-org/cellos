#!/usr/bin/env python3
"""Offline S10-4.11 evidence/metric report; never starts or controls a guest."""
from collections import Counter
import json
from pathlib import Path

import recurring as R

ROOT = Path(__file__).resolve().parent


def records(path):
    if path.exists():
        with path.open() as source:
            for line in source:
                yield json.loads(line)


def profile_report(profile, preservation):
    root = ROOT / profile
    evidence = root / 'evidence'
    runtime = json.loads((root / 'runtime-result.json').read_text())
    observation_path = evidence / 'observation.json'
    observed = json.loads(observation_path.read_text()) if observation_path.exists() else {}
    steps = list(records(evidence / 'steps.jsonl'))
    by_ordinal = {step['ordinal']: step for step in steps}
    commands = [(row['monotonic_ns'], bytes.fromhex(row['payload_hex']).decode('ascii'))
                for row in records(evidence / 'rsp-traffic.jsonl') if row['event'] == 'packet_tx']
    forbidden = [(stamp, command) for stamp, command in commands
                 if command.startswith(('M', 'X', 'G', 'P', 'Z0', 'z0', 'qRcmd', 'Qqemu.sstep'))]
    native_count = sum(command == 's' for _, command in commands)
    additional = [step for step in steps if step.get('kind') == 'single-approved-additional-step']
    issued = [step for step in additional if step.get('additional_native_packet_issued')]
    census_equal = all(step.get('breakpoints_before') == step.get('breakpoints_after') and
                       step.get('watches_before') == step.get('watches_after') for step in steps)
    decisions = runtime['decisions']
    terminal = [decision for decision in decisions if not decision['terminal_latch_cleared']]
    controls_after_terminal = [(stamp, command) for stamp, command in commands
                              if terminal and stamp > terminal[-1]['end_ns'] and command in ('s', 'c')]
    event_reports = []
    proof_checks = []
    with (evidence / 'qemu-trace.log').open('rb') as trace_file:
        for decision in decisions:
            first = by_ordinal[decision['first_ordinal']]
            extra = [step for step in additional if step['first_ordinal'] == first['ordinal']]
            event = {'event_id': decision['event_id'], 'phase': first['phase'],
                     'site': hex(first['addr']), 'first_ordinal': first['ordinal'],
                     'outcome': decision['outcome'], 'extra_steps_used': decision['extra_steps_used'],
                     'terminal_latch_cleared': decision['terminal_latch_cleared'],
                     'extra_scope_ordinals': [step['ordinal'] for step in extra]}
            if 'proof' in decision:
                scope = first['trace']
                trace_file.seek(scope['offset'])
                raw = trace_file.read(scope['end_offset'] - scope['offset'])
                health = decision['proof']['retained_health']
                # Recompute the unchanged pure proof from retained bytes, independently of success.
                proof = R.A.admit(first, first['after'], first['qmp_after'],
                    first['breakpoints_after'], first['watches_after'], raw, health, True,
                    decision['lifecycle_before'], decision['lifecycle_after'])
                proof_checks.append(proof['admitted'])
                event.update(zero_retirement_proof_recomputed=True,
                             raw_sha256=scope['raw_sha256'], empty_watch_expected=proof['empty_watch_expected'])
            else:
                event['zero_retirement_proof_recomputed'] = None
            if extra and 'after' in extra[-1] and 'before' in extra[-1]:
                step = extra[-1]
                event.update(additional_before_rip=hex(step['before']['rip']),
                             additional_after_rip=hex(step['after']['rip']),
                             additional_rip_delta=step['after']['rip'] - step['before']['rip'],
                             additional_returned_stop=step.get('returned_stop'),
                             additional_error=step.get('error'), capture_error=step.get('capture_error'))
            event_reports.append(event)
    heap_counts = Counter()
    heap_maxima = {}
    for event in records(evidence / 'heap-events.jsonl'):
        instance = event['instance']
        heap_counts[instance] += 1
        heap_maxima[instance] = max(heap_maxima.get(instance, 0), event['aligned_used_bytes'])
    coverage_counts = Counter(event['instance'] for event in records(evidence / 'coverage-events.jsonl'))
    all_counters_match = all(heap_counts[inst['id']] == inst['post_events'] and
                             coverage_counts[inst['id']] == inst['coverage_writes'] and
                             (not inst['post_events'] or heap_maxima[inst['id']] == inst['peak_aligned_used_bytes'])
                             for inst in observed.get('instances', []))
    coverage_complete = (runtime['workload_complete'] and observed.get('status') == 'COMPLETE' and
                         not observed.get('error') and census_equal and all_counters_match and
                         not terminal and all(inst.get('pending_writer') is None and inst['initial_counter'] == 0
                                              for inst in observed.get('instances', [])))
    instances = []
    for inst in observed.get('instances', []):
        instances.append({'id': inst['id'], 'role': inst['role'], 'base': hex(inst['base']),
            'post_events': inst['post_events'], 'coverage_writes': inst['coverage_writes'],
            'maximum_retained_post_event_aligned_bytes': heap_maxima.get(inst['id']),
            'complete_approved_workload_aligned_allocator_peak_bytes':
                inst['peak_aligned_used_bytes'] if coverage_complete else None,
            'peak_scope': 'complete approved observed schedule' if coverage_complete else
                          'partial retained post-event interval; not a true-workload peak',
            'phase_event_maxima_bytes': inst['phase_peaks'], 'last_observed_value': inst['last_value'],
            'pending_writer': inst.get('pending_writer'),
            'stack_usable_reservation_bytes': inst['stack_top'] - inst['stack_bottom'],
            'stack_CPU_write_attempt_pages': len(inst['touched_pages']),
            'stack_CPU_write_attempt_depth_bound_bytes': inst['stack_write_attempt_depth_bound_bytes'],
            'stack_bound_scope': 'CPU write-attempt4KiB page bounds over retained interval; NOT exact byte/RSP/kernel-stack peak',
            'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None, 'resident_bytes': None})
    checks = {'one_fresh_profile_attempt': runtime['candidate_runs'] == 1,
              'original_deadline_and_command_limits': runtime['original_window_seconds'] == 900 and
                                                       runtime['command_deadlines_unchanged'],
              'no_guest_write_Z0_injection_or_mode_setter': not forbidden,
              'all_recorded_step_execute_watch_censuses_equal': census_equal,
              'one_extra_max_per_independent_event': all(decision['extra_steps_used'] <= 1 for decision in decisions) and
                  all(sum(step.get('additional_native_packet_issued', False) for step in additional
                          if step['first_ordinal'] == decision['first_ordinal']) <= 1 for decision in decisions),
              'additional_packet_count_matches': len(issued) == runtime['additional_native_packets'] ==
                                                  sum(decision['extra_steps_used'] for decision in decisions),
              'all_admitted_flag3_proofs_recomputed': all(proof_checks),
              'no_control_after_terminal_event': not controls_after_terminal,
              'raw_counter_and_coverage_totals_match': all_counters_match,
              'protected_and_previous_preserved': preservation['unchanged'] and
                  preservation['previous_archives_unchanged'] and preservation['old_compressed_trace_unchanged'] and
                  not preservation['host_source_changed']}
    report = {'profile': profile, 'status': runtime['status'], 'error': runtime.get('error'),
              'observer_error': runtime.get('observer_error'), 'cleanup_error': runtime.get('cleanup_error'),
              'workload_complete': runtime['workload_complete'],
              'complete_event_coverage_for_approved_schedule': coverage_complete,
              'host_interval_seconds_including_setup_cleanup': (runtime['end_ns'] - runtime['start_ns']) / 1e9,
              'native_step_packets': native_count, 'scope_records': len(steps),
              'original_same_pc_failures': sum(R.X.NO_PROGRESS in step.get('error', '') and
                  step.get('kind') != 'single-approved-additional-step' for step in steps),
              'additional_native_packets': len(issued), 'recurrence_events': event_reports,
              'event_outcomes': dict(Counter(decision['outcome'] for decision in decisions)),
              'checks': checks, 'forbidden_packets': forbidden, 'controls_after_terminal': controls_after_terminal,
              'surface_events': runtime.get('surface_events', []), 'streams': runtime.get('streams'),
              'top_idle': runtime.get('top_idle'), 'instances': instances,
              'mapped_backing_snapshots': observed.get('snapshots', []),
              'draw_entry_counts': dict(Counter(event['kind'] for event in observed.get('draw_events', []))),
              'observer_pause_ns_completed_stop_intervals': observed.get('observer_pause_ns'),
              'resident_bytes': None, 'historical_runtime_baseline': None,
              'limits': ['Observed timings include all-stop observer overhead; not startup/performance results',
                         'A clean observer-thread shutdown does not imply the workload schedule completed',
                         'Event maxima in an incomplete schedule are not full-workload peaks',
                         'Stack write-attempt4KiB bounds are not exact stack or RSP high-water marks',
                         'Mapped ELF-plus-user-stack backing is not RSS, residency, quota or whole-cell memory',
                         'Draw/flush entry counts are attempts/calls, not redraw cost or framebuffer changes',
                         'Hidden exit-request origin and historical failure causality remain unproven']}
    report['evidence_sha256'] = {str(path.relative_to(root)): R.X.sha(path) for path in
        [root / 'runtime-result.json', evidence / 'observation.json', evidence / 'steps.jsonl',
         evidence / 'rsp-traffic.jsonl', evidence / 'qmp-queries.jsonl', evidence / 'qemu-trace.log',
         evidence / 'heap-events.jsonl', evidence / 'coverage-events.jsonl'] if path.exists()}
    R.X.dump(root / 'interpretation.json', report)
    return report


def main():
    summary = json.loads((ROOT / 'summary.json').read_text())
    reports = [profile_report(profile, summary['preservation']) for profile in ('headless', 'desktop')]
    report = {'status': summary['status'], 'profiles': reports, 'preservation': summary['preservation'],
              'host_tests': {'command': ['python3', '-m', 'unittest', '-v', 'test_recurring'],
                             'result': json.loads((ROOT / 'host-tests.json').read_text())['returncode']},
              'cbx27_met': False, 'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
              'CP2_gate': 'Accept actual observed results/bounds and deviations only; no automatic equivalent-feature baseline, criterion4, canonical promotion or containment closure.'}
    R.X.dump(ROOT / 'interpretation.json', report)
    for profile in ('headless', 'desktop'):
        R.manifest(ROOT / profile)
    R.manifest(ROOT)
    print(json.dumps({'status': report['status'], 'profiles': [{key: profile[key] for key in
        ('profile', 'status', 'error', 'observer_error', 'workload_complete', 'complete_event_coverage_for_approved_schedule',
         'native_step_packets', 'original_same_pc_failures', 'additional_native_packets', 'event_outcomes',
         'instances', 'draw_entry_counts', 'checks')} for profile in reports],
        'preservation': report['preservation']}, indent=2))


if __name__ == '__main__':
    main()
