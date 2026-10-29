"""S10-4.17 final outcome classes A-E (pure). Every class is final; none authorizes a rerun."""
import re

DEADLINE = re.compile(r'timeout waiting for|timeout:|expired|deadline', re.I)

CLASSES = {
    'A': 'SUCCESS_COMPLETE',
    'B': 'SUCCESS_METRIC_ONLY',
    'C': 'OBSERVABILITY_INCOMPLETE',
    'D': 'OBSERVED_NEGATIVE_RESULT',
    'E': 'HOST_FAILURE',
}


def stop_class(facts):
    """Class of the STOP itself, ignoring whether the metric had already been proved."""
    error = facts.get('error') or facts.get('observer_error') or ''
    oracle = facts.get('oracle_failure')
    hook = facts.get('hook_failure')
    if oracle:
        return oracle['cls'], f"oracle {oracle['code']}"
    if hook:
        return 'C', f"hook {hook['code']}"
    if error and DEADLINE.search(error):
        return 'C', 'deadline expiry, cause not attributable'
    if error:
        return 'E', error
    return None, None


def classify(facts):
    host = bool(facts.get('cleanup_error') or facts.get('preservation_ok') is False or facts.get('ledger_io_error'))
    stop, why = stop_class(facts)
    if facts.get('metric_complete'):
        clean = facts.get('status') == 'COMPLETE' and not host and stop is None
        return {'class': 'A' if clean else 'B', 'name': CLASSES['A' if clean else 'B'],
                'tail_reason': None if clean else (why or ('host: cleanup/preservation' if host else 'schedule incomplete')),
                'tail_stop_class': stop}
    if host or stop == 'E':
        return {'class': 'E', 'name': CLASSES['E'], 'reason': why or 'host: cleanup/preservation/ledger I/O'}
    if stop in ('C', 'D'):
        return {'class': stop, 'name': CLASSES[stop], 'reason': why}
    return {'class': 'C', 'name': CLASSES['C'], 'reason': 'run ended without a proved metric and without a recorded cause'}
