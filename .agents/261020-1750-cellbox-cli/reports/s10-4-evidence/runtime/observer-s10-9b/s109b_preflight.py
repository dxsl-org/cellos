#!/usr/bin/env python3
"""S10-9b go/no-go for the ONE B2 desktop run (offline; never starts QEMU, never claims the run token).

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109b_preflight.py [--write]

Reuses the S10-9 checks unchanged (frozen retained / S10-7 / S10-8 sources, prior archives, S10-7/8 evidence manifests, 546-path
protected census, environment, QEMU identity/input-source binding) and adds what is specific to this run:

  frozen_s109_sources    every executing source frozen by the S10-9 gate (observer-s10-9 + earlier + retained) is byte-identical
  prior_run_evidence_s109 the S10-9 harness / run / b2-desktop-v2 evidence manifests (evidence.sha256) verify, no unlisted file
  gate                   the S10-9b single clean invocation record is PASS and the S10-9b sources are frozen
  collision_census       s109b_census over the new classes is empty
  no_other_qemu          no qemu-system process exists (the rule: no other VM may run; nothing is ever signalled)
  schedule               schedule-b2-desktop.json phases/rows equal the B2 terminal SOURCE literals and the signed terminal ELF carries the banner
  <set>: kernels_isos    hooks kernel/control kernel/ISO/qcow2/kernel_fs.img/init sha256 == the v2 pins == manifest.json
  <set>: hooks_provenance kernel features contain test-hooks, kernel symtab carries test_hook_prime_watermark and the per-CPU symbols,
                         control kernel does not
  <set>: run_token       RUN_CONSUMED-s109-b2-desktop absent and no b2-desktop-run/measurement-run directory
  <set>: private_io      the argv this run builds: unix sockets only under the work dir, no hostfwd/-nic/-net/-vnc/tcp, restricted user netdev,
                         no NVMe data disk, private gdb chardev
  <set>: inputs          signed terminal/shell ELFs pinned to the v2 image table, distinct fingerprints, glyph oracle == GLYPH_SHA, schedule parses
"""
import json
import re
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s109_preflight as PF9  # noqa: E402
import s107_preflight as PF7  # noqa: E402

GATE_SUMMARY = HERE / 'verification-summary-s10-9b.json'
NEW_FROZEN = HERE / 'frozen-sources-s10-9b.sha256'
S109_DIR = HERE.parent / 'observer-s10-9'
S109_FROZEN = S109_DIR / 'frozen-sources-s10-9.sha256'
PRIOR_EVIDENCE_S109 = ('harness', 'measurement-runs/cand-headless', 'measurement-runs/cand-desktop', 'b2-desktop-v2')
SOURCE_OF_TRUTH = Path('/tmp/cellbox-s10-7/worktree/cells/apps/terminal/src/session.rs')

# Shared unchanged.
tree_hashes = PF9.tree_hashes
check_frozen_sources = PF9.check_frozen_sources
check_frozen_s107 = PF9.check_frozen_s107
check_frozen_s108 = PF9.check_frozen_s108
check_prior_archives = PF9.check_prior_archives
check_prior_run_evidence = PF9.check_prior_run_evidence
check_census = PF9.check_census
census_now = PF9.census_now
check_environment = PF9.check_environment
check_qemu_identity = PF9.check_qemu_identity
own_qemu_note = PF9.own_qemu_note
verify_manifest = PF9.verify_manifest
PRIOR_CENSUS = PF9.PRIOR_CENSUS
argv_private_violations = PF7.argv_private_violations


def check_frozen_s109():
    if not S109_FROZEN.exists():
        return {'ok': False, 'detail': {'reason': f'{S109_FROZEN} missing'}}
    return check_frozen_sources(S109_FROZEN)


def verify_manifest_normalized(root):
    """`verify_manifest` semantics, but `./`-prefixed names (sha256sum style, used by the b2-desktop-v2 manifest) are accepted."""
    root = Path(root)
    manifest = root / 'evidence.sha256'
    if not manifest.is_file():
        return {'root': str(root), 'ok': False, 'reason': 'evidence.sha256 missing'}
    listed, bad = {}, []
    for line in manifest.read_text().splitlines():
        digest, name = line.split('  ', 1)
        name = name[2:] if name.startswith('./') else name
        listed[name] = digest
        path = root / name
        if not path.is_file() or ENV.sha256_file(path) != digest:
            bad.append(name)
    unlisted = sorted(str(p.relative_to(root)) for p in root.rglob('*')
                      if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts and str(p.relative_to(root)) not in listed)
    return {'root': str(root), 'ok': not bad and not unlisted, 'listed': len(listed), 'modified_or_missing': bad[:5], 'unlisted': unlisted[:5]}


def check_prior_run_evidence_s109():
    results = [verify_manifest_normalized(ENV.REPORTS / 's10-9-evidence' / name) for name in PRIOR_EVIDENCE_S109]
    return {'ok': all(r['ok'] for r in results), 'detail': {'manifests': len(results), 'results': results}}


def check_gate():
    if not GATE_SUMMARY.exists() or not NEW_FROZEN.exists():
        return {'ok': False, 'detail': {'reason': 'no clean S10-9b gate record yet'}}
    summary = json.loads(GATE_SUMMARY.read_text())
    frozen = json.loads(NEW_FROZEN.read_text())
    drift = sorted(p for p, h in frozen.items() if not Path(p).is_file() or ENV.sha256_file(p) != h)
    clean = (summary.get('status') == 'PASS' and summary.get('single_invocation') is True and summary.get('failures') == 0
             and summary.get('errors') == 0 and summary.get('skipped') == 0 and summary.get('expected_failures') == 0
             and summary.get('unexpected_successes') == 0)
    return {'ok': clean and not drift, 'detail': {'tests_run': summary.get('tests_run'), 'status': summary.get('status'),
                                                  'components': summary.get('components'), 'frozen_files': len(frozen), 'drift': drift}}


def check_collision_census():
    try:
        import s109b_census as CENSUS
        census = CENSUS.build_census()
        return {'ok': bool(census['empty']), 'detail': {'unintended_new': census['unintended_new'], 'new_classes': sorted(census['new_classes'])}}
    except Exception as error:  # noqa: BLE001 - recorded as the no-go reason
        return {'ok': False, 'detail': {'error': f'{type(error).__name__}: {error}'}}


def qemu_pids():
    found = []
    for proc in Path('/proc').iterdir():
        if proc.name.isdigit():
            try:
                if (proc / 'comm').read_text().strip().startswith('qemu-system'):
                    found.append(int(proc.name))
            except OSError:
                continue
    return found


def check_no_other_qemu():
    found = qemu_pids()
    return {'ok': not found, 'detail': {'qemu_pids': found, 'rule': 'no other VM may run; this harness only ever signals the PID it started'}}


# ------------------------------------------------------------------ schedule vs the B2 terminal source
def source_literals(path=SOURCE_OF_TRUTH):
    """{command: [lines]} of the builtin output tables of `TerminalSession::execute_command` plus the banner/prompt literals (from source text)."""
    text = Path(path).read_text()
    unescape = lambda raw: raw.encode().decode('unicode_escape')   # noqa: E731 - the literals only use \n and \'
    banner = re.search(r'grid\.write_str\("((?:[^"\\]|\\.)*Cellos > )"\);', text).group(1)
    tables = {}
    body = text[text.index('pub fn execute_command'):]
    for name in ('ls', 'ps', 'echo'):
        match = re.search(r'"%s" => \{(.*?)\n            \}' % name, body, re.S)
        if match:
            tables[name] = [unescape(m).rstrip('\n') for m in re.findall(r'write_str\("((?:[^"\\]|\\.)*)"\)', match.group(1))]
    not_found = re.search(r'format!\("(shell: command not found: )\{\}\\n", other\)', text)
    return {'banner': unescape(banner), 'tables': tables, 'not_found_prefix': not_found.group(1) if not_found else None}


def expected_command_rows(literals, command):
    name = command.split()[0]
    if name in ('ls', 'ps'):
        return literals['tables'][name]
    if name == 'echo':
        return [' '.join(command.split()[1:])]
    return [literals['not_found_prefix'] + name]


def check_schedule():
    detail, ok = {}, True
    try:
        schedule = ENV.schedule()
        literals = source_literals()
        detail['source'] = str(SOURCE_OF_TRUTH)
        phases = {p['name']: p for p in schedule['phases']}
        detail['banner_matches'] = literals['banner'].split('\n')[:2] == schedule['phases'][2]['expect_rows'][:2] and literals['banner'].endswith('\n\nCellos > ')
        detail['commands'] = {}
        for phase in schedule['phases']:
            if phase['kind'] == 'command':
                got = expected_command_rows(literals, phase['command'])
                detail['commands'][phase['name']] = got == phase['expect_rows']
        elf = (ENV.SPEC.signed / 'terminal').read_bytes()
        detail['terminal_elf_has_banner'] = all(part.encode() in elf for part in ('=== CellOS GUI Terminal (x86_64) ===', "Type 'help' for commands. Single-reader stream lease active.",
                                                                                    'shell: command not found: ', 'bin/  apps/  data/  etc/  welcome.md'))
        detail['snapshots'] = list(ENV.schedule_snapshots(schedule))
        detail['phases'] = list(phases)
        ok = detail['banner_matches'] and all(detail['commands'].values()) and detail['terminal_elf_has_banner'] and len(detail['commands']) == 4
    except Exception as error:  # noqa: BLE001
        detail['error'] = f'{type(error).__name__}: {error}'
        ok = False
    return {'ok': bool(ok), 'detail': detail}


# ------------------------------------------------------------------ per-set checks
def check_set_files(name=ENV.SET_NAME):
    record = ENV.verify_set_files(ENV.SPEC)
    return {'ok': record['all_match'], 'detail': record}


def check_hooks_provenance(name=ENV.SET_NAME):
    spec = ENV.SPEC
    detail = {}
    try:
        provenance = ENV.provenance(spec)
        detail['kernel_features'] = provenance['kernel_features']
        detail['kernel_sha256'] = provenance['kernel_sha256']
        detail['features_contain_test_hooks'] = 'test-hooks' in provenance['kernel_features']
        detail['kernel_sha_matches_file'] = ENV.sha256_file(spec.kernel) == provenance['kernel_sha256']
        detail['kernel_has_test_hook_symbol'] = ENV.kernel_has_test_hooks(spec.kernel)
        detail['control_kernel_has_test_hook_symbol'] = ENV.kernel_has_test_hooks(spec.control_kernel)
        import s107_memory as MEM  # noqa: E402
        symbols = MEM.KernelSymbols(spec.kernel)
        detail['cpu_locals_va'], detail['tss_tables_va'] = hex(symbols.cpu_locals), hex(symbols.tables)
        ok = (detail['features_contain_test_hooks'] and detail['kernel_sha_matches_file'] and detail['kernel_has_test_hook_symbol']
              and not detail['control_kernel_has_test_hook_symbol'])
    except Exception as error:  # noqa: BLE001 - recorded as the no-go reason
        detail['error'] = f'{type(error).__name__}: {error}'
        ok = False
    return {'ok': ok, 'detail': detail}


def check_run_token(name=ENV.SET_NAME):
    token = ENV.token(name)
    return {'ok': not token.exists() and not ENV.RUN_EVIDENCE.exists(),
            'detail': {'token': str(token), 'consumed': token.exists(), 'evidence_dir_exists': ENV.RUN_EVIDENCE.exists()}}


def run_argv(scratch, *, observed=True):
    """The exact argv a run builds (retained desktop D.Machine argv, no data disk), built in `scratch`, nothing started or verified."""
    import s107_machine as MACH  # noqa: E402
    return MACH.dry_argv(ENV.SPEC, scratch, observed=observed, with_data_disk=False)


def check_private_io(name=ENV.SET_NAME):
    with tempfile.TemporaryDirectory(prefix='s109b-argv-') as scratch:
        argv = run_argv(scratch)
        argv += ['-D', f'{scratch}/evidence/qemu-trace.log', '-msg', 'timestamp=on']
        bad = argv_private_violations(argv, scratch)
        if any('nvme' in item or 'p10fx' in item for item in argv):
            bad.append('data disk present')
        if not any(item.startswith('file=') and item.endswith('desktop-test-hooks-cellos-x86_64-desktop.iso,media=cdrom,readonly=on,format=raw') for item in argv):
            bad.append('hooks ISO is not the read-only cdrom')
        mode = oct(Path(scratch, 'sock').stat().st_mode & 0o777)
    return {'ok': not bad and mode == '0o700', 'detail': {'violations': bad, 'sock_dir_mode': mode,
                                                        'argv_template': [a.replace(scratch, '$WORK') for a in argv]}}


def check_inputs(name=ENV.SET_NAME):
    spec = ENV.SPEC
    detail, ok = {}, True
    try:
        import s109b_b2d as B2D  # noqa: E402
        targets = B2D.B2DesktopTargets()
        detail['b2_tasks'] = {t['name']: {'sha256': t['sha256'][:16], 'windows': len(t['windows']), 'heap_arena': t['heap_arena']} for t in targets.targets}
        detail['static_verification'] = {'ok': ENV.static_verification()['ok'], 'checks': ENV.static_verification()['checks'] if 'checks' in ENV.static_verification() else None,
                                         'sha256': ENV.sha256_file(ENV.IMAGE_ROOT / 'static-verification.json')}
        ok &= detail['static_verification']['ok'] is True and detail['static_verification']['sha256'] == ENV.PINNED_STATIC_VERIFICATION
        detail['glyph_oracle'] = {'path': str(spec.glyph_bin), 'exists': spec.glyph_bin.is_file(),
                                  'sha256': ENV.sha256_file(spec.glyph_bin) if spec.glyph_bin.is_file() else None, 'expected': ENV.GLYPH_SHA}
        ok &= bool(detail['glyph_oracle']['exists'] and detail['glyph_oracle']['sha256'] == ENV.GLYPH_SHA)
        detail['schedule'] = {'path': str(ENV.SCHEDULE), 'exists': ENV.SCHEDULE.is_file()}
        ok &= detail['schedule']['exists']
        ok &= all(t['heap_arena'] is not None for t in targets.targets)
    except Exception as error:  # noqa: BLE001
        detail['error'] = f'{type(error).__name__}: {error}'
        ok = False
    return {'ok': bool(ok), 'detail': detail}


def preflight(names=ENV.SET_NAMES):
    checks = {'frozen_sources': check_frozen_sources(), 'frozen_s107_sources': check_frozen_s107(), 'frozen_s108_sources': check_frozen_s108(),
              'frozen_s109_sources': check_frozen_s109(), 'prior_archives': check_prior_archives(), 'prior_run_evidence': check_prior_run_evidence(),
              'prior_run_evidence_s109': check_prior_run_evidence_s109(), 'protected_census': check_census(), 'gate': check_gate(),
              'collision_census': check_collision_census(), 'environment': check_environment(), 'no_other_qemu': check_no_other_qemu(),
              'schedule': check_schedule()}
    checks['qemu_identity'], health, binding = check_qemu_identity()
    sets = {}
    for name in names:
        sets[name] = {'kernels_isos': check_set_files(name), 'hooks_provenance': check_hooks_provenance(name),
                      'run_token': check_run_token(name), 'private_io': check_private_io(name), 'inputs': check_inputs(name)}
    global_ok = all(item['ok'] for item in checks.values())
    result = {'schema': 'cellos.s10-9b.go-no-go/1', 'checks': checks, 'sets': sets,
              'go_by_set': {name: global_ok and all(item['ok'] for item in group.values()) for name, group in sets.items()},
              'informational': own_qemu_note()}
    result['go'] = all(result['go_by_set'].values())
    return result, health, binding


if __name__ == '__main__':
    outcome, _health, _binding = preflight()
    if '--write' in sys.argv:
        (HERE / 'go-no-go.json').write_text(json.dumps(outcome, indent=2) + '\n')
    print(json.dumps({'go': outcome['go'], 'go_by_set': outcome['go_by_set'],
                      'failed': {k: v['detail'] for k, v in outcome['checks'].items() if not v['ok']},
                      'failed_sets': {n: {k: v['detail'] for k, v in group.items() if not v['ok']} for n, group in outcome['sets'].items()}}, indent=1)[:6000])
    raise SystemExit(0 if outcome['go'] else 2)
