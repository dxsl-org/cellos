#!/usr/bin/env python3
"""S10-9.0b offline harvest of every `[stack-baseline]` serial line of the retained logs (no VM, read-only).

The test-hooks measurement kernels (S10-7.2) emit, once, at the first sample after tick 1500, one line per priming task and kind:

    [ WARN] [stack-baseline] name=<cell> phase=boot kind=<kernel|user> used_bytes=N used_pages=N alloc_bytes=N usable_bytes=N baseline=authoritative-input

`phase=boot` is the kernel's literal label.  The line is emitted ONCE per boot, shortly after first light, NOT at the end of a workload:
the values are boot-phase watermarks of the sentinel-primed stacks and are labelled boot-phase only.  The emission can nevertheless
fall after some workload has started (the S10-7.4 cand-headless line block follows the first prompt and the partially typed finite-stream
command), which is why every record keeps its serial line number and the serial lines that immediately precede the block.

Table key: `<profile>/<kernel_build>@<sha12>/<cell>/<phase>/<kind>#<occurrence>`; occurrence is the 1-based order of identical
(cell, phase, kind) lines inside ONE log (desktop `cellbox` is spawned twice: occurrence 1 = autoload-background, occurrence 2 = the
foreground-terminal instance; the memory observer's instance roles confirm that order, see the cross-checks).

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109_stack_baseline.py [--write]

`--write` stores reports/s10-9-evidence/s10-9.0b-stack-baseline/stack-baseline-table.json (new directory; fails if it exists).
"""
import json
import re
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_env as ENV  # noqa: E402
import s107_env as E7  # noqa: E402

LINE = re.compile(r'^\[\s*(?P<level>[A-Z]+)\]\s+\[stack-baseline\]\s+(?P<fields>name=\S+(?:\s+\w+=\S+)*)\s*$')
EMIT = r'^\[ *[A-Z]+\] \[stack-baseline\] name='        # grep -E: real emission lines only (not JSON/markdown/source mentions)
REQUIRED = ('name', 'phase', 'kind', 'used_bytes', 'used_pages', 'alloc_bytes', 'usable_bytes', 'baseline')
INT_FIELDS = ('used_bytes', 'used_pages', 'alloc_bytes', 'usable_bytes')

S107_RUNS = ENV.REPORTS / 's10-7-evidence/measurement-runs'
S108_RUNS = ENV.REPORTS / 's10-8-evidence/measurement-runs'
S107_SMOKE = ENV.REPORTS / 's10-7-evidence/boot-smoke'
S107_B2_DIAG = ENV.REPORTS / 's10-7-evidence/b2-desktop-diagnostic'


def sources():
    """Every retained serial log of a measurement kernel boot: (source label, set, path, kind of log)."""
    found = []
    for tag, root, names in (('s10-7.4', S107_RUNS, ('cand-headless', 'cand-desktop', 'b2-headless')), ('s10-8', S108_RUNS, ('cand-headless', 'cand-desktop'))):
        for name in names:
            for log in ('serial-measurement.log', 'serial-measurement.raw.log'):
                found.append({'source': f'{tag}/{name}/{log}', 'run': f'{tag}/{name}', 'set': name, 'path': root / name / 'evidence' / log,
                              'log': 'cooked' if log.endswith('measurement.log') else 'raw'})
    for name in ('cand-headless', 'cand-desktop', 'b2-headless', 'b2-desktop'):
        found.append({'source': f's10-7-boot-smoke/{name}/serial.log', 'run': f's10-7-boot-smoke/{name}', 'set': name,
                      'path': S107_SMOKE / name / 'serial.log', 'log': 'smoke'})
    found.append({'source': 's10-7-b2-desktop-diagnostic/serial.log', 'run': 's10-7-b2-desktop-diagnostic', 'set': 'b2-desktop',
                  'path': S107_B2_DIAG / 'serial.log', 'log': 'diagnostic (non-hooks boot)'})
    return found


def kernel_of(set_name):
    """(profile, kernel_build, sha256) of the S10-7.2 measurement kernel manifest entry (hash source of truth)."""
    entry = json.loads((E7.TH_KERNELS / 'result.json').read_text())['sets'][set_name]
    profile = 'headless' if set_name.endswith('headless') else 'desktop'
    return profile, f'{set_name}-test-hooks', entry['test_hooks_kernel_sha256'], entry['control_kernel_non_hooks_sha256']


def parse_line(text):
    match = LINE.match(text)
    if not match:
        return None
    fields = dict(item.split('=', 1) for item in match['fields'].split())
    if any(name not in fields for name in REQUIRED):
        return None
    record = {'level': match['level'], **{name: fields[name] for name in REQUIRED}}
    try:
        for name in INT_FIELDS:
            record[name] = int(record[name])
    except ValueError:
        return None                       # reported by harvest_log as a malformed emission line, never a silent 0
    return record


def harvest_log(path):
    """[(line_no, record, preceding serial lines)] for every parseable `[stack-baseline]` line + the lines that did not parse."""
    records, malformed = [], []
    lines = Path(path).read_bytes().decode('utf-8', errors='replace').splitlines()
    previous = []
    for number, text in enumerate(lines, 1):
        if '[stack-baseline]' in text and re.match(r'^\[\s*[A-Z]+\]\s+\[stack-baseline\]\s+name=', text):
            record = parse_line(text)
            if record is None:
                malformed.append({'line': number, 'text': text[:200]})
            else:
                records.append((number, record, [] if records and records[-1][0] == number - 1 else list(previous[-3:])))
        elif '[stack-baseline]' not in text:
            previous.append(text[:160])
            previous[:] = previous[-3:]
    return {'lines_total': len(lines), 'records': records, 'malformed': malformed}


def rel(path):
    path = Path(path)
    try:
        return str(path.relative_to(ENV.REPORTS))
    except ValueError:
        return str(path)


def key_for(profile, build, sha, cell, phase, kind, occurrence):
    return f'{profile}/{build}@{sha[:12]}/{cell}/{phase}/{kind}#{occurrence}'


def build_table(items=None):
    items = items if items is not None else sources()
    entries, per_source, notes = {}, [], []
    for item in items:
        path = Path(item['path'])
        profile, build, sha, control_sha = kernel_of(item['set'])
        if not path.is_file():
            per_source.append({**{k: v for k, v in item.items() if k != 'path'}, 'path': rel(path), 'present': False, 'lines': 0})
            continue
        harvested = harvest_log(path)
        seen = {}
        for number, record, before in harvested['records']:
            triple = (record['name'], record['phase'], record['kind'])
            seen[triple] = seen.get(triple, 0) + 1
            key = key_for(profile, build, sha, record['name'], record['phase'], record['kind'], seen[triple])
            entry = entries.setdefault(key, {
                'key': key, 'profile': profile, 'kernel_build': build, 'kernel_sha256': sha, 'cell': record['name'], 'phase': record['phase'],
                'kind': record['kind'], 'occurrence': seen[triple], 'scope': 'boot-phase only (kernel label phase=boot; emitted once at the first sample after tick 1500)',
                'observations': []})
            entry['observations'].append({'source': item['source'], 'run': item['run'], 'log': item['log'], 'line': number,
                                          'used_bytes': record['used_bytes'], 'used_pages': record['used_pages'], 'alloc_bytes': record['alloc_bytes'],
                                          'usable_bytes': record['usable_bytes'], 'baseline': record['baseline'], 'level': record['level'],
                                          'serial_lines_before_block': before})
        per_source.append({**{k: v for k, v in item.items() if k != 'path'}, 'path': rel(path), 'present': True,
                           'lines_total': harvested['lines_total'], 'lines': len(harvested['records']), 'malformed': harvested['malformed'],
                           'kernel_sha256': sha, 'control_kernel_non_hooks_sha256': control_sha})
    for entry in entries.values():
        observations = entry['observations']
        used = sorted({o['used_bytes'] for o in observations})
        usable = sorted({o['usable_bytes'] for o in observations})
        cooked = [o for o in observations if o['log'] == 'cooked']
        entry['used_bytes'] = used[0] if len(used) == 1 else None
        entry['usable_bytes'] = usable[0] if len(usable) == 1 else None
        entry['used_bytes_all_observed'] = used
        entry['usable_bytes_all_observed'] = usable
        entry['used_pct_of_usable'] = round(100.0 * entry['used_bytes'] / entry['usable_bytes'], 2) if entry['used_bytes'] is not None and entry['usable_bytes'] else None
        entry['independent_runs'] = sorted({o['run'] for o in cooked})
        entry['value_equal_across_all_logs'] = len(used) == 1
    return {'entries': entries, 'per_source': per_source}


def cooked_vs_raw(per_source):
    """The `.raw.log` and `.log` of a run must carry the identical baseline block (they are two renderings of one serial stream)."""
    result = {}
    for item in per_source:
        if item.get('log') in ('cooked', 'raw') and item['present']:
            result.setdefault(item['run'], {})[item['log']] = item['lines']
    return {run: {'cooked': v.get('cooked'), 'raw': v.get('raw'), 'equal': v.get('cooked') == v.get('raw')} for run, v in sorted(result.items())}


def completeness_scan():
    """Independent `grep -r` over the whole reports tree for real emission lines; every hit file must be a harvested source."""
    harvested = {str(Path(item['path']).resolve()) for item in sources()}
    out = subprocess.run(['grep', '-rlaE', EMIT, str(ENV.REPORTS)], capture_output=True, text=True, check=False)
    hits = sorted(set(out.stdout.split()))
    return {'method': f'grep -rlaE {EMIT!r} over reports/ (anchored at line start: JSON/markdown/source mentions do not match)',
            'files_with_emission_lines': [str(Path(h).relative_to(ENV.REPORTS)) for h in hits],
            'files_not_harvested': [str(Path(h).relative_to(ENV.REPORTS)) for h in hits if str(Path(h).resolve()) not in harvested],
            'complete': all(str(Path(h).resolve()) in harvested for h in hits)}


def cross_checks(table):
    """Informational: does the boot-phase watermark of the focus cellbox equal the depth the memory observer measured later?"""
    checks = []
    for run, summary_path in (('s10-7.4/cand-headless', S107_RUNS / 'cand-headless/summary.json'), ('s10-7.4/cand-desktop', S107_RUNS / 'cand-desktop/summary.json'),
                              ('s10-8/cand-desktop', S108_RUNS / 'cand-desktop/summary.json')):
        if not summary_path.is_file():
            continue
        depths = [r['stack_depth_bytes'] for r in json.loads(summary_path.read_text()).get('memory_table', []) if r.get('stack_depth_bytes')]
        if not depths:
            checks.append({'run': run, 'note': 'no memory snapshot depths'})
            continue
        set_name = run.split('/')[1]
        profile, build, sha, _ = kernel_of(set_name)
        focus_occurrence = 2 if profile == 'desktop' else 1
        user = table['entries'].get(key_for(profile, build, sha, 'cellbox', 'boot', 'user', focus_occurrence))
        kernel = table['entries'].get(key_for(profile, build, sha, 'cellbox', 'boot', 'kernel', focus_occurrence))
        user_values = sorted({d['user'] for d in depths})
        kernel_values = sorted({d['kernel'] for d in depths})
        checks.append({'run': run, 'focus_cellbox_occurrence': focus_occurrence,
                       'baseline_user_used_bytes': user['used_bytes'] if user else None, 'memory_observer_user_depths_over_run': user_values,
                       'baseline_user_equals_one_of_the_measured_depths': bool(user and user['used_bytes'] in user_values),
                       'baseline_kernel_used_bytes': kernel['used_bytes'] if kernel else None, 'memory_observer_kernel_depths_over_run': kernel_values,
                       'baseline_kernel_equals_measured_kernel_depth': bool(kernel and kernel_values == [kernel['used_bytes']])})
    return checks


def build_result():
    table = build_table()
    entries = list(table['entries'].values())
    by_profile = {}
    for entry in entries:
        by_profile.setdefault(entry['profile'], set()).add(entry['cell'])
    differing = [e['key'] for e in entries if not e['value_equal_across_all_logs']]
    return {
        'schema': 'cellos.s10-9.stack-baseline-table/1',
        'scope': 'BOOT-PHASE ONLY: sentinel-primed stack watermarks (lower bound) printed once by the test-hooks measurement kernels at the first sample after tick 1500; '
                 'NOT end-of-workload peaks; measurement kernels are not the promoted kernel; QEMU/TCG guest numbers',
        'key_format': '<profile>/<kernel_build>@<kernel sha12>/<cell>/<phase>/<kind>#<occurrence in one log>',
        'fields_per_entry': ['used_bytes', 'usable_bytes', 'used_pages', 'alloc_bytes (in observations)', 'observations[] with source/line/serial context'],
        'sources': table['per_source'],
        'cooked_vs_raw_log_agreement': cooked_vs_raw(table['per_source']),
        'completeness': completeness_scan(),
        'entry_count': len(entries),
        'observation_count': sum(len(e['observations']) for e in entries),
        'entries_with_differing_values_across_logs': differing,
        'cells_by_profile': {profile: sorted(cells) for profile, cells in sorted(by_profile.items())},
        'cross_checks': cross_checks(table),
        'entries': sorted(entries, key=lambda e: (e['profile'], e['kernel_build'], e['cell'], e['kind'], e['occurrence'])),
    }


def summary_rows(result):
    """Compact (key, used_bytes, usable_bytes, runs) rows for display."""
    return [(e['key'], e['used_bytes'] if e['used_bytes'] is not None else e['used_bytes_all_observed'], e['usable_bytes'], len(e['independent_runs']))
            for e in result['entries']]


def main(argv):
    result = build_result()
    for row in summary_rows(result):
        print(row)
    print(json.dumps({k: result[k] for k in ('entry_count', 'observation_count', 'entries_with_differing_values_across_logs', 'cooked_vs_raw_log_agreement', 'cross_checks')}, indent=1)[:3500])
    print(json.dumps(result['completeness'], indent=1)[:2500])
    if '--write' in argv:
        destination = ENV.STACK_BASELINE_EVIDENCE
        destination.mkdir(mode=0o700, parents=True, exist_ok=False)
        ENV.dump(destination / 'stack-baseline-table.json', result)
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
