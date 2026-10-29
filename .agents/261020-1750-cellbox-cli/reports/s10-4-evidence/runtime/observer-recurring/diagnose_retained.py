#!/usr/bin/env python3
"""Read-only diagnosis from retained runs; no workload/recovery/fix execution."""
import json
from pathlib import Path

import recurring as R

ROOT = Path(__file__).resolve().parent
SECOND = 1_000_000_000


def records(path):
    with path.open() as source:
        for line in source:
            yield json.loads(line)


def overlap(intervals, left, right):
    return sum(max(0, min(end, right) - max(start, left)) for start, end in intervals)


def main():
    headless = ROOT / 'headless'
    evidence = headless / 'evidence'
    observation = json.loads((evidence / 'observation.json').read_text())
    final_snapshot = observation['snapshots'][-1]
    assert final_snapshot['phase'] == 'interactive-top-idle'
    lower_wait_start = final_snapshot['monotonic_ns']
    packets = []
    stopped_intervals = []
    stopped_since = None
    cleanup = None
    for row in records(evidence / 'rsp-traffic.jsonl'):
        stamp = row['monotonic_ns']
        if row['event'] == 'state' and row['state'] == 'stopped' and stopped_since is None:
            stopped_since = stamp
        if row['event'] != 'packet_tx':
            continue
        command = bytes.fromhex(row['payload_hex']).decode('ascii')
        if command in ('s', 'c'):
            packets.append((stamp, command))
            if stopped_since is not None:
                stopped_intervals.append((stopped_since, stamp))
                stopped_since = None
        # Persistent heap watch is removed only during owned observer shutdown, not page-first-hit handling.
        if command == 'z2,1088a80c0,8':
            cleanup = stamp
            if stopped_since is not None:
                stopped_intervals.append((stopped_since, stamp))
            break
    assert cleanup is not None and cleanup - lower_wait_start >= 10 * SECOND
    # Ctrl-C send is not timestamped. Actual wait began after the completed snapshot,
    # and its original10s timeout occurred before the shutdown-only heap-watch removal.
    # Hence start w is in[S,C-10s]. Every possible [w,w+10s] contains[C-10s,S+10s].
    common_left = cleanup - 10 * SECOND
    common_right = lower_wait_start + 10 * SECOND
    assert common_right >= common_left
    paused_lower_bound = overlap(stopped_intervals, common_left, common_right)
    heap_events = [event for event in records(evidence / 'heap-events.jsonl')
                   if common_left <= event['monotonic_ns'] <= common_right]
    receipts = json.loads((evidence / 'serial-receipts.json').read_text())
    final_receipt = receipts[-1]
    pause_report = {
        'failure': 'Original10s wait for new interactive-top interrupt prompt expired',
        'Ctrl_C_send_timestamp_retained': False,
        'snapshot_lower_wait_start_ns': lower_wait_start,
        'shutdown_only_heap_watch_remove_upper_timeout_ns': cleanup,
        'possible_actual_wait_start_bounds_ns': [lower_wait_start, cleanup - 10 * SECOND],
        'interval_contained_in_every_possible_original10s_wait_ns': [common_left, common_right],
        'common_interval_seconds': (common_right - common_left) / SECOND,
        'proven_stopped_seconds_minimum_within_actual10s_wait': paused_lower_bound / SECOND,
        'time_not_proven_stopped_seconds_upper_bound_within_actual10s_wait':
            (10 * SECOND - paused_lower_bound) / SECOND,
        'method': 'Union of RSP stopped-state receipt to next native-s/continue packet_tx (before send). Use only intersection of all possible timeout windows, not an invented exact Ctrl-C time.',
        'native_s_packets_in_common_interval': sum(command == 's' and common_left <= stamp <= common_right
                                                   for stamp, command in packets),
        'continue_packets_in_common_interval': sum(command == 'c' and common_left <= stamp <= common_right
                                                   for stamp, command in packets),
        'retained_heap_post_events_in_common_interval': len(heap_events),
        'heap_values_in_common_interval': {
            'first': heap_events[0]['aligned_used_bytes'] if heap_events else None,
            'last': heap_events[-1]['aligned_used_bytes'] if heap_events else None},
        'last_serial_receipt': final_receipt,
        'last_serial_receipt_before_possible_interrupt_seconds':
            (lower_wait_start - final_receipt['monotonic_ns']) / SECOND,
        'completed_stop_interval_pause_total_seconds': observation['observer_pause_ns'] / SECOND,
        'aggregate_pause_counter_caveat': 'Observer.pause_ns sums completed _stopped handling intervals, including native-step execution and snapshot handling. Its103.061s whole-run total is not pure guest-stopped time or the10s interrupt interval and must not simply be subtracted from that deadline.',
        'conclusion': 'The unchanged wall-clock deadline spent at least the measured bound debugger-stopped, while native/heap progress remained observable. This is not evidence of a product interrupt failure. Retained data do not prove when guest consumed Ctrl-C or whether it would have produced the prompt within10 unpaused seconds.',
        'not_authorized': 'No timeout extension, pause-excluded deadline, observer coverage/proof reduction, or rerun was applied.'}
    desktop = ROOT / 'desktop'
    qmp = list(records(desktop / 'evidence/qmp-queries.jsonl'))
    tail = qmp[-5:]
    assert tail[-2]['message']['execute'] == 'query-status' and isinstance(tail[-1]['message']['return'], list)
    source_refs = {
        'machine_owner': (R.X.RUNTIME / 'common.py', 'Machine.start line60 creates one machine.qmp = D.Qmp'),
        'desktop_same_owner': (R.X.R / 'scripts/phase10_canonical_drive.py', 'Desk.__init__ line929 assigns self.qmp = mach.qmp; type_text line961 uses self.qmp.keys'),
        'GUI_transaction': (R.X.R / 'scripts/phase04_gui_drive.py', 'Qmp lines188-218 owns one unbuffered rwb socket file; cmd(name, **args) writes/reads entire replies without lock or request IDs; keys and grab call this same cmd'),
        'observer_same_file': (R.A.PREVIOUS / 'diagnostic.py', 'TraceControl.query lines85-110 writes/reads self.machine.qmp.file directly without transaction lock; retained by AmendedTraceControl and RecurringClient')}
    qmp_report = {
        'kind': 'Host shared-transport ownership/concurrency defect, not product finding',
        'actual_failure': json.loads((desktop / 'runtime-result.json').read_text())['error'],
        'observer_failure': json.loads((desktop / 'runtime-result.json').read_text())['observer_error'],
        'source_evidence': {name: {'path': str(path), 'sha256': R.X.sha(path), 'contract': contract}
                            for name, (path, contract) in source_refs.items()},
        'retained_final_QMP_order': tail,
        'observed_misattribution': 'query-status receives a CPU-list return, not a status object; GUI input simultaneously raises malformed-JSON and observer context capture fails.',
        'exact_corrupted_JSON_bytes_retained': False,
        'boundary': '47th scope fails during pre-step QMP capture, before before/trace/native-s;46 native packets total,0 additional; no complete terminal census proof, not evidence of actual watch removal.',
        'minimal_proposed_fix_not_applied': [
            'One shared host threading.RLock per machine.qmp transport, installed before observer/GUI use.',
            'Wrap actual Qmp instance cmd for the full write-through-matching-return/error transaction. Existing keys/abs_move/buttons/grab then use the same wrapped cmd; do not lock only writes or individual readline calls.',
            'A new host TraceControl.query override holds that identical lock around the entire unchanged policy-checked original query body, including trace commands whose arguments contain name.',
            'Do not route trace commands through existing cmd(name, **args) unmodified: name is both Python positional parameter and QMP trace argument. Do not rename/bypass public API or alter immutable retained helpers.',
            'Release lock on all exceptions; no holding it across GUI waits or snapshot queue waits; retain original deadlines, proof gates, event logging and coverage.'
        ],
        'required_deterministic_tests_before_any_replacement': [
            'Barrier-controlled simultaneous GUI input/screendump and observer CPU/status/trace transactions cannot interleave bytes or consume another response.',
            'Trace-event name arguments preserved; asynchronous events skipped without losing ownership; error/EOF/timeout releases lock.',
            'No lock held during cross-thread snapshot/wait; all existing recurrence/admission/one-extra/terminal/deadline invariants retained.'
        ],
        'next_execution_gate': 'Separate explicit approval for bounded fresh desktop replacement after host-only fix/tests; do not reinterpret recurring recovery as retry-run permission. Headless retry/budget-policy change is not included.'}
    report = {'scope': 'Read-only post-run diagnosis; no candidate rerun or host fix applied',
              'headless_pause_accounting': pause_report, 'desktop_QMP_ownership': qmp_report,
              'evidence_sha256': {str(path.relative_to(ROOT)): R.X.sha(path) for path in
                  (evidence / 'rsp-traffic.jsonl', evidence / 'serial-receipts.json', evidence / 'heap-events.jsonl',
                   headless / 'runtime-result.json', desktop / 'runtime-result.json',
                   desktop / 'evidence/qmp-queries.jsonl', desktop / 'evidence/steps.jsonl')}}
    R.X.dump(ROOT / 'retained-failure-diagnosis.json', report)
    print(json.dumps({'headless_pause_accounting': pause_report, 'desktop_QMP_boundary': qmp_report['boundary'],
                      'observed_misattribution': qmp_report['observed_misattribution']}, indent=2))


if __name__ == '__main__':
    main()
