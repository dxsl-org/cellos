#!/usr/bin/env python3
"""Offline S10-4.12 desktop report, reusing immutable S10-4.11 metric semantics."""
import importlib.util
import json
from pathlib import Path

import serialized as S

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('retained_observed_collector', S.PREVIOUS / 'collect_result.py')
C = importlib.util.module_from_spec(spec)
spec.loader.exec_module(C)
C.ROOT = ROOT


def qmp_audit():
    path = ROOT / 'desktop/evidence/qmp-queries.jsonl'
    current = None
    mismatches = []
    counts = {}
    with path.open() as source:
        for line in source:
            row = json.loads(line)
            message = row['message']
            if row['event'] == 'request':
                if current is not None:
                    mismatches.append({'pending_request_before_next': current, 'next': message})
                current = message
                name = message['execute']
                counts[name] = counts.get(name, 0) + 1
            elif 'return' in message:
                if current is None:
                    mismatches.append({'unowned_return': message})
                    continue
                name, value = current['execute'], message['return']
                if name == 'query-status':
                    valid = isinstance(value, dict) and 'status' in value and 'running' in value
                elif name == 'query-cpus-fast':
                    valid = isinstance(value, list) and len(value) == 1 and value[0].get('cpu-index') == 0
                elif name == 'trace-event-get-state':
                    valid = isinstance(value, list) and len(value) == 1 and value[0].get('name') == current['arguments']['name']
                elif name == 'trace-event-set-state':
                    valid = value == {}
                else:
                    valid = False
                if not valid:
                    mismatches.append({'request': current, 'return': message})
                current = None
            elif 'error' in message:
                mismatches.append({'request': current, 'error': message})
                current = None
    return {'request_counts': counts, 'response_type_or_ownership_mismatches': mismatches,
            'pending_request_at_end': current, 'complete_recorded_transactions': not mismatches and current is None,
            'scope': 'Audit of recorded observer QMP request/returns; GUI shares the same installed full-transaction mutex. This does not invent raw GUI reply logging.'}


def main():
    summary = json.loads((ROOT / 'summary.json').read_text())
    desktop = C.profile_report('desktop', summary['preservation'])
    report = {'status': summary['status'], 'desktop': desktop,
              'QMP_lock_installation': json.loads((ROOT / 'desktop/evidence/qmp-lock-installation.json').read_text()),
              'QMP_transaction_audit': qmp_audit(), 'preservation': summary['preservation'],
              'host_tests': json.loads((ROOT / 'host-tests.json').read_text()), 'headless_runs': 0,
              'cbx27_met': False, 'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
              'CP2_gate': 'Accept actual desktop completion/coverage or partial results and deviations only. Prior headless timeout, historical comparator, exact stack/kernel-stack/RSS, canonical and containment gates do not close automatically.'}
    S.X.dump(ROOT / 'interpretation.json', report)
    S.R.manifest(ROOT / 'desktop')
    S.R.manifest(ROOT)
    print(json.dumps({'status': report['status'], 'desktop': {key: desktop[key] for key in
        ('status', 'error', 'observer_error', 'cleanup_error', 'workload_complete',
         'complete_event_coverage_for_approved_schedule', 'native_step_packets', 'scope_records',
         'original_same_pc_failures', 'additional_native_packets', 'event_outcomes', 'instances',
         'draw_entry_counts', 'checks')}, 'QMP_transaction_audit': report['QMP_transaction_audit'],
         'preservation': report['preservation']}, indent=2))


if __name__ == '__main__':
    main()
