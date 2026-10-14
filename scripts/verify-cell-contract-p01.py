#!/usr/bin/env python3
"""Verify P01 document structure, not approval, runtime correctness or qualification.

No dependencies or runtime/toolchain activation. Default is read-only. Negative
controls mutate loaded documents in memory and invoke the same validator.
--require-approved fails on pending exact deltas; it must not be used to mint them.
--refresh-census refreshes the explicitly lexical JSON inventory, never approvals.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = '.agents/261011-1200-cell-contract-authority'
CONTROLLER = PACKAGE + '/plan.md'
REGISTER = '.agents/cell-contract-0024-locks.md'
TOKEN = 'CELL-CONTRACT-0024'
SPEC = 'docs/specs/25-cell-kernel-authority-contract.md'
CENSUS = PACKAGE + '/consumer-census-source.json'
DELTAS = [f'D{i:02}' for i in range(1, 13)] + [f'C{i:02}' for i in range(1, 12)]
DELTA_MANIFEST = PACKAGE + '/exact-deltas/manifest.json'
ACS = [f'AC-{i:02}' for i in range(1, 51)]
EXPORT_ROOTS = ('libs/api/src/', 'libs/types/src/', 'libs/ostd/src/',
                'kernel/src/task/', 'kernel/src/cell/', 'kernel/src/loader/')
EXPORT_EXTRA = ('kernel/src/task.rs', 'kernel/src/cell.rs', 'kernel/src/loader.rs')
SOURCE_ROOTS = ('libs/', 'kernel/', 'cells/', 'hal/', 'tests/',
                'third_party/mlibc/sysdeps/', 'tools/', 'scripts/', 'docs/')
SUFFIXES = {'.rs', '.c', '.h', '.cpp', '.hpp', '.zig', '.S', '.s', '.py', '.sh',
            '.ps1', '.toml', '.ld', '.md', '.json', '.cfg', '.yaml', '.yml', '.nix', '.meson'}
LINK = re.compile(r'(?<!!)\[[^\]\n]*\]\(([^\s)]+)(?:\s+"[^"]*")?\)')
PUB = re.compile(r'\bpub\s+(?:(?:unsafe|async|const)\s+)*'
                 r'(?:extern\s+"[^"]+"\s+)?'
                 r'(fn|struct|enum|trait|type|const|static|mod|use)\s+'
                 r'([A-Za-z_][A-Za-z_0-9]*)')
WORD = re.compile(r'\b[A-Za-z_][A-Za-z_0-9]*\b')


def front(text: str) -> str:
    match = re.match(r'\A---\r?\n(.*?)\r?\n---(?:\r?\n|$)', text, re.S)
    return match.group(1) if match else ''


def scalar(text: str, key: str) -> str:
    match = re.search(r'^' + re.escape(key) + r':\s*([^\n]*)$', front(text), re.M)
    return match.group(1).strip().strip('\"\'') if match else ''


def sequence(text: str, key: str) -> list[str]:
    """Only the recorded flat scalar-list metadata grammar; reject other YAML."""
    header = front(text)
    match = re.search(r'^' + re.escape(key) + r':[ \t]*(.*)$', header, re.M)
    if not match:
        raise ValueError(f'missing {key}')
    inline = match.group(1).strip()
    if inline:
        if not inline.startswith('[') or not inline.endswith(']'):
            raise ValueError(f'{key}: expected flat list')
        return [x.strip().strip('\"\'') for x in inline[1:-1].split(',') if x.strip()]
    result = []
    for line in header[match.end():].splitlines():
        if not line.strip():
            continue
        entry = re.fullmatch(r'[ \t]+-[ \t]+(.+)', line)
        if not entry:
            break
        result.append(entry.group(1).strip().strip('\"\''))
    return result


def section(text: str, heading: str) -> str:
    match = re.search(r'^## ' + re.escape(heading) + r'\s*\n(.*?)(?=^## |\Z)', text, re.M | re.S)
    return match.group(1) if match else ''


def rows(text: str, pattern: str) -> list[list[str]]:
    result = []
    for line in text.splitlines():
        if line.startswith('|'):
            columns = [x.strip() for x in re.split(r'(?<!\\)\|', line)[1:-1]]
            if columns and re.fullmatch(pattern, columns[0]):
                result.append(columns)
    return result


def phase_ids(text: str) -> set[int]:
    return {int(x) for x in re.findall(r'\bP(\d{2})(?:-[A-E])?\b', text)}


def ac_ids(text: str) -> set[str]:
    result = {f'AC-{int(x):02}' for x in re.findall(r'AC-(\d{2})', text)}
    for a, b in re.findall(r'AC-(\d{2})\s*(?:\.\.|[–-])\s*(?:AC-)?(\d{2})', text):
        result.update(f'AC-{x:02}' for x in range(int(a), int(b) + 1))
    for group in re.findall(r'AC-\d{2}((?:/\d{2})+)', text):
        result.update(f'AC-{int(x):02}' for x in group.split('/') if x)
    return result


def outside_fences(text: str) -> str:
    return re.sub(r'^```[^\n]*\n.*?^```\s*$', '', text, flags=re.M | re.S)


def targets(path: str, text: str) -> list[tuple[str, str]]:
    result = []
    for match in LINK.finditer(outside_fences(text)):
        raw = unquote(match.group(1)).strip('<>')
        if re.match(r'^[a-zA-Z][a-zA-Z0-9+.-]*:', raw):
            continue
        name = raw.split('#', 1)[0].split('?', 1)[0]
        target = (ROOT / path).parent / name if name else ROOT / path
        try:
            result.append((target.resolve().relative_to(ROOT).as_posix(), raw))
        except ValueError:
            result.append(('!outside-repository', raw))
    return result


def anchors(text: str) -> set[str]:
    result, duplicates = set(), {}
    for title in re.findall(r'^#{1,6}\s+(.+?)\s*#*\s*$', outside_fences(text), re.M):
        slug = re.sub(r'[^\w\- ]', '', title.lower()).replace(' ', '-')
        count = duplicates.get(slug, 0)
        duplicates[slug] = count + 1
        result.add(slug if count == 0 else f'{slug}-{count}')
    result.update(re.findall(r'<a\s+(?:name|id)=["\']([^"\']+)["\']', text))
    return result


def load() -> dict[str, str]:
    paths = set((ROOT / PACKAGE).rglob('*.md'))
    paths.add(ROOT / REGISTER)
    paths.add(ROOT / SPEC)
    paths.add(ROOT / CONTROLLER)
    register = (ROOT / REGISTER).read_text()
    for path in sequence(register, 'blocks'):
        target = ROOT / path
        if target.is_file():
            paths.add(target)
            paths.update(target.parent.glob('phase-*.md'))
    return {p.relative_to(ROOT).as_posix(): p.read_bytes().decode('utf-8') for p in sorted(paths)}


def validate(docs: dict[str, str], require_approved: bool = False) -> dict:
    errors: list[dict] = []
    pending: list[dict] = []
    counts: dict[str, int] = {}

    def fail(code: str, path: str, detail: str) -> None:
        errors.append({'code': code, 'path': path, 'detail': detail})

    def get_list(path: str, key: str) -> list[str]:
        try:
            return sequence(docs.get(path, ''), key)
        except ValueError as exc:
            fail('METADATA', path, str(exc))
            return []

    register = get_list(REGISTER, 'blocks')
    controller = get_list(CONTROLLER, 'blocks')
    counts['locks'] = len(register)
    classified = []
    for line in docs.get(REGISTER, '').splitlines():
        if line.startswith('|'):
            first_cell = line.split('|', 2)[1]
            classified.extend(p for p, _ in targets(REGISTER, first_cell) if p.endswith('/plan.md'))
    counts['recorded_legacy_inventory'] = len(classified)
    if len(classified) != 161 or len(set(classified)) != 161:
        fail('LEGACY_INVENTORY', REGISTER, 'historical 161-entry classified denominator missing/duplicate')
    if not set(register) <= set(classified):
        fail('LOCK_INVENTORY', REGISTER, 'locked owner absent from classified inventory')
    if len(register) != 38 or len(set(register)) != 38 or set(register) != set(controller):
        fail('LOCK_SET', REGISTER, 'expected 38 unique reciprocal controller/register locks')
    if scalar(docs.get(REGISTER, ''), 'controller') != CONTROLLER:
        fail('REGISTER_OWNER', REGISTER, 'controller metadata mismatch')
    if set(get_list(CONTROLLER, 'blockedBy')) & set(register):
        fail('BACKWARD_EDGE', CONTROLLER, 'controller waits on a locked predecessor')
    ledger = docs.get(PACKAGE + '/migration-ledger.md', '')
    adoption = rows(ledger, r'L\d{2} .*')
    counts['adoptions'] = len(adoption)
    expected_l = [f'L{i:02}' for i in range(1, 39)]
    if [r[0][:3] for r in adoption] != expected_l:
        fail('ADOPTION_SET', PACKAGE + '/migration-ledger.md', 'L01..L38 not unique/ordered')
    adopted_targets = set()
    for row in adoption:
        links = targets(PACKAGE + '/migration-ledger.md', '| ' + ' | '.join(row) + ' |')
        adopted_targets.update(p for p, _ in links if p.endswith('/plan.md'))
        if len(row) != 6 or not phase_ids(row[3]) or not row[5].startswith('N'):
            fail('ADOPTION_OWNER', PACKAGE + '/migration-ledger.md', row[0])
    if adopted_targets != set(register):
        fail('ADOPTION_LINKS', PACKAGE + '/migration-ledger.md', 'former owners do not match 38 locks')
    direct_count = 0
    for path in register:
        text = docs.get(path, '')
        if CONTROLLER not in get_list(path, 'blockedBy'):
            fail('RECIPROCAL_LOCK', path, 'missing blockedBy controller edge')
        if scalar(text, 'executionLock') != TOKEN or scalar(text, 'executionStatus') != 'blocked' or not scalar(text, 'lockScope'):
            fail('LOCK_METADATA', path, 'token/status/scope missing or changed')
        notices = '\n'.join(line for line in text.splitlines() if line.startswith('>'))
        if TOKEN not in notices or CONTROLLER not in {p for p, _ in targets(path, notices)}:
            fail('DIRECT_NOTICE', path, 'visible scoped notice does not link controller')
        for target, raw in targets(path, text):
            if Path(target).name.startswith('phase-') and target.endswith('.md'):
                direct_count += 1
                if target not in docs and not (ROOT / target).is_file():
                    fail('DIRECT_PHASE_LINK', path, raw)
                # External historical phase links retain their own owner. A link
                # never grants execution; same-directory phases inherit this lock.
    counts['locked_direct_phase_links'] = direct_count
    phases: dict[int, str] = {}
    graph: dict[int, list[int]] = {}
    for path, text in docs.items():
        if str(Path(path).parent) == PACKAGE and Path(path).name.startswith('phase-'):
            try:
                number = int(scalar(text, 'phase'))
                deps = [int(x) for x in get_list(path, 'dependencies')]
            except ValueError:
                fail('PHASE_METADATA', path, 'noninteger phase/dependency')
                continue
            if number in phases:
                fail('PHASE_DUPLICATE', path, str(number))
            phases[number] = path
            graph[number] = deps
    counts['phases'] = len(phases)
    if set(phases) != set(range(1, 11)):
        fail('PHASE_SET', CONTROLLER, 'expected P01..P10')
    controller_links = {p for p, _ in targets(CONTROLLER, section(docs.get(CONTROLLER, ''), 'Phases and dependencies'))}
    handoff_path = PACKAGE + '/ownership-handoffs.md'
    handoff = docs.get(handoff_path, '')
    phase_rows = rows(section(handoff, 'Phase integration ownership and dependencies'), r'\[P\d{2}\]\([^)]+\)')
    for number, path in phases.items():
        if path not in controller_links:
            fail('PHASE_LINK', CONTROLLER, f'no phase-table link to {path}')
        if any(d not in phases for d in graph[number]) or len(set(graph[number])) != len(graph[number]):
            fail('DEPENDENCY', path, 'missing/duplicate dependency')
        matches = [r for r in phase_rows if f'[P{number:02}]' in r[0]]
        if len(matches) != 1:
            fail('PHASE_HANDOFF', handoff_path, f'P{number:02} missing/duplicate')
        elif number != 10 and phase_ids(matches[0][1]) != set(graph[number]):
            fail('HANDOFF_DEPENDENCY', handoff_path, f'P{number:02} reciprocal dependencies differ')
        elif number == 10 and matches[0][1] != 'P02–P09':
            fail('HANDOFF_DEPENDENCY', handoff_path, 'P10 range differs')
    visiting, visited = set(), set()

    def visit(node: int) -> None:
        if node in visiting:
            fail('CYCLE', phases.get(node, CONTROLLER), f'cycle at P{node:02}')
            return
        if node in visited:
            return
        visiting.add(node)
        for dep in graph.get(node, []):
            visit(dep)
        visiting.remove(node)
        visited.add(node)

    for node in graph:
        visit(node)
    canonical = rows(section(docs.get(SPEC, ''), '9. Acceptance ledger'), r'AC-\d{2}')
    owner_rows = rows(section(handoff, 'All 50 acceptance owners and required proofs'), r'AC-\d{2}')
    approvals = rows(section(handoff, 'AC approval and evidence ledger'), r'AC-\d{2}')
    for name, table in [('canonical', canonical), ('owners', owner_rows), ('approvals', approvals)]:
        counts['ac_' + name] = len(table)
        if [r[0] for r in table] != ACS:
            fail('AC_SET', handoff_path if name != 'canonical' else SPEC, name + ' must contain ordered AC-01..AC-50')
    owner_map = {r[0]: r for r in owner_rows}
    approval_map = {r[0]: r for r in approvals}
    for row in canonical:
        owners = phase_ids(row[-1])
        observed = owner_map.get(row[0], [])
        if not observed or not owners <= phase_ids(observed[1]):
            fail('AC_OWNER', handoff_path, row[0] + ' missing canonical owner')
        for number in owners:
            if number not in phases or row[0] not in ac_ids(docs.get(phases.get(number, ''), '')):
                fail('AC_RECIPROCAL', phases.get(number, handoff_path), row[0] + ' absent from implementing phase')
        approval = approval_map.get(row[0], [])
        if len(approval) != 7 or not owners <= phase_ids(approval[2]):
            fail('AC_APPROVAL_FIELDS', handoff_path, row[0])
        elif approval[3] != 'Spec25-v1.3; architecture-only' or any(x != 'pending' for x in approval[4:]):
            fail('AC_EVIDENCE_CEILING', handoff_path, row[0] + ' must not promote architecture into checkpoints/runtime')
        if approval:
            for delta in re.findall(r'\b[DC]\d{2}\b', approval[1]):
                if delta not in DELTAS:
                    fail('AC_DELTA', handoff_path, row[0] + ': ' + delta)
    delta_path = PACKAGE + '/abi-deltas.md'
    delta_rows = rows(section(docs.get(delta_path, ''), 'Per-delta approval records'), r'[DC]\d{2}')
    counts['delta_approval_rows'] = len(delta_rows)
    if [r[0] for r in delta_rows] != DELTAS:
        fail('DELTA_SET', delta_path, 'expected D01..D12,C01..C11 exactly once')
    for row in delta_rows:
        if len(row) != 8 or not phase_ids(row[1]) or any(x != 'pending' for x in row[2:7]) or row[-1] != 'blocked':
            fail('DELTA_APPROVAL_FIELDS', delta_path, row[0] + ': owner/revision/digest/checkpoints/evidence/gate')
        else:
            pending.append({'delta': row[0], 'owner': row[1], 'candidate': row[2], 'checkpoint1': row[4], 'checkpoint2': row[5]})
    if require_approved and pending:
        fail('APPROVAL_PENDING', delta_path, f'{len(pending)} exact deltas lack checkpoint approval; P01 completion refused')
    link_count = 0
    for path, text in docs.items():
        if not (Path(path).parent == Path(PACKAGE) or Path(PACKAGE) in Path(path).parents) and path != REGISTER:
            continue
        for target, raw in targets(path, text):
            link_count += 1
            if target not in docs and not (ROOT / target).exists():
                fail('DOCUMENT_LINK', path, raw)
            elif '#' in raw and target.endswith('.md'):
                fragment = raw.split('#', 1)[1]
                target_text = docs[target] if target in docs else (ROOT / target).read_text()
                if fragment and fragment not in anchors(target_text):
                    fail('DOCUMENT_ANCHOR', path, raw)
    counts['local_links'] = link_count
    return {'errors': errors, 'counts': counts, 'pending_exact_deltas': pending,
            'scope': 'document consistency only; no ABI approval, lock release, runtime or P01 completion'}


def validate_delta_packages(manifest: dict, docs: dict[str, str],
                            census: dict, census_digest: str) -> dict:
    """Authenticate preparation bytes; never interpret a digest as consent."""
    if not isinstance(manifest, dict):
        return {'errors': [{'code': 'PACKAGE_SCHEMA', 'path': DELTA_MANIFEST,
                            'detail': 'manifest must be an object'}],
                'counts': {'delta_packages': 0, 'package_source_inputs': 0}}
    errors = []
    def fail(code: str, path: str, detail: str) -> None:
        errors.append({'code': code, 'path': path, 'detail': detail})
    if manifest.get('format') != 'cell-contract-delta-packages-v1':
        fail('PACKAGE_SCHEMA', DELTA_MANIFEST, 'unsupported preparation manifest')
    if manifest.get('scope') != 'preparation-only; not CP1 candidate approval':
        fail('PACKAGE_SCOPE', DELTA_MANIFEST, 'preparation cannot promote approval')
    if manifest.get('source_census_sha256') != census_digest:
        fail('PACKAGE_CENSUS_DRIFT', DELTA_MANIFEST, 'preparation source denominator differs from pinned census')
    for key in ('source_head', 'source_tree'):
        if not re.fullmatch(r'[0-9a-f]{40}', str(manifest.get(key, ''))):
            fail('PACKAGE_BASELINE', DELTA_MANIFEST, 'missing exact ' + key)
    packages = manifest.get('packages', [])
    if not isinstance(packages, list):
        packages = []
    if [row.get('delta') for row in packages if isinstance(row, dict)] != DELTAS:
        fail('PACKAGE_SET', DELTA_MANIFEST, 'expected ordered D01..D12,C01..C11 once')
    for row in packages:
        if not isinstance(row, dict):
            fail('PACKAGE_SCHEMA', DELTA_MANIFEST, 'package record is not an object')
            continue
        delta = row.get('delta')
        path = PACKAGE + '/exact-deltas/' + str(delta) + '.md'
        if row.get('path') != path or not re.fullmatch(r'r[1-9]\d*', str(row.get('revision', ''))):
            fail('PACKAGE_IDENTITY', DELTA_MANIFEST, str(delta) + ': path/revision')
        if not phase_ids(str(row.get('owner', ''))):
            fail('PACKAGE_OWNER', path, 'missing integrating phase')
        if any(row.get(key) != value for key, value in (
            ('disposition', 'blocked-inputs'), ('checkpoint1', 'pending'),
            ('checkpoint2', 'pending'), ('release', 'blocked'))):
            fail('PACKAGE_APPROVAL', path, 'preparation must preserve pending checkpoints and blocked release')
        if not isinstance(row.get('blockers'), list) or not row['blockers']:
            fail('PACKAGE_BLOCKERS', path, 'blocked package must identify required inputs')
        text = docs.get(path)
        if text is None:
            fail('PACKAGE_MISSING', path, 'package document absent')
        elif hashlib.sha256(text.encode()).hexdigest() != row.get('sha256'):
            fail('PACKAGE_DIGEST', path, 'preparation bytes differ from pinned revision')
        if text is not None and not section(text, 'Blocking inputs'):
            fail('PACKAGE_BLOCKERS', path, 'missing explicit Blocking inputs section')
    inputs = manifest.get('source_inputs', [])
    if not isinstance(inputs, list) or not inputs:
        fail('PACKAGE_SOURCE', DELTA_MANIFEST, 'missing working-source byte inventory')
        inputs = []
    seen = set()
    for item in inputs:
        if not isinstance(item, dict) or not isinstance(item.get('path'), str):
            fail('PACKAGE_SOURCE', DELTA_MANIFEST, 'invalid source input')
            continue
        path = item['path']
        if path in seen or Path(path).is_absolute() or '..' in Path(path).parts:
            fail('PACKAGE_SOURCE', DELTA_MANIFEST, 'duplicate or nonlocal source path: ' + path)
            continue
        seen.add(path)
        source = ROOT / path
        if not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest() != item.get('sha256'):
            fail('PACKAGE_SOURCE_DRIFT', path, 'pinned source bytes differ; revise preparation, not approval')
    expected = {item['path'] for item in census['consumer_inputs']}
    if seen != expected:
        fail('PACKAGE_SOURCE_SET', DELTA_MANIFEST, 'working-source denominator must equal the pinned lexical consumer inventory')
    return {'errors': errors, 'counts': {'delta_packages': len(packages), 'package_source_inputs': len(inputs)}}


def refresh_census() -> None:
    paths = sorted(set(subprocess.check_output(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT
    ).decode().split('\0')) - {''})
    export_files = [p for p in paths if p.endswith('.rs') and (p.startswith(EXPORT_ROOTS) or p in EXPORT_EXTRA)]
    exports, export_inputs = [], []
    for path in export_files:
        data = (ROOT / path).read_bytes()
        lines = data.decode('utf-8').splitlines()
        export_inputs.append({'path': path, 'sha256': hashlib.sha256(data).hexdigest()})
        for number, line in enumerate(lines, 1):
            for match in PUB.finditer(line):
                exports.append({'path': path, 'line': number, 'kind': match[1], 'symbol': match[2], 'text': line.strip()})
            if '#[macro_export]' in line:
                for index in range(number, min(number + 12, len(lines))):
                    macro = re.search(r'\bmacro_rules!\s*([A-Za-z_][A-Za-z_0-9]*)', lines[index])
                    if macro:
                        exports.append({'path': path, 'line': index + 1, 'kind': 'macro_export', 'symbol': macro[1], 'text': lines[index].strip()})
                        break
    symbols = {r['symbol'] for r in exports if r['kind'] != 'use'}
    inputs, occurrences, fallbacks, excluded = [], [], [], []
    for path in paths:
        if Path(path).suffix not in SUFFIXES or not (path.startswith(SOURCE_ROOTS) or '/' not in path):
            continue
        file = ROOT / path
        if not file.is_file():
            excluded.append({'path': path, 'reason': 'listed input absent/not regular'})
            continue
        data = file.read_bytes()
        entry = {'path': path, 'sha256': hashlib.sha256(data).hexdigest()}
        try:
            text = data.decode('utf-8')
        except UnicodeDecodeError:
            text = data.decode('latin-1')
            entry['decoding'] = 'lossless Latin-1 byte view; actual source encoding not asserted'
            fallbacks.append({'path': path, 'reason': 'UTF-8 decode failed; included with lossless Latin-1 byte view, not excluded'})
        inputs.append(entry)
        refs: dict[str, list[int]] = {}
        for number, line in enumerate(text.splitlines(), 1):
            for symbol in sorted(set(WORD.findall(line)) & symbols):
                refs.setdefault(symbol, []).append(number)
        if refs:
            occurrences.append({'path': path, 'symbols': dict(sorted(refs.items()))})
    payload = {'format': 'cell-contract-lexical-census-v1',
               'scope': 'repository-local textual inventory, not semantic/LSP/build closure',
               'lsp': 'xd://lsp unavailable at original collection; fallback lexical source scanning',
               'enumeration': 'git ls-files --cached --others --exclude-standard -z; sorted unique paths; no semantic reference or ABI approval claim',
               'export_roots': list(EXPORT_ROOTS) + list(EXPORT_EXTRA),
               'consumer_roots': list(SOURCE_ROOTS) + ['root-level text files'],
               'consumer_suffixes': sorted(SUFFIXES), 'export_inputs': export_inputs,
               'consumer_inputs': inputs, 'exports': exports, 'occurrences': occurrences,
               'decoding_fallbacks': fallbacks, 'excluded': excluded,
               'unresolved': ['macro expansion/generated reexports', 'cfg/target/linker/provider visibility and C numeric aliases',
                              'ignored vendor/toolchain/archive/image inputs', 'out-of-tree/external consumers',
                              'candidate revision and exact proposal approvals', 'lexical method names are not resolved calls']}
    (ROOT / CENSUS).write_text(json.dumps(payload, sort_keys=True, separators=(',', ':')) + '\n')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--negative-controls', action='store_true')
    parser.add_argument('--require-approved', action='store_true')
    parser.add_argument('--refresh-census', action='store_true')
    args = parser.parse_args()
    if args.refresh_census:
        refresh_census()
        print(json.dumps({'recorded': CENSUS, 'scope': 'lexical inventory only; no approvals changed'}, sort_keys=True))
        return 0
    try:
        docs = load()
        report = validate(docs, args.require_approved)
        manifest = json.loads((ROOT / DELTA_MANIFEST).read_text())
        census_bytes = (ROOT / CENSUS).read_bytes()
        census = json.loads(census_bytes)
        census_digest = hashlib.sha256(census_bytes).hexdigest()
        packages = validate_delta_packages(manifest, docs, census, census_digest)
        report['errors'].extend(packages['errors'])
        report['counts'].update(packages['counts'])
        if census.get('format') != 'cell-contract-lexical-census-v1' or not census.get('exports') or not census.get('occurrences'):
            report['errors'].append({'code': 'CENSUS_SCHEMA', 'path': CENSUS, 'detail': 'missing declaration/occurrence inventory'})
        for item in census['export_inputs']:
            path = ROOT / item['path']
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != item['sha256']:
                report['errors'].append({'code': 'CENSUS_SOURCE_DRIFT', 'path': item['path'], 'detail': 'export snapshot differs; collect source inventory again, not approval'})
        report['counts']['lexical_exports'] = len(census['exports'])
        report['counts']['lexical_consumer_inputs'] = len(census['consumer_inputs'])
        if args.negative_controls:
            controls = []
            # Remove an actual controller table link, not a validator assertion.
            broken = copy.deepcopy(docs)
            table = section(broken[CONTROLLER], 'Phases and dependencies')
            match = next((m for m in LINK.finditer(table) if m[1].startswith('phase-02-')), None)
            if match is None:
                raise ValueError('negative control prerequisite: actual P02 phase link missing')
            broken[CONTROLLER] = broken[CONTROLLER].replace(match[0], '[removed phase link]', 1)
            found = validate(broken)['errors']
            controls.append({'mutation': 'remove actual P02 controller link', 'detected': any(e['code'] == 'PHASE_LINK' for e in found),
                             'failures': [e for e in found if e['code'] == 'PHASE_LINK']})
            # Remove one real blockedBy relation, leaving its visible notice intact.
            broken = copy.deepcopy(docs)
            locked = sequence(docs[REGISTER], 'blocks')[0]
            header = front(broken[locked])
            block = re.search(r'^blockedBy:[^\n]*(?:\n[ \t]+-[^\n]*)*', header, re.M)
            if block is None or CONTROLLER not in sequence(broken[locked], 'blockedBy'):
                raise ValueError('negative control prerequisite: actual reciprocal edge missing')
            remaining = [p for p in sequence(broken[locked], 'blockedBy') if p != CONTROLLER]
            replacement = 'blockedBy: [' + ', '.join(remaining) + ']'
            changed = header[:block.start()] + replacement + header[block.end():]
            broken[locked] = broken[locked].replace(header, changed, 1)
            found = validate(broken)['errors']
            controls.append({'mutation': 'remove actual reciprocal blockedBy edge', 'path': locked,
                             'detected': any(e['code'] == 'RECIPROCAL_LOCK' and e['path'] == locked for e in found),
                             'failures': [e for e in found if e['code'] == 'RECIPROCAL_LOCK' and e['path'] == locked]})
            for mutation, code in (
                ('alter pinned package bytes', 'PACKAGE_DIGEST'),
                ('promote preparation checkpoint', 'PACKAGE_APPROVAL'),
                ('alter pinned source digest', 'PACKAGE_SOURCE_DRIFT'),
                ('omit pinned source input', 'PACKAGE_SOURCE_SET'),
                ('omit preparation package', 'PACKAGE_SET'),
            ):
                broken_manifest, broken_docs = copy.deepcopy(manifest), copy.deepcopy(docs)
                if code == 'PACKAGE_DIGEST':
                    path = broken_manifest['packages'][0]['path']
                    broken_docs[path] += '\nUnrecorded revision.\n'
                elif code == 'PACKAGE_APPROVAL':
                    broken_manifest['packages'][0]['checkpoint1'] = 'approved'
                elif code == 'PACKAGE_SOURCE_SET':
                    broken_manifest['source_inputs'].pop()
                elif code == 'PACKAGE_SET':
                    broken_manifest['packages'].pop()
                else:
                    broken_manifest['source_inputs'][0]['sha256'] = '0' * 64
                found = validate_delta_packages(broken_manifest, broken_docs, census, census_digest)['errors']
                controls.append({'mutation': mutation,
                                 'detected': any(e['code'] == code for e in found),
                                 'failures': [e for e in found if e['code'] == code]})
            report['negative_controls'] = controls
            for control in controls:
                if not control['detected']:
                    report['errors'].append({'code': 'NEGATIVE_CONTROL', 'path': control.get('path', CONTROLLER), 'detail': control['mutation'] + ' was not detected'})
        print(json.dumps(report, indent=2, sort_keys=True))
        return 1 if report['errors'] else 0
    except (OSError, ValueError, KeyError) as exc:
        print(json.dumps({'errors': [{'code': 'INPUT', 'detail': str(exc)}]}, indent=2), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
