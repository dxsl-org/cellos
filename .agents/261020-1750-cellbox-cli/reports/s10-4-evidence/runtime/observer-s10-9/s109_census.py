#!/usr/bin/env python3
"""S10-9 static collision census (offline, no VM): copy-and-fix of `s108_census` for the S10-9 classes.

Question: does any method, class attribute or `self.<attr> =` instance attribute defined by a NEW S10-9 class collide with a name
that already exists in `threading.Thread`, `Observer`, `CoordinatedObserver`, `HookObserver`, `Machine` (`common.Machine` and
`phase10_canonical_drive.Machine`) or, for the RSP client, in the whole `rsp.RspClient` / `DiagnosticClient` / `RecurringClient` MRO
(the S10-7 defect was a name collision: `MemoryMixin._stop` vs `threading.Thread._stop`)?

A collision is accepted only when it is a DECLARED intentional override whose body chains with `super().<name>(` (verified from the
function source).  Everything else is `unintended` and MUST be empty.  The S10-8 classes that S10-9 reuses unchanged are censused as
well (informational; they were censused empty in S10-8).

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109_census.py [--write]

`--write` stores reports/s10-9-evidence/harness/collision-census.json.
"""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_env as ENV  # noqa: E402
import s108_census as C8  # noqa: E402  (helpers reused unchanged: own_names, mro_names, base_classes, chains_to_super, ...)
import s107_retained as RET  # noqa: E402

# Standalone helpers and mixins of S10-9: only `LatencyRecoveryMixin` is a mixin (its single own method is a pure replacement of the
# S10-8 wait factory, which does not exist in any Thread/Observer/Machine/RSP class).
MIXIN_NAMES = {'MemoryMixin', 'LatencyRecoveryMixin'}
INTENTIONAL = dict(C8.INTENTIONAL)
# Surface-side overrides are checked against the retained surface MRO (not part of the Thread/Observer/Machine question).
SURFACE_INTENTIONAL = {
    ('LatencyRecoveryMixin', '_credit_start'): 'replaces the S10-8 wait factory with the latency-recording wait (S10-8 class, not a retained surface name)',
}


def rsp_base_classes():
    """Whole MROs of the RSP clients (the headless client now uses the atomic mixin over RecurringClient)."""
    return {'rsp.RspClient': RET.X.RspClient, 'diagnostic.DiagnosticClient': RET.X.DiagnosticClient, 'recurring.RecurringClient': RET.R.RecurringClient,
            'run_atomic.AtomicClient': RET.F.AtomicClient}


def base_registry():
    registry = C8.base_registry()
    registry.update({label: C8.mro_names(cls) for label, cls in rsp_base_classes().items()})
    return registry


def census_class(cls, registry, *, label=None):
    found = C8.own_names(cls)
    defined = {'members': sorted(found['members']), 'instance_attributes': sorted(found['instance'])}
    bases = list(C8.base_classes().values()) + list(rsp_base_classes().values())
    standalone = cls.__name__ not in MIXIN_NAMES and not any(issubclass(cls, base) for base in bases)
    skipped = sorted(n for n in found['members'] if standalone and n.startswith('__') and n.endswith('__'))
    collisions = {}
    for kind in ('members', 'instance'):
        for name in sorted(found[kind]):
            if name in skipped:
                continue
            hit = sorted(base for base, names in registry.items() if name in names)
            if hit:
                collisions.setdefault(name, {'kinds': [], 'bases': hit})['kinds'].append(kind)
    intentional, unintended = [], []
    for name, item in collisions.items():
        key = (cls.__name__, name)
        reason = INTENTIONAL.get(key)
        if reason and 'members' in item['kinds'] and C8.chains_to_super(cls, name):
            intentional.append({'name': name, 'bases': item['bases'], 'reason': reason, 'chains_to_super': True})
        else:
            unintended.append({'name': name, 'bases': item['bases'], 'kinds': item['kinds']})
    return {'class': label or f'{cls.__module__}.{cls.__qualname__}', 'defined': defined, 'collisions': collisions,
            'intentional_overrides': intentional, 'unintended': unintended, 'standalone_helper_dunders_not_overrides': skipped}


def surface_overrides(cls):
    """Names the S10-9 surface mixin defines that already exist in the retained surface MRO (S10-8 mixin excluded), with the chain check."""
    import s109_credit as CREDIT
    mixin = CREDIT.LatencyRecoveryMixin
    retained = set()
    for klass in cls.__mro__:
        if klass is object or klass is mixin or klass is cls or klass is CREDIT.C8.CreditedRecoveryMixin:
            continue
        retained |= C8.own_names(klass)['members']
    result = []
    for name in sorted(C8.own_names(mixin)['members'] & retained):
        result.append({'name': name, 'chains_to_super': C8.chains_to_super(mixin, name),
                       'declared': (mixin.__name__, name) in SURFACE_INTENTIONAL})
    return result


def new_classes():
    import s109_credit as CREDIT
    import s109_run as RUN9
    return {
        's109_credit.LatencyWait': CREDIT.LatencyWait,
        's109_credit.LatencyRecoveryMixin': CREDIT.LatencyRecoveryMixin,
        's109_run.AtomicHeadlessClient': RUN9.AtomicHeadlessClient,
        's109_run.LatencyDesktopSurface': RUN9.LatencyDesktopSurface,
        's109_run.LatencyHeadlessSurface': RUN9.LatencyHeadlessSurface,
    }


def s108_classes():
    import s108_credit as CREDIT8
    import s108_memory as MEM8
    import s108_run as RUN8
    return {
        's108_memory.MemoryMixin': MEM8.MemoryMixin,
        's108_run.MemoryHookObserver': RUN8.MemoryHookObserver,
        's108_run.MemoryPlainObserver': RUN8.MemoryPlainObserver,
        's108_credit.PauseSource': CREDIT8.PauseSource,
        's108_credit.CreditedWait': CREDIT8.CreditedWait,
    }


def build_census():
    registry = base_registry()
    new = {name: census_class(cls, registry, label=name) for name, cls in new_classes().items()}
    old = {name: census_class(cls, registry, label=name) for name, cls in s108_classes().items()}
    surface = {name: surface_overrides(cls) for name, cls in new_classes().items() if 'Surface' in name}
    surface_unintended = {name: [item for item in items if not (item['declared'] and item['chains_to_super'])] for name, items in surface.items()}
    # `LatencyRecoveryMixin._credit_start` replaces (does not chain to) the S10-8 factory: verify that it exists nowhere else.
    import s109_credit as CREDIT
    s108_wait_factory = '_credit_start' in C8.own_names(CREDIT.C8.CreditedRecoveryMixin)['members']
    return {
        'schema': 'cellos.s10-9.collision-census/1',
        'question': 'names defined by new S10-9 classes vs threading.Thread, Observer, CoordinatedObserver, HookObserver, Machine, RspClient/DiagnosticClient/RecurringClient/AtomicClient (whole MROs)',
        'method': 'class __dict__ members + AST `self.<x> =` assignments in every function; base side: dir() plus the same AST over the whole MRO',
        'base_name_counts': {label: len(names) for label, names in registry.items()},
        'new_classes': new,
        'unintended_new': {name: item['unintended'] for name, item in new.items() if item['unintended']},
        'surface_chain_overrides': surface,
        'surface_chain_unintended': {name: items for name, items in surface_unintended.items() if items},
        'latency_wait_factory_replaces_s108_factory_only': s108_wait_factory,
        's108_classes_informational': old,
        's108_collisions_found': {name: item['unintended'] for name, item in old.items() if item['unintended']},
        's108_collisions_note': 'informational, not gating: S10-8 standalone helpers are never part of any censused MRO (e.g. PauseSource.observer is an attribute of '
                                'a helper object, not of a client/observer subclass); the widened RSP-client registry is what newly lists it',
        'empty': (not any(item['unintended'] for item in new.values()) and not any(surface_unintended.values()) and s108_wait_factory),
    }


if __name__ == '__main__':
    census = build_census()
    if '--write' in sys.argv:
        ENV.HARNESS_EVIDENCE.mkdir(parents=True, exist_ok=True)
        ENV.dump(ENV.HARNESS_EVIDENCE / 'collision-census.json', census)
    print(json.dumps({'empty': census['empty'], 'unintended_new': census['unintended_new'], 'surface_chain_unintended': census['surface_chain_unintended'],
                      's108_collisions_found': census['s108_collisions_found'], 'base_name_counts': census['base_name_counts']}, indent=1))
    raise SystemExit(0 if census['empty'] else 2)
