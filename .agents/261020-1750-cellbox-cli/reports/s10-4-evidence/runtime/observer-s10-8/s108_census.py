#!/usr/bin/env python3
"""S10-8.0 static collision census (offline, no VM).

Question: does any method, class attribute or `self.<attr> =` instance attribute defined by a new mixin/class collide with a
name that already exists in `threading.Thread`, `Observer`, `CoordinatedObserver`, `HookObserver` or `Machine`
(`common.Machine` and `phase10_canonical_drive.Machine`), whole MRO of each, names taken from the live class dicts and from the
`self.x =` assignments found by AST in every function of those MROs?

A name that collides is only accepted when it is a DECLARED intentional override whose function body chains to the overridden
method with `super().<name>(` (verified here from the function source).  Everything else is `unintended`, which MUST be empty.

    PYTHONDONTWRITEBYTECODE=1 python3 -B s108_census.py [--write]

`--write` stores reports/s10-8-evidence/harness/collision-census.json.  The S10-7 classes are censused as well (informational):
the census reproduces the S10-7 defect (`MemoryMixin._stop` vs `threading.Thread._stop`) and shows it is gone in S10-8.
"""
import ast
import inspect
import json
import sys
import textwrap
import threading
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s108_env as ENV  # noqa: E402
import s107_retained as RET  # noqa: E402  (puts every retained archive on sys.path)

SKIP = {'__module__', '__dict__', '__weakref__', '__doc__', '__qualname__', '__annotations__', '__firstlineno__', '__static_attributes__',
        '__slotnames__', '__abstractmethods__', '_abc_impl'}

# (class name, attribute) -> why this overriding is intended; each must chain with super().<name>( in its body.
INTENTIONAL = {
    ('MemoryMixin', '__init__'): 'constructor extension, chains to the retained observer constructor',
    ('MemoryMixin', '_entry'): 'adds the entry-stop memory record after the retained entry handling',
    ('MemoryMixin', '_snapshot'): 'adds the memory record after the retained snapshot handling',
    ('MemoryMixin', 'write_results'): 'adds memory-summary.json after the retained result writer',
}
# Surface-side overrides are checked against the retained surface MRO (not part of the Thread/Observer/Machine question).
SURFACE_INTENTIONAL = {
    ('CreditedRecoveryMixin', 'wait'): 'D8-1=B: only the 10 s `new interrupt prompt` wait is credited, all else delegates',
    ('CreditedRecoveryMixin', 'gui_fresh'): 'D8-1=B: only the 10 s `runtime new GUI prompt` desk wait is credited, all else delegates',
}


def _self_assignments(function):
    try:
        tree = ast.parse(textwrap.dedent(inspect.getsource(function)))
    except (OSError, TypeError, SyntaxError, IndentationError):
        return set()
    found = set()
    for node in ast.walk(tree):
        targets = []
        if isinstance(node, ast.Assign):
            targets = node.targets
        elif isinstance(node, (ast.AugAssign, ast.AnnAssign)):
            targets = [node.target]
        for target in ast.walk(ast.Tuple(elts=targets, ctx=ast.Store())):
            if isinstance(target, ast.Attribute) and isinstance(target.value, ast.Name) and target.value.id == 'self':
                found.add(target.attr)
    return found


def _unwrap(value):
    if isinstance(value, (staticmethod, classmethod)):
        return value.__func__
    if isinstance(value, property):
        return value.fget
    return value


def own_names(cls):
    """{'members': names in cls.__dict__, 'instance': names assigned as self.<x> in its functions} for ONE class (no MRO)."""
    members, instance = set(), set()
    for name, value in vars(cls).items():
        if name in SKIP:
            continue
        members.add(name)
        function = _unwrap(value)
        if inspect.isfunction(function):
            instance |= _self_assignments(function)
    return {'members': members, 'instance': instance}


def mro_names(cls):
    """Union of own_names over the whole MRO (object excluded) plus dir(cls)."""
    names = set(n for n in dir(cls) if n not in SKIP)
    for klass in cls.__mro__:
        if klass is object:
            continue
        found = own_names(klass)
        names |= found['members'] | found['instance']
    return names


MIXIN_NAMES = {'MemoryMixin', 'CreditedRecoveryMixin'}


def base_classes():
    import common as COMMON
    D = COMMON.D
    return {'threading.Thread': threading.Thread, 'observer.Observer': RET.OBS.Observer, 'coordinated.CoordinatedObserver': RET.C.CoordinatedObserver,
            'hook_observer.HookObserver': RET.HO.HookObserver, 'common.Machine': COMMON.Machine, 'phase10_canonical_drive.Machine': D.Machine}


def base_registry():
    return {label: mro_names(cls) for label, cls in base_classes().items()}


def chains_to_super(cls, name):
    function = _unwrap(vars(cls)[name])
    try:
        return f'super().{name}(' in inspect.getsource(function)
    except (OSError, TypeError):
        return False


def census_class(cls, registry, *, label=None):
    found = own_names(cls)
    defined = {'members': sorted(found['members']), 'instance_attributes': sorted(found['instance'])}
    # A class that is neither a mixin nor derived from any of the censused bases is a standalone helper: its own `__init__`
    # (language protocol, not an override of anything) is recorded but not a collision.
    standalone = cls.__name__ not in MIXIN_NAMES and not any(issubclass(cls, base) for base in base_classes().values())
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
        reason = INTENTIONAL.get(key) or SURFACE_INTENTIONAL.get(key)
        if reason and 'members' in item['kinds'] and chains_to_super(cls, name):
            intentional.append({'name': name, 'bases': item['bases'], 'reason': reason, 'chains_to_super': True})
        else:
            unintended.append({'name': name, 'bases': item['bases'], 'kinds': item['kinds']})
    return {'class': label or f'{cls.__module__}.{cls.__qualname__}', 'defined': defined, 'collisions': collisions,
            'intentional_overrides': intentional, 'unintended': unintended, 'standalone_helper_dunders_not_overrides': skipped}


def surface_overrides(cls):
    """Names the credited surface mixin defines that already exist in the retained surface MRO, with the super() chain check."""
    import s108_credit as CREDIT
    mixin = CREDIT.CreditedRecoveryMixin
    retained = set()
    for klass in cls.__mro__:
        if klass is object or klass is mixin or klass is cls:
            continue
        retained |= own_names(klass)['members']
    result = []
    for name in sorted(own_names(mixin)['members'] & retained):
        result.append({'name': name, 'chains_to_super': chains_to_super(mixin, name),
                       'declared': (mixin.__name__, name) in SURFACE_INTENTIONAL})
    return result


def new_classes():
    import s108_credit as CREDIT
    import s108_memory as MEM8
    import s108_run as RUN8
    return {
        's108_memory.MemoryMixin': MEM8.MemoryMixin,
        's108_run.MemoryHookObserver': RUN8.MemoryHookObserver,
        's108_run.MemoryPlainObserver': RUN8.MemoryPlainObserver,
        's108_credit.PauseSource': CREDIT.PauseSource,
        's108_credit.CreditedWait': CREDIT.CreditedWait,
        's108_credit.CreditedRecoveryMixin': CREDIT.CreditedRecoveryMixin,
        's108_run.CreditedDesktopSurface': RUN8.CreditedDesktopSurface,
        's108_run.CreditedHeadlessSurface': RUN8.CreditedHeadlessSurface,
    }


def s107_classes():
    import s107_memory as MEM7
    import s107_run as RUN7
    import s107_b2 as B27
    import s107_machine as MACH7
    return {
        's107_memory.MemoryMixin': MEM7.MemoryMixin,
        's107_run.MemoryHookObserver': RUN7.MemoryHookObserver,
        's107_run.MemoryPlainObserver': RUN7.MemoryPlainObserver,
        's107_b2.B2MemoryObserver': B27.B2MemoryObserver,
        's107_machine.DesktopStatusMixin': MACH7.DesktopStatusMixin,
        's107_machine.HeadlessStatusMixin': MACH7.HeadlessStatusMixin,
        's107_machine.MeasurementMachine': MACH7.MeasurementMachine,
    }


def build_census():
    registry = base_registry()
    new = {name: census_class(cls, registry, label=name) for name, cls in new_classes().items()}
    old = {name: census_class(cls, registry, label=name) for name, cls in s107_classes().items()}
    surface = {name: surface_overrides(cls) for name, cls in new_classes().items() if 'Surface' in name}
    # the new mixin/observer classes must also not shadow a name of the retained surface chain except the declared two
    surface_unintended = {name: [item for item in items if not (item['declared'] and item['chains_to_super'])] for name, items in surface.items()}
    return {
        'schema': 'cellos.s10-8.collision-census/1',
        'question': 'names defined by new mixins/classes vs threading.Thread, Observer, CoordinatedObserver, HookObserver, Machine (whole MROs)',
        'method': 'class __dict__ members + AST `self.<x> =` assignments in every function; base side: dir() plus the same AST over the whole MRO',
        'base_name_counts': {label: len(names) for label, names in registry.items()},
        'new_classes': new,
        'unintended_new': {name: item['unintended'] for name, item in new.items() if item['unintended']},
        'surface_chain_overrides': surface,
        'surface_chain_unintended': {name: items for name, items in surface_unintended.items() if items},
        's107_classes_informational': old,
        's107_collisions_found': {name: item['unintended'] for name, item in old.items() if item['unintended']},
        'empty': not any(item['unintended'] for item in new.values()) and not any(surface_unintended.values()),
    }


if __name__ == '__main__':
    census = build_census()
    if '--write' in sys.argv:
        ENV.HARNESS_EVIDENCE.mkdir(parents=True, exist_ok=True)
        ENV.dump(ENV.HARNESS_EVIDENCE / 'collision-census.json', census)
    print(json.dumps({'empty': census['empty'], 'unintended_new': census['unintended_new'], 'surface_chain_unintended': census['surface_chain_unintended'],
                      's107_collisions_found': census['s107_collisions_found'], 'base_name_counts': census['base_name_counts']}, indent=1))
    raise SystemExit(0 if census['empty'] else 2)
