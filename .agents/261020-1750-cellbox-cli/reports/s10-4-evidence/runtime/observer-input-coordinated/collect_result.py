#!/usr/bin/env python3
"""Offline S10-4.13 exact-input/metric report; never starts or controls a guest."""
from collections import Counter
import importlib.util
import json
from pathlib import Path

import coordinated as C

ROOT = Path(__file__).resolve().parent


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    result.ROOT = ROOT
    return result


P = module('retained_metric_collector', C.R.HERE / 'collect_result.py')
Q = module('retained_QMP_collector', C.PREVIOUS / 'collect_result.py')


def gui_report(runtime):
    evidence = ROOT / 'desktop/evidence'
    journal_path = evidence / 'gui-input-journal.jsonl'
    rows = list(P.records(journal_path))
    batches = runtime.get('gui_batches', [])
    input_requests = [(index, row) for index, row in enumerate(rows)
                      if row['kind'] == 'request' and row['command'] == 'input-send-event']
    source_verified = json.loads((ROOT / 'source-binding-verification.json').read_text())['verified']
    pending = None
    transaction_issues = []
    replies = []
    active_batch = None
    one_pending = True
    for index, row in enumerate(rows):
        if row['kind'] == 'batch':
            one_pending &= active_batch is None
            active_batch = row['batch_id']
        elif row['kind'] == 'finish':
            one_pending &= active_batch == row['batch_id']
            active_batch = None
        elif row['kind'] == 'request':
            if pending is not None:
                transaction_issues.append({'pending_before_next': pending, 'next_index': index})
            pending = (row['batch_id'], row['command'], row['attempt'])
        elif row['kind'] == 'reply':
            identity = (row['batch_id'], row['command'], row['attempt'])
            if identity != pending:
                transaction_issues.append({'unowned_reply_index': index, 'pending': pending, 'reply_identity': identity})
            value = row['reply']
            if isinstance(value, dict) and 'event' in value and 'return' not in value and 'error' not in value:
                continue
            replies.append((index, row))
            pending = None
    exact_error = {'error': {'class': 'GenericError', 'desc': 'VM not running'}}
    batch_reports = []
    checks = {'source_bound_rejection_verified': source_verified,
              'one_pending_GUI_batch_and_complete_finish_records': one_pending and active_batch is None,
              'all_GUI_input_requests_in_ordinary_gate_phase': all(row['phase'] == C.InputGate.ORDINARY_RESUME_PENDING_OR_RUNNING
                                                                 for _, row in input_requests),
              'complete_journal_transaction_ownership': not transaction_issues and pending is None,
              'GUI_journal_commands_only_readiness_and_original_input': all(row['command'] in ('query-status', 'input-send-event')
                  for row in rows if row['kind'] == 'request'),
              'max64_attempts_per_batch': all(len(batch['attempts']) <= 64 for batch in batches),
              'one_absolute_5s_caller_900_deadline_per_batch': True,
              'all_input_request_bytes_match_frozen_batch': True,
              'only_exact_zero_dispatch_rejection_before_resubmission': True,
              'fresh_running_readiness_before_every_input_attempt': True,
              'no_input_request_after_success_for_same_batch': True,
              'at_most_one_commit_per_batch': all(batch['commit_count'] <= 1 for batch in batches)}
    accepted_events = Counter()
    for batch in batches:
        batch_id = batch['id']
        requests = [(index, row) for index, row in input_requests if row['batch_id'] == batch_id]
        responses = [(index, row) for index, row in replies
                     if row['batch_id'] == batch_id and row['command'] == 'input-send-event']
        all_batch_rows = [(index, row) for index, row in enumerate(rows) if row['batch_id'] == batch_id]
        commits = [(index, row) for index, row in responses if row['reply'] == {'return': {}}]
        if commits:
            checks['no_input_request_after_success_for_same_batch'] &= not any(index > commits[0][0] for index, _ in requests)
        checks['all_input_request_bytes_match_frozen_batch'] &= all(row['raw_hex'] == batch['payload_hex'] for _, row in requests)
        checks['one_absolute_5s_caller_900_deadline_per_batch'] &= (
            batch['deadline'] <= batch['start'] + 5 and
            batch['deadline'] <= runtime['vm_start_ns'] / 1e9 + 900 and
            (batch['caller_deadline'] is None or batch['deadline'] <= batch['caller_deadline']) and
            all(row['time'] <= batch['deadline'] for _, row in requests))
        for request_index, request in requests:
            if request['attempt'] > 1:
                previous = [row for _, row in responses if row['attempt'] == request['attempt'] - 1]
                checks['only_exact_zero_dispatch_rejection_before_resubmission'] &= (
                    len(previous) == 1 and previous[0]['reply'] == exact_error)
            preceding = [row for index, row in all_batch_rows if index < request_index and row['kind'] == 'reply'
                         and row.get('command') == 'query-status' and isinstance(row.get('reply'), dict)
                         and 'return' in row['reply']]
            checks['fresh_running_readiness_before_every_input_attempt'] &= bool(preceding) and (
                preceding[-1]['reply']['return'].get('running') is True and
                preceding[-1]['reply']['return'].get('status') == 'running' and
                preceding[-1]['attempt'] == request['attempt'])
        if batch['commit_count']:
            payload = json.loads(bytes.fromhex(batch['payload_hex']))
            accepted_events.update(event['type'] for event in payload.get('arguments', {}).get('events', []))
        batch_reports.append({'id': batch_id, 'outcome': batch['outcome'], 'commit_count': batch['commit_count'],
                              'attempt_records': len(batch['attempts']), 'journaled_input_requests': len(requests),
                              'exact_pre_dispatch_rejections': sum(row['reply'] == exact_error for _, row in responses),
                              'start': batch['start'], 'finish': batch['finish'], 'deadline': batch['deadline'],
                              'elapsed_seconds': batch['finish'] - batch['start'],
                              'freeze_phase': batch['phase'], 'error': batch.get('error')})
    rsp_commands = [bytes.fromhex(row['payload_hex']).decode('ascii')
                    for row in P.records(evidence / 'rsp-traffic.jsonl') if row['event'] == 'packet_tx']
    return {'batch_count': len(batches), 'committed_batches': sum(batch['commit_count'] for batch in batches),
            'runtime_input_admission_exercised': bool(input_requests),
            'exercise_ceiling': ('Actual input journal present; individual checks retain their stated scope' if input_requests else
                                 'NOT_EXERCISED_PRE_GUI_STOP: zero GUI batches; true empty-set checks are not runtime admission proof'),
            'outcomes': dict(Counter(batch['outcome'] for batch in batches)),
            'input_attempt_records': sum(len(batch['attempts']) for batch in batches),
            'journaled_input_requests': len(input_requests),
            'exact_pre_dispatch_rejections': sum(row['reply'] == exact_error for _, row in replies
                                                  if row['command'] == 'input-send-event'),
            'readiness_debug_observations': sum(isinstance(row['reply'], dict) and
                isinstance(row['reply'].get('return'), dict) and
                row['reply']['return'].get('status') == 'debug' for _, row in replies
                if row['command'] == 'query-status'),
            'batches_with_deferral_attempts': sum(row['attempt_records'] > 1 for row in batch_reports),
            'batches_frozen_during_paused_gate': sum(batch['phase'] == C.InputGate.PAUSED_OR_STEPPING for batch in batches),
            'accepted_event_types': dict(accepted_events), 'batch_summaries': batch_reports,
            'natural_resume_epoch': runtime.get('natural_resume_epoch'),
            'ordinary_RSP_continue_packets': rsp_commands.count('c'),
            'natural_resume_epoch_matches_original_continue_count': runtime.get('natural_resume_epoch') == rsp_commands.count('c'),
            'gate_terminal_reason': runtime.get('gate_terminal_reason'), 'checks': checks,
            'transaction_issues': transaction_issues, 'pending_journal_transaction': pending,
            'journal_sha256': C.X.sha(journal_path) if journal_path.exists() else None,
            'limits': ['Commit is emulator acceptance, not guest consumption or full-workload completion',
                       'Request is journaled before write; a failed/short write may have ambiguous delivery and never permits resend',
                       'Batch identities/order are separate even when two original batches contain identical keys',
                       'Known late acceptance can coexist with terminal caller timeout; commit_count must not be erased']}


def main():
    summary = json.loads((ROOT / 'summary.json').read_text())
    desktop = P.profile_report('desktop', summary['preservation'])
    gui = gui_report(summary['desktop'])
    report = {'status': summary['status'], 'desktop': desktop, 'GUI_input_admission': gui,
              'QMP_transaction_audit': Q.qmp_audit(), 'preservation': summary['preservation'],
              'host_tests': json.loads((ROOT / 'host-tests.json').read_text()), 'headless_runs': 0,
              'cbx27_met': False, 'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
              'retained_stop_framing_finding': 'retained-stop-framing-finding.json',
              'CP2_gate': 'Accept actual workload/event-coverage/input-admission completion or exact STOP/partial bounds only; previous headless command deadline and baseline/exactstack/RSS/canonical/containment gaps remain.'}
    C.X.dump(ROOT / 'interpretation.json', report)
    C.R.manifest(ROOT / 'desktop')
    C.R.manifest(ROOT)
    print(json.dumps({'status': report['status'], 'desktop': {key: desktop[key] for key in
        ('status', 'error', 'observer_error', 'cleanup_error', 'workload_complete', 'complete_event_coverage_for_approved_schedule',
         'native_step_packets', 'scope_records', 'original_same_pc_failures', 'additional_native_packets',
         'event_outcomes', 'instances', 'draw_entry_counts', 'checks')},
        'GUI_input_admission': {key: gui[key] for key in gui if key not in ('batch_summaries',)},
        'QMP_transaction_audit': report['QMP_transaction_audit'], 'preservation': report['preservation']}, indent=2))


if __name__ == '__main__':
    main()
