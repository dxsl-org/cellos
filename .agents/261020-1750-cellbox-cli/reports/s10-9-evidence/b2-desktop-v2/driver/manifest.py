#!/usr/bin/env python3
"""manifest.json (sha256 of every artifact) + result.json for the B2 desktop v2 set."""
import hashlib, json, os, subprocess, sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
import b2v2 as B

OUT, WT, W = B.OUT, B.WT, B.W
env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')


def out(argv):
    return subprocess.run([str(a) for a in argv], cwd=WT, env=env, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT).stdout.decode(errors='replace')


artifacts = {}
for p in sorted(OUT.rglob('*')):
    if p.is_file() and p.name not in ('manifest.json', 'result.json', 'evidence.sha256'):
        artifacts[p.relative_to(OUT).as_posix()] = {'bytes': p.stat().st_size, 'sha256': B.sha(p)}

sign_tools = ['scripts/cellos-sign', 'scripts/sign-cell.py', 'scripts/unsafe-allowlist.toml',
              'scripts/generate-boot-config.py', 'tools/mkfat32.py', 'tools/inspect_fat.py',
              'scripts/x86/make-iso-ci.sh', 'scripts/x86/limine.conf', 'scripts/package-desktop-x86.py',
              'scripts/package-ocel-profile.py', 'scripts/sign-policy.py', 'run-desktop.sh', 'Cargo.toml',
              'Cargo.lock', 'rust-toolchain.toml', 'limine/limine-8.7.0/bin/limine']
sign_tools += sorted(p.relative_to(WT).as_posix() for p in (WT / 'scripts/cellos_sign').glob('*.py'))
src = {rel: B.sha(WT / rel) for rel in sign_tools}
verify = json.loads((OUT / 'static-verification.json').read_text())
cmds = json.loads(B.CMDS.read_text())
sv_ok = verify['ok']
boot_sets = {
    'non-hooks': {'qcow2': 'desktop-cellos-x86_64-desktop.qcow2', 'iso': 'desktop-cellos-x86_64-desktop.iso',
                  'kernel': 'desktop-kernel.elf', 'kernel_fs': 'desktop-kernel_fs.img', 'init': 'desktop-init'},
    'test-hooks': {'qcow2': 'desktop-test-hooks-cellos-x86_64-desktop.qcow2',
                   'iso': 'desktop-test-hooks-cellos-x86_64-desktop.iso',
                   'kernel': 'desktop-test-hooks-kernel.elf', 'kernel_fs': 'desktop-kernel_fs.img',
                   'init': 'desktop-init'},
}
for s in boot_sets.values():
    s['sha256'] = {k: artifacts[v]['sha256'] for k, v in s.items() if k != 'sha256'}
manifest = {
    'commit': B.HEAD,
    'purpose': 'S10-9.3 B2 desktop v2: services rebuilt from source at the commit instead of tracked historical bytes '
               '(reverses S10-7.1 deviation 2)',
    'non_pristine': True,
    'worktree': str(WT),
    'authorized_deltas_applied': ['b2-manifest.patch', 'b2-lock.patch',
                                  'F1 scoped-signing exception (scoped mirror root; whole-workspace F1 still refuses)'],
    'deltas_beyond_authorized': [],
    'rebuilt_from_source_and_signed': {l: d for l, _p, _e, _b, d in B.BUILDS} | {b: d for b, d in B.SYS_TOOLS},
    'kept_tracked_historical': B.TRACKED_KEPT + ['/data/*', '/etc/cellos/{services,system}.toml', 'init (embedded)'],
    'static_verification': {'ok': sv_ok, 'checks': len(verify['checks']),
                            'passed': sum(1 for c in verify['checks'] if c['ok']), 'file': 'static-verification.json'},
    'boot_sets': boot_sets,
    'tool_versions': {'rustc': out(['rustc', '+nightly-2026-05-01', '-Vv']), 'cargo': out(['cargo', '+nightly-2026-05-01', '-V']),
                      'qemu': out(['qemu-system-x86_64', '--version']).splitlines()[0],
                      'xorriso': out(['xorriso', '-version']).splitlines()[0] if out(['xorriso', '-version']) else None,
                      'python': sys.version.split()[0]},
    'build_tool_sha256': src,
    'commands': [{k: c[k] for k in ('label', 'argv', 'cwd', 'returncode', 'log_sha256', 'seconds', 'env_overrides')} for c in cmds],
    'artifacts': artifacts,
}
(OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
print('manifest written', len(artifacts), 'artifacts; static ok =', sv_ok)
