#!/usr/bin/env python3
"""Offline interpretation of the sole retained S10-4.9 attempt; never runs a VM."""
import collections
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent
EVIDENCE = ROOT / 'evidence'


def load(path):
    return json.loads(path.read_text())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    summary = load(ROOT / 'summary.json')
    steps = [json.loads(line) for line in (EVIDENCE / 'steps.jsonl').read_text().splitlines()]
    rsp = [json.loads(line) for line in (EVIDENCE / 'rsp-traffic.jsonl').read_text().splitlines()]
    commands = [(n, bytes.fromhex(row['payload_hex']).decode('ascii'))
                for n, row in enumerate(rsp, 1) if row['event'] == 'packet_tx']
    last_s = max(n for n, command in commands if command == 's')
    forbidden = [(n, c) for n, c in commands if c.startswith(('M', 'X', 'G', 'P', 'Z0', 'z0', 'qRcmd'))]
    after_control = [(n, c) for n, c in commands if n > last_s and (c == 's' or c == 'c')]
    last = steps[-1]
    previous = steps[-2]
    entries = last['trace']['tb_entries']
    exits = last['trace']['tb_exits']
    entry_match = re.fullmatch(r'tb:(0x[0-9a-f]+) pc=(0x[0-9a-f]+)', entries[0]['text'])
    exit_match = re.fullmatch(r'tb:(0x[0-9a-f]+) flags=(0x[0-9a-f]+)', exits[0]['text'])
    proof = {
        'one_candidate_run': summary['runtime']['candidate_runs'] == 1,
        'native_commands_equal_scopes': sum(c == 's' for _, c in commands) == len(steps),
        'one_terminal_same_pc_failure': summary['same_pc_failures'] == 1 and
            'did not advance' in last.get('error', ''),
        'complete_capture': all('capture_error' not in step and step['trace']['complete_ordered_scope'] for step in steps),
        'exactly_one_final_TB_entry_and_exit': len(entries) == 1 and len(exits) == 1,
        'same_TB_pointer': entry_match is not None and exit_match is not None and entry_match[1] == exit_match[1],
        'entry_PC_matches_pre_and_post_RIP': entry_match is not None and
            int(entry_match[2], 16) == last['before']['rip'] == last['after']['rip'],
        'requested_exit_flag3': exit_match is not None and int(exit_match[2], 16) == 3,
        'same_vcpu_context': last['qmp_before']['cpus'] == last['qmp_after']['cpus'] and
            entries[0]['tid'] == exits[0]['tid'] == last['qmp_before']['cpus'][0]['thread-id'],
        'same_CS_CR3_RSP_EFLAGS': all(last['before'][key] == last['after'][key] for key in ('cs', 'cr3', 'rsp', 'eflags')),
        'TF_RF_clear_before_and_after': not any(last[when][flag] for when in ('before', 'after') for flag in ('tf', 'rf')),
        'step_mode7_before_and_after': all(last[when]['sstep'] == '0x7' for when in ('before', 'after')),
        'all_execute_watch_censuses_preserved': all(step['breakpoints_before'] == step['breakpoints_after'] and
            step['watches_before'] == step['watches_after'] for step in steps),
        'no_guest_write_or_software_breakpoint_packets': not forbidden,
        'no_continue_or_step_after_failed_s': not after_control,
        'protected_inputs_and_old_trace_unchanged': summary['preservation']['unchanged'] and
            summary['preservation']['old_compressed_trace_unchanged'] and not summary['preservation']['host_source_changed'],
    }
    assert all(proof.values()), proof
    # Static host-binary inspection only: preserve the compiled path for this build ID.
    commands_disassembly = [
        ['objdump', '-d', '--start-address=0x8cdc61', '--stop-address=0x8cdd35', '/usr/bin/qemu-system-x86_64'],
        ['objdump', '-d', '--start-address=0x8cdf18', '--stop-address=0x8cdf37', '/usr/bin/qemu-system-x86_64'],
    ]
    disassembly = [{'command': command, 'output': subprocess.run(command, capture_output=True, text=True, check=True).stdout}
                   for command in commands_disassembly]
    (ROOT / 'installed-host-exit-disassembly.json').write_text(json.dumps(disassembly, indent=2) + '\n')
    raw_trace = (EVIDENCE / 'qemu-trace.log').read_bytes()
    fragment = raw_trace[last['trace']['offset']:last['trace']['end_offset']]
    assert hashlib.sha256(fragment).hexdigest() == last['trace']['raw_sha256']
    (ROOT / 'failed-step-trace.log').write_bytes(fragment)
    pfragment = raw_trace[previous['trace']['offset']:previous['trace']['end_offset']]
    (ROOT / 'preceding-successful-step-trace.log').write_bytes(pfragment)
    result = {
        'status': 'TB_NOT_STARTED_REQUESTED_EXIT_OBSERVED',
        'confidence': 'CONFIRMED for this attempt\'s unstarted-TB/zero-retirement mechanism; hidden exit-request origin and prior jmp failure attribution remain UNPROVEN',
        'original_executed_classifier_retained': summary['classification'],
        'classifier_ceiling': 'Original runner distinguishes zero TB dispatch only and conservatively labels a dispatched TB inconclusive. Offline source-contract interpretation of explicit exit flag3 adds the unstarted-TB fact without changing the original result.',
        'proof_conditions': proof,
        'scope_count': len(steps),
        'trace_health_successes': summary['successful_trace_health_scopes'],
        'site': hex(last['addr']),
        'instruction': '58 pop %rax (post-sub successor); not the earlier failed eb02 jmp site',
        'phase': last['phase'],
        'before': last['before'], 'after': last['after'],
        'tb_entry': entries[0], 'tb_exit': exits[0],
        'previous_success': {'ordinal': previous['ordinal'], 'site': hex(previous['addr']),
                             'before': previous['before'], 'after': previous['after'], 'trace': previous['trace']},
        'source_contract': {
            'url': 'https://github.com/qemu/qemu/blob/v8.2.2/include/tcg/tcg.h',
            'symbol': 'TB_EXIT_REQUESTED', 'value': 3,
            'meaning': 'The CPU exit_request flag caused return before the pointed-to TB started. Non-index exits require synchronize_from_tb/set_pc.',
            'execution_source': 'https://github.com/qemu/qemu/blob/v8.2.2/accel/tcg/cpu-exec.c',
            'native_step_caveat': 'cpu_tb_exec can set EXCP_DEBUG for enabled host single-step after a returned unstarted TB. It does not first require successful guest-instruction retirement.',
            'installed_binary_confirmation': 'Compiled return-mask/flag>1 synchronization branch at0x8cdc85..0x8cdd2c and single-step/exception-index debug branch at0x8cdcb6,0x8cdf18..0x8cdf32; exact captured objdump output retained. This is static inspection, not hidden CPUState capture.'
        },
        'not_established': ['Who/what set exit_request in this attempt',
                            'Actual pending CPU_INTERRUPT_DEBUG state',
                            'Whether the earlier 0x1080e40b1 jmp failure had the same cause',
                            'A proven-safe general additional-step completion protocol',
                            'Full-workload memory qualification or any promotion/containment gate'],
        'next_decision': 'Keep stopped, or separately approve a bounded semantics amendment after review of how explicit zero-retirement evidence can authorize any additional native control. No extra step or rerun is inferred from this result.',
        'forbidden_packets': forbidden, 'post_failure_controls': after_control,
        'native_step_packets': sum(c == 's' for _, c in commands),
        'final_native_step_packet_line': last_s,
        'trace_exit_flags': dict(collections.Counter(x['text'].rsplit('flags=', 1)[-1]
            for step in steps for x in step['trace']['tb_exits'])),
        'evidence_hashes': {str(p.relative_to(ROOT)): digest(p) for p in
                           [EVIDENCE / 'steps.jsonl', EVIDENCE / 'rsp-traffic.jsonl', EVIDENCE / 'qmp-queries.jsonl',
                            EVIDENCE / 'qemu-trace.log', ROOT / 'summary.json']},
    }
    (ROOT / 'interpretation.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: result[key] for key in ('status', 'confidence', 'scope_count', 'trace_health_successes',
          'site', 'instruction', 'native_step_packets', 'final_native_step_packet_line', 'trace_exit_flags')}, indent=2))


if __name__ == '__main__':
    main()
