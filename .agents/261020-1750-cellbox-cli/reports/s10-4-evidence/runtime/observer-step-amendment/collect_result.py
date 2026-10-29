#!/usr/bin/env python3
"""Offline report for the one S10-4.10 attempt; never resumes or launches anything."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / 'evidence'


def main():
    summary = json.loads((ROOT / 'summary.json').read_text())
    rows = [json.loads(line) for line in (EVIDENCE / 'rsp-traffic.jsonl').read_text().splitlines()]
    steps_path = EVIDENCE / 'steps.jsonl'
    steps = [json.loads(line) for line in steps_path.read_text().splitlines()] if steps_path.exists() else []
    commands = [(n, bytes.fromhex(row['payload_hex']).decode('ascii')) for n, row in enumerate(rows, 1)
                if row['event'] == 'packet_tx']
    native = [(n, command) for n, command in commands if command == 's']
    forbidden = [(n, command) for n, command in commands
                 if command.startswith(('M', 'X', 'G', 'P', 'Z0', 'z0', 'qRcmd', 'Qqemu.sstep'))]
    no_progress = [s for s in steps if 'did not advance current breakpoint' in s.get('error', '')]
    additional = [s for s in steps if s.get('kind') == 'single-approved-additional-step']
    decisions = summary['decisions']
    final_native_line = native[-1][0] if native else None
    post_terminal_controls = [(n, command) for n, command in commands
                              if final_native_line is not None and n > final_native_line and command in ('s', 'c')]
    checks = {
        'one_candidate_run': summary['runtime']['candidate_runs'] == 1,
        'no_guest_write_or_Z0_injection_packets': not forbidden,
        'at_most_one_decision_and_additional_scope': len(decisions) <= 1 and len(additional) <= 1,
        'at_most_one_additional_native_packet': summary['extra_steps_used'] <= 1,
        'no_continuation_after_terminal_native_step': not post_terminal_controls,
        'no_recurrence_authorized': summary['recurrence_authorized'] is False,
        'all_execute_watch_censuses_unchanged': all(s['breakpoints_before'] == s['breakpoints_after'] and
            s['watches_before'] == s['watches_after'] for s in steps),
        'canonical_candidate_userVMX_unchanged': summary['preservation']['unchanged'],
        'previous_diagnostic_unchanged': summary['preservation']['previous_diagnostic_unchanged'],
        'old_compressed_trace_unchanged': summary['preservation']['old_compressed_trace_unchanged'],
        'executing_host_sources_unchanged': not summary['preservation']['host_source_changed'],
    }
    assert all(checks.values()), checks
    report = {'status': summary['status'], 'checks': checks, 'native_step_packets': len(native),
              'native_scope_records': len(steps), 'no_progress_records': len(no_progress),
              'additional_native_packets': summary['extra_steps_used'],
              'original_no_progress_failures_preserved': True,
              'full_observed_workload_complete': False, 'recurrence_authorized': False,
              'root_exit_request_origin': 'UNPROVEN', 'forbidden_packets': forbidden,
              'post_terminal_controls': post_terminal_controls}
    raw_trace = (EVIDENCE / 'qemu-trace.log').read_bytes()
    if decisions:
        decision = decisions[0]
        report['decision'] = decision
        first = next(s for s in steps if s['ordinal'] == decision['first_ordinal'])
        report['first_failure'] = {key: first.get(key) for key in
                                  ('ordinal', 'addr', 'phase', 'before', 'after', 'qmp_before', 'qmp_after', 'error', 'capture_error', 'trace')}
        first_native_line = native[first['ordinal'] - 1][0]
        after_first = [(n, c) for n, c in commands if n > first_native_line and c in ('s', 'c')]
        assert sum(c == 's' for _, c in after_first) == summary['extra_steps_used']
        assert not any(c == 'c' for _, c in after_first)
        report['controls_after_first_no_progress_native'] = after_first
        selected = [('first-not-started-step.log', first)]
        if decision.get('proof'):
            health = next(s for s in steps if s['ordinal'] == decision['proof']['health_ordinal'])
            selected.append(('preceding-proof-health-step.log', health))
        if additional:
            extra = additional[0]
            report['additional'] = {key: extra.get(key) for key in
                                   ('ordinal', 'addr', 'before', 'after', 'returned_stop', 'error', 'capture_error', 'trace', 'additional_native_packet_issued')}
            if 'before' in extra and 'after' in extra:
                report['architectural_effects'] = {'rip_delta': extra['after']['rip'] - extra['before']['rip'],
                                                   'rsp_delta': extra['after']['rsp'] - extra['before']['rsp'],
                                                   'before_instruction_bytes': extra['before']['instruction_bytes']}
            if 'trace' in extra:
                selected.append(('single-additional-step.log', extra))
        for name, step in selected:
            if 'trace' not in step:
                continue
            trace = step['trace']
            raw = raw_trace[trace['offset']:trace['end_offset']]
            assert hashlib.sha256(raw).hexdigest() == trace['raw_sha256']
            (ROOT / name).write_bytes(raw)
    report['next_gate'] = ('Explicit recurrence/workload-continuation CP1 remains required; this approval '
                           'permits only the one global additional operation and diagnostic stop. '
                           'No further step, candidate run, desktop or full qualification is inferred.')
    report['evidence_hashes'] = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                               for path in (ROOT / 'summary.json', steps_path, EVIDENCE / 'rsp-traffic.jsonl',
                                            EVIDENCE / 'qmp-queries.jsonl', EVIDENCE / 'qemu-trace.log') if path.exists()}
    (ROOT / 'interpretation.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report.get(key) for key in ('status', 'native_step_packets', 'no_progress_records',
          'additional_native_packets', 'architectural_effects', 'checks')}, indent=2))


if __name__ == '__main__':
    main()
