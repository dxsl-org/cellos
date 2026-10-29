#!/usr/bin/env python3
"""Offline S10-4.10 replacement report; no VM launch, step or continuation."""
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
    forbidden = [(n, c) for n, c in commands if c.startswith(('M', 'X', 'G', 'P', 'Z0', 'z0', 'qRcmd', 'Qqemu.sstep'))]
    additional = [s for s in steps if s.get('kind') == 'single-approved-additional-step']
    terminal_line = native[-1][0] if native else None
    terminal_controls = [(n, c) for n, c in commands if terminal_line is not None and n > terminal_line and c in ('s', 'c')]
    checks = {
        'one_authorized_replacement_run': summary['runtime']['candidate_runs'] == 1,
        'single_global_additional_packet_limit': summary['extra_steps_used'] <= 1,
        'single_admission_boundary_only': len(summary['decisions']) <= 1 and len(additional) <= 1,
        'no_guest_write_Z0_injection_or_mode_setter': not forbidden,
        'no_post_terminal_step_or_continue': not terminal_controls,
        'no_recurrence_authorized': summary['recurrence_authorized'] is False,
        'execute_watch_censuses_preserved': all(s['breakpoints_before'] == s['breakpoints_after'] and
            s['watches_before'] == s['watches_after'] for s in steps),
        'canonical_candidate_userVMX_unchanged': summary['preservation']['unchanged'],
        'initial_failed_host_admission_unchanged': summary['preservation']['initial_failed_admission_unchanged'],
        'S10_4_9_diagnostic_unchanged': summary['preservation']['previous_diagnostic_unchanged'],
        'old_compressed_trace_unchanged': summary['preservation']['old_compressed_trace_unchanged'],
        'executing_host_sources_unchanged': not summary['preservation']['host_source_changed'],
    }
    assert all(checks.values()), checks
    report = {'status': summary['status'], 'checks': checks,
              'native_step_packets': len(native), 'scope_records': len(steps),
              'additional_native_packets': summary['extra_steps_used'],
              'same_pc_guard_failures': sum('did not advance current breakpoint' in s.get('error', '') for s in steps),
              'original_no_progress_result_preserved': True, 'terminal_controls': terminal_controls,
              'recurrence_authorized': False, 'full_workload_qualification': False,
              'root_exit_request_origin': 'UNPROVEN'}
    trace_bytes = (EVIDENCE / 'qemu-trace.log').read_bytes()
    if summary['decisions']:
        decision = summary['decisions'][0]
        report['decision'] = decision
        first = next(s for s in steps if s['ordinal'] == decision['first_ordinal'])
        keys = ('ordinal', 'addr', 'phase', 'before', 'after', 'qmp_before', 'qmp_after', 'error', 'capture_error', 'trace')
        report['first_failure'] = {key: first.get(key) for key in keys}
        first_line = native[first['ordinal'] - 1][0]
        after_first = [(n, c) for n, c in commands if n > first_line and c in ('s', 'c')]
        assert sum(c == 's' for _, c in after_first) == summary['extra_steps_used']
        assert not any(c == 'c' for _, c in after_first)
        report['controls_after_first_failure'] = after_first
        selected = [('first-not-started-step.log', first)]
        if additional:
            extra = additional[0]
            report['additional'] = {key: extra.get(key) for key in
                                   keys + ('returned_stop', 'additional_native_packet_issued')}
            if 'before' in extra and 'after' in extra:
                report['architectural_effects'] = {'rip_delta': extra['after']['rip'] - extra['before']['rip'],
                                                   'rsp_delta': extra['after']['rsp'] - extra['before']['rsp'],
                                                   'before_instruction_bytes': extra['before']['instruction_bytes']}
            selected.append(('single-additional-step.log', extra))
        for name, step in selected:
            if 'trace' not in step:
                continue
            trace = step['trace']
            raw = trace_bytes[trace['offset']:trace['end_offset']]
            assert hashlib.sha256(raw).hexdigest() == trace['raw_sha256']
            (ROOT / name).write_bytes(raw)
    report['limits'] = ['This was one replacement diagnostic, not resumed full-workload observation',
                        'Retained same-binary trace health is not same-address code identity across different cells',
                        'One additional-step outcome does not establish an unbounded or per-occurrence recovery rule',
                        'No further run/control, desktop, peak-memory/baseline, canonical or containment gate follows']
    report['next_gate'] = 'Report this boundary outcome; any recurring use or workload continuation needs explicit approval.'
    report['evidence_hashes'] = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
                               for p in (ROOT / 'summary.json', steps_path, EVIDENCE / 'rsp-traffic.jsonl',
                                         EVIDENCE / 'qmp-queries.jsonl', EVIDENCE / 'qemu-trace.log') if p.exists()}
    (ROOT / 'interpretation.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report.get(key) for key in ('status', 'native_step_packets', 'same_pc_guard_failures',
          'additional_native_packets', 'architectural_effects', 'checks')}, indent=2))


if __name__ == '__main__':
    main()
