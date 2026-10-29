#!/usr/bin/env python3
"""Offline atomic receive/input/metric evidence only, no VM controls."""
import importlib.util
import json
from pathlib import Path

import run_atomic as F

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('prior_coordinated_collector', F.PREVIOUS / 'collect_result.py')
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)
P.ROOT = P.P.ROOT = P.Q.ROOT = ROOT


def wire_audit():
    evidence = ROOT / 'desktop/evidence'
    pending = []
    delivered = []
    terminal = []
    raw_ack = 0
    pending_count = 0
    for row in P.P.records(evidence / 'rsp-traffic.jsonl'):
        if row['event'] == 'wire_frame_pending':
            pending_count += 1
            pending.append((row['monotonic_ns'], row['payload_hex']))
        elif row['event'] == 'packet_rx':
            delivered.append(row['payload_hex'])
            if pending and pending[0][1] == row['payload_hex']:
                pending.pop(0)
        elif row['event'] == 'tx' and row['raw_hex'] == '2b':
            raw_ack += 1
        elif row['event'] == 'wire_terminal':
            terminal.append(row)
    return {'validated_pending_frames': pending_count, 'decoded_packet_dispatches': len(delivered),
            'positive_ACK_attempts': raw_ack, 'pending_at_end': pending,
            'terminal_wire_errors': terminal,
            'scope': 'One pending-frame record per validated frame; packet_rx after successful ACK. ACK-attempt log precedes send and alone never proves delivery.',
            'complete_ACK_dispatch_counts_match': pending_count == len(delivered) == raw_ack and not pending and not terminal}


def main():
    summary = json.loads((ROOT / 'summary.json').read_text())
    desktop = P.P.profile_report('desktop', summary['preservation'])
    gui = P.gui_report(summary['desktop'])
    report = {'status': summary['status'], 'desktop': desktop, 'GUI_input_admission': gui,
              'QMP_transaction_audit': P.Q.qmp_audit(), 'RSP_atomic_receive_audit': wire_audit(),
              'preservation': summary['preservation'], 'host_tests': json.loads((ROOT / 'host-tests.json').read_text()),
              'deterministic_before': json.loads((ROOT / 'reproduction-before.json').read_text()),
              'deterministic_after': json.loads((ROOT / 'reproduction-after.json').read_text()),
              'headless_runs': 0, 'cbx27_met': False, 'phase10_criterion4_met': False,
              'historical_runtime_baseline': None,
              'CP2_gate': 'Accept actual deterministic fix and single-desktop workload/coverage or exact partial STOP only; prior headless deadline and historical comparator/exactstack/RSS/promotion/containment ceilings remain.'}
    F.X.dump(ROOT / 'interpretation.json', report)
    F.R.manifest(ROOT / 'desktop')
    F.R.manifest(ROOT)
    print(json.dumps({'status': report['status'], 'desktop': {key: desktop[key] for key in
        ('status', 'error', 'observer_error', 'cleanup_error', 'workload_complete', 'complete_event_coverage_for_approved_schedule',
         'native_step_packets', 'scope_records', 'original_same_pc_failures', 'additional_native_packets',
         'event_outcomes', 'instances', 'draw_entry_counts', 'checks')},
        'GUI_input_admission': {key: gui[key] for key in gui if key != 'batch_summaries'},
        'RSP_atomic_receive_audit': report['RSP_atomic_receive_audit'],
        'QMP_transaction_audit': report['QMP_transaction_audit'], 'preservation': report['preservation']}, indent=2))


if __name__ == '__main__':
    main()
