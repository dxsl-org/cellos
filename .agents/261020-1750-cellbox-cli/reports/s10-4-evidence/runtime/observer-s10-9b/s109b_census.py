#!/usr/bin/env python3
"""S10-9b static collision census (offline, no VM) for the new B2-desktop classes.

Same method as the S10-8/S10-9 census (`s108_census` helpers imported unchanged): for each NEW class, every `__dict__` member and every
`self.<x> =` assignment is compared with the names of the whole MROs of `threading.Thread`, the retained observers/clients/machines/
surfaces and the S10-7 B2 classes.  A collision is accepted only when it is DECLARED here with a reason; declared overrides that
are meant to extend a base must chain with `super().<name>(`.  The S10-7 defect class (`MemoryMixin._stop` vs `Thread._stop`) is
exactly what this census exists to catch.

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109b_census.py [--write]
"""
import json
import sys
import threading
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s108_census as C8  # noqa: E402
import s107_retained as RET  # noqa: E402

# (class, name) -> (reason, must_chain_to_super)
DECLARED = {
    ('B2DesktopObserver', '__init__'): ('constructor extension: stores the input gate, chains to B2MemoryObserver.__init__', True),
    ('B2DesktopObserver', 'gate'): ('the attribute CoordinatedObserver.__init__ would set; that constructor is bypassed (it chains to the Cellbox-specific '
                                    'Observer.__init__ that parses /bin/cellbox), exactly as B2MemoryObserver bypasses Observer.__init__', False),
    ('B2DesktopObserver', '_snapshot'): ('replaces B2MemoryObserver._snapshot: same per-spawn memory record, focus task is `terminal` instead of `shell`', False),
    ('B2DesktopTargets', '__init__'): ('replaces B2Targets.__init__ (headless tool table) with the desktop table; `match` is inherited unchanged', False),
    ('B2DesktopMachine', '__init__'): ('replaces MeasurementMachine.__init__ (verifies the candidate/B2-headless manifest) with the v2 pins; chains to '
                                       'D.Machine.__init__ explicitly like the retained class', False),
    ('B2DesktopMachine', 'stop'): ('replaces MeasurementMachine.stop: same shutdown/receipt code, input proof against the v2 pins', False),
    ('B2DesktopSurface', '__init__'): ('constructor extension: schedule + result list, chains to CoordinatedSurface.__init__', True),
    ('B2DesktopSurface', 'ready'): ('replaces the retained desktop ready: same sequence, expected rows are the B2 terminal source literals', False),
    ('B2DesktopSurface', 'd'): ('the Desk the retained ready() creates (`self.d`), read by the retained sample_idle/snapshot code; same meaning', False),
    ('B2DesktopSurface', 'timestamps'): ('the readiness timestamps the retained ready() records (same keys), read by the runner classification; same meaning', False),
    ('B2DesktopTargets', 'targets'): ('same attribute as B2Targets.targets (list of target records); `match` is inherited and reads it', False),
    ('B2DesktopObserver', '_b2_entry'): ('replaces B2MemoryObserver._b2_entry: same record, focus task is `terminal`; called by the inherited B2MemoryObserver._handle', False),
    ('B2DesktopObserver', 'focus'): ('same attribute the retained observers use for the focus instance (entry base pc); retained snapshot code is replaced here', False),
    ('B2DesktopObserver', 'live'): ('same attribute B2MemoryObserver.__init__ initialises (pc -> live spawn); written by the replaced entry handler', False),
    ('B2DesktopObserver', 'memory_failure'): ('same attribute B2MemoryObserver._b2_entry sets and write_results reads; same meaning', False),
    ('B2DesktopObserver', 'unmatched_entries'): ('same counter B2MemoryObserver._b2_entry increments and write_results reads; same meaning', False),
}
MACHINE_ATTRIBUTES = ('argv', 'gdb_path', 'inputs', 'observed', 'provenance', 'source_disk_sha', 'spec', 'start_ns', 'started', 'work')
DECLARED.update({('B2DesktopMachine', name): ('the attribute the replaced MeasurementMachine/common.Machine constructor assigns; same meaning', False)
                 for name in MACHINE_ATTRIBUTES})
NEW_NAMES = ('B2DesktopTargets', 'B2DesktopObserver', 'B2DesktopMachine', 'B2DesktopSurface')


def base_classes():
    import common as COMMON
    import s107_b2 as B2
    import s107_machine as MACH
    D = COMMON.D
    return {'threading.Thread': threading.Thread, 'observer.Observer': RET.OBS.Observer, 'coordinated.CoordinatedObserver': RET.C.CoordinatedObserver,
            'hook_observer.HookObserver': RET.HO.HookObserver, 'common.Machine': COMMON.Machine, 'phase10_canonical_drive.Machine': D.Machine,
            's107_machine.MeasurementMachine': MACH.MeasurementMachine, 's107_b2.B2MemoryObserver': B2.B2MemoryObserver, 's107_b2.B2Targets': B2.B2Targets,
            'common.Surface': COMMON.Surface, 'coordinated.CoordinatedSurface': RET.C.CoordinatedSurface, 'gui_temporal.TemporalSurface': RET.G.TemporalSurface,
            'hook_surface.HookSurface': RET.HS.HookSurface, 'rsp.RspClient': RET.X.RspClient, 'recurring.RecurringClient': RET.R.RecurringClient,
            'run_atomic.AtomicClient': RET.F.AtomicClient}


def new_classes():
    import s109b_b2d as B2D
    return {f's109b_b2d.{name}': getattr(B2D, name) for name in NEW_NAMES}


def census_class(cls, registry, label):
    found = C8.own_names(cls)
    collisions = {}
    for kind in ('members', 'instance'):
        for name in sorted(found[kind]):
            hit = sorted(base for base, names in registry.items() if name in names)
            if hit:
                collisions.setdefault(name, {'kinds': [], 'bases': hit})['kinds'].append(kind)
    intentional, unintended = [], []
    for name, item in collisions.items():
        declared = DECLARED.get((cls.__name__, name))
        if declared is None:
            unintended.append({'name': name, 'bases': item['bases'], 'kinds': item['kinds'], 'why': 'not declared'})
            continue
        reason, chain = declared
        if chain and not ('members' in item['kinds'] and C8.chains_to_super(cls, name)):
            unintended.append({'name': name, 'bases': item['bases'], 'kinds': item['kinds'], 'why': 'declared to chain but no super().<name>( call'})
            continue
        intentional.append({'name': name, 'bases': item['bases'], 'reason': reason, 'chains_to_super': chain})
    return {'class': label, 'defined': {'members': sorted(found['members']), 'instance_attributes': sorted(found['instance'])},
            'collisions': collisions, 'intentional_overrides': intentional, 'unintended': unintended}


def build_census():
    registry = {label: C8.mro_names(cls) for label, cls in base_classes().items()}
    new = {label: census_class(cls, registry, label) for label, cls in new_classes().items()}
    colliding = {(label.split('.')[-1], name) for label, item in new.items() for name in item['collisions']}
    unused = sorted(key for key in DECLARED if key not in colliding)
    return {'schema': 'cellos.s10-9b.collision-census/1',
            'question': 'names defined by the new S10-9b classes vs threading.Thread, the retained observer/client/machine/surface MROs and the S10-7 B2 classes',
            'method': 'class __dict__ members + AST `self.<x> =` assignments in every function; base side: dir() plus the same AST over the whole MRO',
            'base_name_counts': {label: len(names) for label, names in registry.items()}, 'new_classes': new,
            'unintended_new': {label: item['unintended'] for label, item in new.items() if item['unintended']},
            'declared_but_not_colliding': [list(key) for key in unused],
            'empty': not any(item['unintended'] for item in new.values()) and not unused}


if __name__ == '__main__':
    census = build_census()
    if '--write' in sys.argv:
        ENV.HARNESS_EVIDENCE.mkdir(parents=True, exist_ok=True)
        ENV.dump(ENV.HARNESS_EVIDENCE / 'collision-census.json', census)
    print(json.dumps({'empty': census['empty'], 'unintended_new': census['unintended_new'], 'declared_but_not_colliding': census['declared_but_not_colliding'],
                      'base_name_counts': census['base_name_counts']}, indent=1))
    raise SystemExit(0 if census['empty'] else 2)
