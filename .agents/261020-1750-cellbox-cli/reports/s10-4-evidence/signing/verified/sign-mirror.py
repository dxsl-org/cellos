#!/usr/bin/env python3
"""Execute approved D4-Sign1 A only; never build, bypass a gate, or deploy."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import sys
import time
import tomllib

sys.dont_write_bytecode = True
W = Path('/tmp/cellbox-s10-4')
H = W / 'baseline-worktree'
R = Path('/home/dmin/cellos')
M = W / 'signing-review-root'
O = W / 'signing-evidence-verified'
S = W / 'b2-reviewed-signed'
ARCHIVE = R / 'reports/s10-4-evidence/signing/verified'
NAMES = ['shell', 'ls', 'cat', 'echo', 'ps', 'kill']
COMMIT = 'f53b654a8882fd5fc036c4ca7a4ff41ce32497a6'
KEY_HASH = '7db0b01f39d179aacf7b03dbd7dd9d71f641598bed40fb9839d3c1e1117e666f'
ENV = dict(os.environ)
ENV['PYTHONDONTWRITEBYTECODE'] = '1'
COMMANDS = []
SUMMARY = {'status': 'STARTED', 'scope': 'D4-Sign1 A historical measurement artifacts only',
           'deployable_image_or_candidate': False, 'runtime_comparator_available': False,
           'feature_equivalent': False, 'commands': COMMANDS}


def require(ok, message):
    if not ok:
        raise RuntimeError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def dump(name, value):
    path = O / name
    require(not path.exists(), f'refuse overwrite evidence: {path}')
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def regular(path, readonly=False, singlelink=True):
    for component in [path, *path.parents]:
        require(not component.is_symlink(), f'symlink prohibited: {component}')
    st = path.lstat()
    require(stat.S_ISREG(st.st_mode) and (not singlelink or st.st_nlink == 1),
            f'not a permitted regular file (singlelink={singlelink}): {path}')
    if readonly:
        require(st.st_mode & 0o222 == 0, f'writable snapshot source: {path}')
    return st


def record(path, readonly=False, singlelink=True):
    st = regular(path, readonly, singlelink)
    return {'bytes': st.st_size, 'sha256': digest(path.read_bytes())}


def run(label, argv, cwd=R, allowed=(0,)):
    p = subprocess.run([str(x) for x in argv], cwd=cwd, env=ENV,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180)
    stdout = p.stdout.decode(errors='replace')
    stderr = p.stderr.decode(errors='replace')
    (O / (label + '.stdout.log')).write_bytes(p.stdout)
    (O / (label + '.stderr.log')).write_bytes(p.stderr)
    COMMANDS.append({'label': label, 'argv': [str(x) for x in argv], 'cwd': str(cwd),
                     'returncode': p.returncode, 'stdout': label + '.stdout.log',
                     'stderr': label + '.stderr.log'})
    require(p.returncode in allowed,
            f'{label} failed with exit {p.returncode}: {stderr.strip()} {stdout.strip()}')
    return p, stdout


def git_bytes(*args):
    return subprocess.run(['git', '-C', str(H), *args], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout


def inventory(root, readonly=False):
    files = {}
    dirs = []
    for path in sorted(root.rglob('*')):
        require(not path.is_symlink(), f'symlink in snapshot: {path}')
        rel = path.relative_to(root).as_posix()
        if path.is_dir():
            dirs.append(rel)
        else:
            files[rel] = record(path, readonly)
    canonical = json.dumps({'directories': dirs, 'files': files}, sort_keys=True,
                           separators=(',', ':')).encode()
    return {'directories': dirs, 'files': files, 'file_count': len(files),
            'membership_content_sha256': digest(canonical)}


def sections(data):
    require(data[:6] == b'\x7fELF\x02\x01', 'expected ELF64 little endian')
    shoff = struct.unpack_from('<Q', data, 40)[0]
    entsize, count, names = struct.unpack_from('<HHH', data, 58)
    require(entsize == 64 and count > 0 and names < count and shoff + count * 64 <= len(data),
            'invalid ELF section table')
    raw = [struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * 64) for i in range(count)]
    strings = raw[names]
    require(strings[1] == 3 and strings[4] + strings[5] <= len(data), 'invalid section strings')
    table = data[strings[4]:strings[4] + strings[5]]
    out = []
    for sh in raw:
        require(sh[0] < len(table), 'invalid section name offset')
        end = table.find(b'\0', sh[0])
        require(end >= 0, 'unterminated section name')
        name = table[sh[0]:end].decode('ascii')
        require(sh[1] == 8 or sh[4] + sh[5] <= len(data), f'section out of bounds: {name}')
        out.append({'name': name, 'type': sh[1], 'flags': sh[2], 'addr': sh[3],
                    'offset': sh[4], 'bytes': sh[5], 'align': sh[8], 'entsize': sh[9]})
    return out


def measure(label, path):
    _, text = run(label, ['size', path], cwd=W)
    nums = text.splitlines()[1].split()
    return {**record(path), 'path': str(path), 'text_bytes': int(nums[0]),
            'data_bytes': int(nums[1]), 'bss_bytes': int(nums[2]), 'size_output': text}


def pin_inputs():
    census = json.loads((W / 'observability/signing-scope-census.json').read_text())
    closure = json.loads((W / 'dependency-closure.json').read_text())
    measurement = json.loads((W / 'b2-measurement.json').read_text())
    audit = json.loads((W / 'lock-audit.json').read_text())
    require(closure['manifests'] == census['local_dependency_manifests'] and
            len(closure['manifests']) == 8, 'local closure/census mismatch')
    require(measurement['dev_public_key_sha256'] == KEY_HASH, 'original key provenance changed')
    for item in measurement['artifacts']:
        require(item['name'] in NAMES, 'unexpected unsigned input')
        got = record(W / 'b2-unsigned' / item['name'])
        require(all(got[k] == item['unsigned'][k] for k in got), 'unsigned input hash drift')
    require(sorted(x['name'] for x in measurement['artifacts']) == sorted(NAMES), 'unsigned membership mismatch')
    for tool in census['source_tools'].values():
        require(record(Path(tool['path'])) == {k: tool[k] for k in ['bytes', 'sha256']},
                f'current tool drift: {tool["path"]}')
    require(git_bytes('rev-parse', 'HEAD').decode().strip() == measurement['baseline_head'],
            'historical worktree HEAD drift')
    old = (W / 'lock-before-reconciliation.lock').read_bytes()
    require(old == git_bytes('show', 'HEAD:Cargo.lock'), 'original lock differs from historical HEAD')
    new = (H / 'Cargo.lock').read_bytes()
    def blocks(raw):
        return re.findall(rb'\[\[package\]\]\r?\n.*?(?=\[\[package\]\]|\Z)', raw, re.S)
    ob, nb = blocks(old), blocks(new)
    require(len(ob) == 407 and all(nb.count(b) == 1 for b in ob), 'original 407 lock blocks changed')
    added = [tomllib.loads(b.decode())['package'][0] for b in nb if b not in ob]
    require(len(nb) == 410 and sorted(p['name'] for p in added) == ['app-chrome', 'files', 'terminal']
            and all('source' not in p for p in added), 'unexpected lock additions')
    metadata = json.loads((W / 'logs-lock/metadata.json').read_text())
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    todo = [i for i, p in packages.items() if p['name'] in ['app-shell', 'app-sys-tools']]
    seen = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        todo.extend(nodes[i]['dependencies'])
    graph = [{'name': packages[i]['name'], 'version': packages[i]['version'],
              'source': packages[i]['source']} for i in sorted(seen)]
    require(graph == audit['measured_graph'], 'retained resolved graph drift')
    require(not {p['name'] for p in added} & {p['name'] for p in graph}, 'lock addition entered graph')
    local = {Path(packages[i]['manifest_path']).relative_to(H).as_posix(): packages[i]['name']
             for i in seen if packages[i]['source'] is None}
    require(local == closure['manifests'], 'graph local closure differs from approved eight directories')
    original_manifest = tomllib.loads(git_bytes('show', 'HEAD:Cargo.toml').decode())
    reconciled_manifest = tomllib.loads((H / 'Cargo.toml').read_text())
    expected = json.loads(json.dumps(original_manifest))
    expected['workspace']['members'].remove('cells/apps/vm-runner')
    require(reconciled_manifest == expected, 'unexpected historical root manifest change')
    for i in seen:
        p = packages[i]
        if p['source'] is None:
            require(tomllib.loads(Path(p['manifest_path']).read_text())['package']['name'] == p['name'],
                    'local package provenance mismatch')
    dump('dependency-proof.json', {'original_lock_blocks': 407, 'byte_identical': True,
         'added': added, 'local_manifests': local, 'resolved_graph': graph,
         'retained_graph_inputs': {str(p): record(p) for p in [W / 'logs-lock/metadata.json',
             W / 'lock-audit.json', W / 'dependency-closure.json', W / 'b2-measurement.json',
             W / 'observability/signing-scope-census.json', W / 'lock-before-reconciliation.lock']}})
    return census, closure, measurement


def create_mirror(census, closure):
    if S.exists():
        for path in S.iterdir():
            require(path.name in NAMES and record(path) == record(W / 'b2-unsigned' / path.name),
                    'refuse reuse of signed or altered staging')
    if M.exists():
        prior = W / 'signing-evidence-completed'
        previous = json.loads((prior / 'mirror-before.json').read_text())
        require(inventory(M, True) == previous, 'retained mirror changed after census-only attempt')
        provenance = json.loads((prior / 'source-provenance.json').read_text())
        for rel, rec in provenance['files'].items():
            require(record(H / rel) == {k: rec[k] for k in ['bytes', 'sha256']},
                    'retained mirror historical input changed: ' + rel)
        dump('source-provenance.json', provenance)
        dump('retained-mirror-origin.json', {'prior_evidence': str(prior),
             'reason': 'initial census compared Path order to string order; membership was identical',
             'previous_membership_content_sha256': previous['membership_content_sha256']})
        return provenance['files']
    for parent in M.parents:
        require(not (parent / '.git').exists(), f'mirror ancestor belongs to Git: {parent}')
    dirs = sorted(str(Path(p).parent) for p in closure['manifests'])
    support = ['scripts/cellos-sign', 'scripts/sign-cell.py', 'scripts/unsafe-allowlist.toml',
               'scripts/cellos_sign']
    raw = git_bytes('ls-files', '-z', '--', *dirs, *support)
    tracked = sorted(p.decode() for p in raw.split(b'\0') if p)
    require(len(tracked) == len(set(tracked)), 'duplicate tracked membership')
    selected = set(tracked) | {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'}
    sources = {p.relative_to(H).as_posix(): {'sha256': x['sha256'], 'bytes': x['bytes']}
               for x in census['cell_source_files'] for p in [Path(x['path'])]}
    actual_sources = {p for p in selected if p.startswith('cells/') and p.endswith('.rs')}
    require(actual_sources == set(sources) and len(sources) == 38, '38-file complete Cell source mismatch')
    provenance = {}
    M.mkdir(mode=0o700)
    for rel in sorted(selected):
        src, dst = H / rel, M / rel
        rec = record(src)
        if rel not in ['Cargo.toml', 'Cargo.lock']:
            require(rec['sha256'] == digest(git_bytes('show', 'HEAD:' + rel)),
                    f'historical tracked source differs from HEAD: {rel}')
        if rel in sources:
            require(rec == sources[rel], f'Cell source differs from approved census: {rel}')
        dst.parent.mkdir(parents=True, exist_ok=True)
        with dst.open('xb') as out:
            out.write(src.read_bytes())
        dst.chmod(0o444)
        require(record(dst, True) == rec, f'copy mismatch: {rel}')
        provenance[rel] = {'historical_path': str(src), **rec,
                           'historical_HEAD_exact': rel not in ['Cargo.toml', 'Cargo.lock']}
    dump('source-provenance.json', {'historical_HEAD': git_bytes('rev-parse', 'HEAD').decode().strip(),
         'root_mode': oct(stat.S_IMODE(M.stat().st_mode)), 'complete_local_crate_directories': dirs,
         'root_manifest_lock': 'exact reconciled historical bytes; authorized vm-runner member removal and three local lock additions only',
         'TCB_boundary': census['tcb_boundary'], 'existing_crate_allowlist_entries': census['existing_crate_allowlist_entries'],
         'existing_file_allowlist_entries': census['existing_file_allowlist_entries'], 'files': provenance})
    return provenance


def scanner_census(census):
    sys.path.insert(0, str(R / 'scripts'))
    from cellos_sign import scan
    found = scan.tracked_sources(M, ['cells'])
    crates = scan.discover_crates(M, ['cells'])
    expected = sorted(Path(x['path']).relative_to(H).as_posix() for x in census['cell_source_files'])
    roots = {c.name: sorted(p.relative_to(M).as_posix() for p in c.roots) for c in crates}
    require(sorted(p.as_posix() for p in found) == expected, 'scanner source coverage mismatch')
    require(set(roots) == {'app-shell', 'app-sys-tools'} and
            len(roots['app-shell']) == 1 and len(roots['app-sys-tools']) == 11,
            'scanner 12-root coverage mismatch')
    dump('scanner-census.json', {'files': expected, 'file_count': len(found), 'roots': roots,
         'root_count': sum(len(x) for x in roots.values()), 'mode': 'unchanged filesystem fallback outside Git'})


def guard_preservation():
    before = json.loads((W / 'canonical/before.json').read_text())
    got = {p: record(Path(p), singlelink=False) for p in before}
    require(all(got[p]['sha256'] == v['sha256'] and got[p]['bytes'] == v['size']
                for p, v in before.items()), 'canonical/candidate differs from preserved original')
    require(got['/tmp/cellbox-s10-3d-resume/cand/candidate.json']['sha256'] ==
            '4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821', 'candidate identity drift')
    return got


def main():
    if (W / 'signing-evidence-complete/scoped-sign-six.stdout.log').exists():
        resume_readback()
        return
    O.mkdir(mode=0o700)
    forbidden = ['CELLBOX_PRIVATE_FIXTURE_SIGNING', 'CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI',
                 'CELLOS_SIGN_SEED_HEX', 'GIT_DIR', 'GIT_WORK_TREE']
    require(not any(os.environ.get(x) for x in forbidden), 'unexpected signing/Git override environment')
    ENV['TMPDIR'] = str(O / 'temporary')
    Path(ENV['TMPDIR']).mkdir(mode=0o700)
    protected = guard_preservation()
    dump('protected-before.json', protected)
    census, closure, measurement = pin_inputs()
    refusal = W / 'logs-reconciled/b2-sign.log'
    original_refusal = record(refusal)
    refusal_text = refusal.read_text()
    require('FAIL: F1' in refusal_text and 'ocel-quickjs' in refusal_text and
            'REFUSED: not signing' in refusal_text, 'missing original refusal evidence')
    shutil.copyfile(refusal, O / 'original-workspace-refusal.log')
    provenance = create_mirror(census, closure)
    before = inventory(M, True)
    dump('mirror-before.json', before)
    scanner_census(census)
    tool_paths = sorted((R / 'scripts/cellos_sign').glob('*.py')) + [R / 'scripts/cellos-sign']
    tool_hashes = {str(p): record(p) for p in tool_paths}
    executables = {}
    for name in ['python3', 'rustc', 'objcopy', 'size', 'readelf', 'git']:
        p = Path(shutil.which(name)).resolve()
        executables[name] = {'path': str(p), 'bytes': p.stat().st_size,
                             'sha256': digest(p.read_bytes())}
    _, compiler_path = run('rustc-installed-path',
                           ['rustup', 'which', '--toolchain', 'nightly-2026-05-01', 'rustc'], cwd=M)
    actual_compiler = Path(compiler_path.strip())
    executables['actual_pinned_rustc'] = {'path': str(actual_compiler), **record(actual_compiler)}
    _, active = run('rustc-active', ['rustc', '-Vv'], cwd=M)
    _, pinned = run('rustc-pinned', ['rustc', '+nightly-2026-05-01', '-Vv'], cwd=M)
    build_rustc = (W / 'logs-reconciled/b2-rustc.log').read_text()
    require(all('commit-hash: ' + COMMIT in x for x in [active, pinned, build_rustc]),
            'F5 active/pinned/actual-build full compiler mismatch')
    _, pubtext = run('existing-dev-pubkey', ['python3', R / 'scripts/sign-cell.py', '--emit-pubkey'])
    pub = bytes(int(x, 16) for x in re.findall(r'0x([0-9a-fA-F]{2})', pubtext))
    require(len(pub) == 32 and digest(pub) == KEY_HASH, 'dev public key hash mismatch')
    dump('tools-compiler-key.json', {'current_CLI_tools': tool_hashes, 'executables': executables,
         'historical_signer': provenance['scripts/sign-cell.py'],
         'historical_allowlist': provenance['scripts/unsafe-allowlist.toml'],
         'full_compiler_commit': COMMIT, 'actual_build_rustc_log': record(W / 'logs-reconciled/b2-rustc.log'),
         'dev_public_key_hex': pub.hex(), 'dev_public_key_sha256': digest(pub),
         'harness': record(Path(__file__).resolve())})
    _, check = run('scoped-check-strict', ['python3', R / 'scripts/cellos-sign', '--repo', M, '--check', '--strict'])
    require('OK:   F1 — 2 crates and 38 files scanned' in check and 'OK:   F5' in check and
            'SKIP: F5' not in check, 'unexpected scoped strict census/F5 result')
    S.mkdir(mode=0o700, exist_ok=True)
    unsigned = {}
    for name in NAMES:
        src, dst = W / 'b2-unsigned' / name, S / name
        unsigned[name] = measure('size-unsigned-' + name, src)
        if not dst.exists():
            with dst.open('xb') as out:
                out.write(src.read_bytes())
        require(record(dst) == record(src), 'fresh staging copy mismatch')
        require(not any(x['name'] == '__ViCell_sig' for x in sections(src.read_bytes())),
                'unsigned original already has a signature section')
        _, comment = run('compiler-comment-' + name, ['readelf', '-p', '.comment', src], cwd=W)
        require('rustc version 1.97.0-nightly (' + COMMIT[:9] + ' 2026-04-30)' in comment,
                'ELF compiler metadata mismatch: ' + name)
    _, signed_log = run('scoped-sign-six', ['python3', R / 'scripts/cellos-sign', '--repo', M,
                         '--sign', *[S / n for n in NAMES]])
    require('signed 6 cell(s) with the dev key after a passing F1/F5 check' in signed_log,
            'missing six-artifact checked signing verdict')
    finish_readback(census, protected, original_refusal, provenance, before, tool_hashes, unsigned, pub)


def finish_readback(census, protected, original_refusal, provenance, before, tool_hashes, unsigned, pub):
    refusal = W / 'logs-reconciled/b2-sign.log'
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    artifacts = []
    for name in NAMES:
        path = S / name
        run('readback-existing-verify-' + name, ['python3', R / 'scripts/sign-cell.py', '--verify', '--in', path])
        data = path.read_bytes()
        sh = sections(data)
        sigs = [x for x in sh if x['name'] == '__ViCell_sig']
        require(len(sigs) == 1 and sigs[0]['type'] == 1 and sigs[0]['bytes'] == 64,
                'not exactly one 64-byte PROGBITS signature section: ' + name)
        off = sigs[0]['offset']
        payload = data[:off] + data[off + 64:]
        Ed25519PublicKey.from_public_bytes(pub).verify(data[off:off + 64], payload)
        original = (W / 'b2-unsigned' / name).read_bytes()
        old_sections = sections(original)
        old_alloc = [x for x in old_sections if x['flags'] & 2]
        new_alloc = [x for x in sh if x['flags'] & 2]
        require(len(old_alloc) == len(new_alloc), 'allocated section membership changed: ' + name)
        metadata_changes = []
        for a, b in zip(old_alloc, new_alloc):
            require(all(a[k] == b[k] for k in ['name', 'flags', 'addr', 'bytes', 'align']),
                    'allocated section VA/flags/size/alignment changed: ' + name)
            differences = {k: {'unsigned': a[k], 'signed': b[k]}
                           for k in ['type', 'entsize'] if a[k] != b[k]}
            if differences:
                metadata_changes.append({'section': a['name'], 'changes': differences})
            if a['type'] != 8:
                require(original[a['offset']:a['offset'] + a['bytes']] == data[b['offset']:b['offset'] + b['bytes']],
                        'allocated section bytes changed: ' + name)
        def program_headers(blob):
            offset = struct.unpack_from('<Q', blob, 32)[0]
            entry_size, count = struct.unpack_from('<HH', blob, 54)
            keys = ['type', 'flags', 'offset', 'vaddr', 'paddr', 'filesz', 'memsz', 'align']
            return [dict(zip(keys, struct.unpack_from('<IIQQQQQQ', blob, offset + i * entry_size)))
                    for i in range(count)]
        old_ph, new_ph = program_headers(original), program_headers(data)
        dump('layout-' + name + '.json', {'unsigned_sections': old_sections, 'signed_sections': sh,
             'unsigned_program_headers': old_ph, 'signed_program_headers': new_ph,
             'program_header_byte_identity': old_ph == new_ph,
             'allocated_bytes_VA_flags_sizes_alignment_unchanged': True,
             'section_type_entry_size_normalization': metadata_changes})
        signed = measure('size-signed-' + name, path)
        require(all(signed[k] == unsigned[name][k] for k in ['text_bytes', 'data_bytes', 'bss_bytes']),
                'text/data/BSS changed during signing: ' + name)
        artifacts.append({'name': name, 'unsigned': unsigned[name], 'signed': signed,
                          'signature_section': sigs[0], 'signature_section_count': 1,
                          'payload_bytes': len(payload), 'payload_sha256': digest(payload),
                          'signature_hex': data[off:off + 64].hex(), 'signature_valid': True,
                          'independent_existing_verifier': True, 'allocated_section_bytes_unchanged': True,
                          'allocated_section_metadata_deltas': metadata_changes,
                          'program_headers_exact': old_ph == new_ph})
    after = inventory(M, True)
    dump('mirror-after.json', after)
    require(after == before, 'mirror membership/content changed during checks/signing')
    historical_after = {rel: record(H / rel) for rel in provenance}
    require(all(historical_after[p] == {k: provenance[p][k] for k in ['bytes', 'sha256']}
                for p in provenance), 'historical source changed during signing')
    require(all(record(Path(p)) == v for p, v in tool_hashes.items()), 'current CLI tools changed')
    require(record(refusal) == original_refusal, 'original workspace refusal changed')
    for a in artifacts:
        require(record(W / 'b2-unsigned' / a['name']) ==
                {k: a['unsigned'][k] for k in ['bytes', 'sha256']}, 'original unsigned changed')
    final_protected = guard_preservation()
    dump('protected-after.json', final_protected)
    require(final_protected == protected, 'protected paths changed during signing')
    dump('historical-after.json', historical_after)
    dump('scope-comparison.json', {'exact_before_after_match': True,
         'file_count': before['file_count'], 'membership_content_sha256': before['membership_content_sha256'],
         'historical_source_unchanged': True, 'current_tools_unchanged': True,
         'original_unsigned_unchanged': True, 'original_refusal_unchanged': True,
         'original_refusal': original_refusal, 'protected_unchanged': True,
         'protected_file_count': len(protected), 'mirror_retained': str(M), 'historical_worktree_retained': str(H)})
    SUMMARY.update({'status': 'COMPLETE_SIX_VALID_SCOPED_DEV_SIGNATURES', 'artifacts': artifacts,
         'six_signed_bytes': sum(a['signed']['bytes'] for a in artifacts),
         'six_unsigned_bytes': sum(a['unsigned']['bytes'] for a in artifacts),
         'suite_signed_bytes': artifacts[0]['signed']['bytes'],
         'workers_signed_bytes': sum(a['signed']['bytes'] for a in artifacts[1:]),
         'dev_public_key_sha256': KEY_HASH, 'original_workspace_F1': 'FAIL preserved; not changed to PASS',
         'scoped_F1': 'PASS: 2 crates, 38 source files, 12 crate roots; existing app-shell exception unchanged',
         'F5': 'PASS verified full active/pinned/actual-build commit ' + COMMIT,
         'TCB_boundary': census['tcb_boundary'], 'builds_or_tests_run': 0})


def resume_readback():
    prior = W / 'signing-evidence-complete'
    prior_summary = json.loads((prior / 'summary.json').read_text())
    successful = [c for c in prior_summary['commands'] if c['label'] == 'scoped-sign-six']
    require(len(successful) == 1 and successful[0]['returncode'] == 0 and
            successful[0]['argv'] == ['python3', str(R / 'scripts/cellos-sign'), '--repo', str(M),
                                      '--sign', *[str(S / n) for n in NAMES]],
            'no exact prior successful checked signing batch')
    require('signed 6 cell(s) with the dev key after a passing F1/F5 check' in
            (prior / 'scoped-sign-six.stdout.log').read_text(), 'prior checked verdict absent')
    O.mkdir(mode=0o700)
    for path in sorted(prior.iterdir()):
        if path.is_file() and path.name != 'summary.json':
            shutil.copyfile(path, O / path.name)
    shutil.copyfile(prior / 'summary.json', O / 'prior-readback-stop.json')
    COMMANDS.extend(prior_summary['commands'])
    census = json.loads((W / 'observability/signing-scope-census.json').read_text())
    protected = json.loads((O / 'protected-before.json').read_text())
    provenance = json.loads((O / 'source-provenance.json').read_text())['files']
    before = json.loads((O / 'mirror-before.json').read_text())
    require(inventory(M, True) == before, 'mirror drift before evidence-only continuation')
    tools = json.loads((O / 'tools-compiler-key.json').read_text())
    tool_hashes = tools['current_CLI_tools']
    require(all(record(Path(p)) == v for p, v in tool_hashes.items()), 'current tools drift before readback')
    pub = bytes.fromhex(tools['dev_public_key_hex'])
    require(digest(pub) == KEY_HASH, 'retained dev key identity mismatch')
    original_refusal = record(O / 'original-workspace-refusal.log')
    unsigned = {name: measure('readback-size-unsigned-' + name, W / 'b2-unsigned' / name) for name in NAMES}
    measurement = json.loads((W / 'b2-measurement.json').read_text())
    for item in measurement['artifacts']:
        require(all(unsigned[item['name']][k] == item['unsigned'][k] for k in ['bytes', 'sha256']),
                'unsigned drift before readback')
    dump('continuation-disposition.json', {
         'authorization': 'Parent S103cResume: finish existing verifier readback for all six, no resign',
         'new_signatures_minted_in_continuation': 0,
         'earlier_harness_stops_preserved': [
             {'path': str(W / 'signing-evidence'), 'cause': 'single-link requirement incorrectly applied to already-hardlinked protected files; corrected only for protected-file hash reads'},
             {'path': str(W / 'signing-evidence-completed'), 'cause': 'Path ordering vs string ordering, identical complete 38-file membership; normalized comparison ordering'},
             {'path': str(W / 'signing-evidence-final'), 'cause': 'rustc ELF .comment uses nine-character hash prefix; full active/pinned/build-log commit remained verified'},
             {'path': str(prior), 'cause': 'extra harness layout identity rejected normal objcopy .init_array metadata normalization; parent authorized recording deltas without resign'}],
         'harness': record(Path(__file__).resolve())})
    finish_readback(census, protected, original_refusal, provenance, before, tool_hashes, unsigned, pub)


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        SUMMARY.update({'status': 'STOP', 'error': str(exc), 'exception': type(exc).__name__})
        print('STOP: ' + str(exc), file=sys.stderr)
    finally:
        if O.exists():
            dump('summary.json', SUMMARY)
            if not ARCHIVE.exists():
                ARCHIVE.mkdir(parents=True)
                for path in sorted(O.iterdir()):
                    if path.is_file():
                        shutil.copyfile(path, ARCHIVE / path.name)
                shutil.copyfile(Path(__file__), ARCHIVE / 'sign-mirror.py')
            else:
                print('STOP: archive already exists; original evidence preserved', file=sys.stderr)
                SUMMARY['status'] = 'STOP'
    print(json.dumps({'status': SUMMARY['status'], 'summary': str(O / 'summary.json'),
                      'archive': str(ARCHIVE)}, sort_keys=True))
    sys.exit(0 if SUMMARY['status'] == 'COMPLETE_SIX_VALID_SCOPED_DEV_SIGNATURES' else 1)
