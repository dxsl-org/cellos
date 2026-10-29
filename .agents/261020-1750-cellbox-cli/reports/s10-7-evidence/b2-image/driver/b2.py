#!/usr/bin/env python3
"""S10-7.1 option (a) driver: replay of commit 3471028bc's own x86 image routes.

Mirrors, step for step:
  headless: scripts/build-x86_64-cells.ps1 (cells, signing, init, mkfat32, policy) ->
            kernel build (default features) -> scripts/x86/make-iso-ci.sh
  desktop : run-desktop.sh --rebuild block (ocel + net, sign, package-ocel-profile.py,
            kernel with desktop features, make-iso-ci.sh, limine bios-install, qcow2)
Deviations (all recorded in manifest): --locked --offline on every cargo call, signing
through a scoped mirror root (D7-4 F1 exception), no pwsh/run-desktop.sh execution.
Fail-fast: first failing command writes STOP and exits; no retries.
"""
import hashlib, json, os, shutil, subprocess, sys, time
from pathlib import Path

sys.dont_write_bytecode = True
W = Path('/tmp/cellbox-s10-7')
WT = W / 'worktree'
LOGS = W / 'logs'
OUT = W / 'b2-image'
STATE = W / 'state'
TARGET = WT / 'target'
REL = TARGET / 'x86_64-unknown-none/release'
EMB = WT / 'kernel/src/embedded-x86_64'
PATCHES = Path('/home/dmin/cellos/.agents/261020-1750-cellbox-cli/reports/s10-4-evidence')
HEAD = '3471028bc8b4db14275553b4740c17e24fa4f32b'
STOP_FILE = STATE / 'stop.json'
CMDS = STATE / 'commands.json'

KERNEL_RUSTFLAGS = '-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static'
DESKTOP_KERNEL_FEATURES = 'dev-policy-key,dev-signing-key,dev-weak-rng,signing-required,native-domains,qemu-dev-https,large-cell-va'

STRIP_ENV = ['CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS', 'CELLBOX_PRIVATE_FIXTURE_SIGNING',
             'CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI', 'CELLOS_SIGN_SEED_HEX',
             'CELLOS_PHASE02_CELL_PUBLIC_KEY', 'CELLOS_CONFIG_DIR', 'CELLOS_INIT_FEATURES',
             'EMBEDDED_OVERRIDE', 'RUSTUP_TOOLCHAIN', 'X86_KERNEL', 'X86_ISO_ROOT', 'XORRISO',
             'OBJCOPY', 'GIT_DIR', 'GIT_WORK_TREE', 'CARGO_TARGET_DIR']


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def base_env():
    env = dict(os.environ)
    for k in STRIP_ENV:
        env.pop(k, None)
    env['PYTHONDONTWRITEBYTECODE'] = '1'
    env['CARGO_NET_OFFLINE'] = 'true'
    env['CARGO_TARGET_DIR'] = str(TARGET)
    inc = WT / 'third_party/freestanding-include'
    env['CC_x86_64_unknown_none'] = 'cc'
    env['CFLAGS_x86_64_unknown_none'] = ('-ffreestanding -fno-stack-protector -mno-red-zone -mno-sse '
                                         f'-mno-mmx -DLFS_NO_INTRINSICS -I{inc}')
    env['BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none'] = '--target=x86_64-linux-gnu'
    return env


def cells_env():
    e = base_env()
    e['RUSTFLAGS'] = '-C relocation-model=pic'
    return e


def kernel_env():
    e = base_env()
    e['CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS'] = KERNEL_RUSTFLAGS
    return e


def load_cmds():
    return json.loads(CMDS.read_text()) if CMDS.exists() else []


def run(label, argv, env=None, cwd=WT, overrides=None, timeout=3300):
    LOGS.mkdir(exist_ok=True)
    STATE.mkdir(exist_ok=True)
    if STOP_FILE.exists():
        raise SystemExit('STOP already recorded; refusing to run ' + label)
    cmds = load_cmds()
    log = LOGS / (label + '.log')
    assert not log.exists(), f'log exists (one attempt per step): {log}'
    t0 = time.time()
    argv = [str(a) for a in argv]
    with log.open('wb') as f:
        p = subprocess.run(argv, cwd=str(cwd), env=env or base_env(), stdout=f,
                           stderr=subprocess.STDOUT, timeout=timeout)
    rec = {'label': label, 'argv': argv, 'cwd': str(cwd), 'returncode': p.returncode,
           'log': str(log), 'log_sha256': sha(log), 'start': t0, 'seconds': round(time.time() - t0, 1),
           'env_overrides': overrides or {}}
    cmds.append(rec)
    CMDS.write_text(json.dumps(cmds, indent=2) + '\n')
    if p.returncode:
        STOP_FILE.write_text(json.dumps({'status': 'STOPPED', 'failing_label': label,
                                         'argv': argv, 'cwd': str(cwd), 'returncode': p.returncode,
                                         'log': str(log)}, indent=2) + '\n')
        print(f'STOP: {label} exit {p.returncode}; see {log}', file=sys.stderr)
        raise SystemExit(1)
    return rec


def cargo(label, args, env, overrides):
    return run(label, ['cargo', '+nightly-2026-05-01', *args], env=env, overrides=overrides)


CELLS_OV = {'RUSTFLAGS': '-C relocation-model=pic', 'CARGO_NET_OFFLINE': 'true',
            'CARGO_TARGET_DIR': str(TARGET)}
KERNEL_OV = {'CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS': KERNEL_RUSTFLAGS, 'CARGO_NET_OFFLINE': 'true',
             'CARGO_TARGET_DIR': str(TARGET)}
BS = ['-Z', 'build-std=core,alloc']
TGT = ['--target', 'x86_64-unknown-none']


def ccargo(label, pkgs, extra=()):
    args = ['build', '--release', '--locked', '--offline']
    for p in pkgs:
        args += ['-p', p]
    args += list(extra) + TGT + BS
    return cargo(label, args, cells_env(), CELLS_OV)


# --------------------------------------------------------------------------- setup
def phase_setup():
    OUT.mkdir(exist_ok=True)
    for sub in ('unsigned', 'signed', 'patches', 'logs'):
        (OUT / sub).mkdir(exist_ok=True)
    run('00-git-head', ['git', 'rev-parse', 'HEAD'])
    head = (LOGS / '00-git-head.log').read_text().strip()
    assert head == HEAD, head
    run('01-git-status-pristine', ['git', 'status', '--porcelain', '--untracked-files=no'])
    assert (LOGS / '01-git-status-pristine.log').read_text().strip() == '', 'worktree not pristine'
    for name in ('b2-manifest.patch', 'b2-lock.patch'):
        shutil.copyfile(PATCHES / name, OUT / 'patches' / name)
        run('02-apply-check-' + name, ['git', 'apply', '--check', '--', OUT / 'patches' / name])
        run('03-apply-' + name, ['git', 'apply', '--', OUT / 'patches' / name])
    run('04-rustc', ['rustc', '+nightly-2026-05-01', '-Vv'])
    run('05-cargo', ['cargo', '+nightly-2026-05-01', '-V'])
    run('06-rustc-active', ['rustc', '-Vv'])
    run('07-limine-build', ['make', '-C', 'limine/limine-8.7.0/bin', 'limine'])
    # patched-tree diff (the complete non-pristine delta)
    p = subprocess.run(['git', 'diff', '--binary'], cwd=WT, stdout=subprocess.PIPE, check=True)
    (OUT / 'patches' / 'all-deltas-in-worktree.patch').write_bytes(p.stdout)


# ------------------------------------------------------------- scoped signing mirror
def closure_dirs(pkgs):
    """Local-path dependency closure of pkgs from cargo metadata (locked, offline)."""
    meta = W / 'state' / 'metadata.json'
    if not meta.exists():
        rec = None
        with meta.open('wb') as f:
            p = subprocess.run(['cargo', '+nightly-2026-05-01', 'metadata', '--format-version', '1',
                                '--locked', '--offline'], cwd=WT, env=base_env(), stdout=f,
                               stderr=subprocess.PIPE)
        (LOGS / 'metadata.stderr.log').write_bytes(p.stderr)
        cmds = load_cmds()
        cmds.append({'label': 'metadata', 'argv': ['cargo', '+nightly-2026-05-01', 'metadata',
                     '--format-version', '1', '--locked', '--offline'], 'cwd': str(WT),
                     'returncode': p.returncode, 'log': str(LOGS / 'metadata.stderr.log')})
        CMDS.write_text(json.dumps(cmds, indent=2) + '\n')
        if p.returncode:
            STOP_FILE.write_text(json.dumps({'status': 'STOPPED', 'failing_label': 'metadata',
                                             'returncode': p.returncode,
                                             'log': str(LOGS / 'metadata.stderr.log')}, indent=2) + '\n')
            raise SystemExit('STOP: cargo metadata failed')
    m = json.loads(meta.read_text())
    byid = {p['id']: p for p in m['packages']}
    nodes = {n['id']: n for n in m['resolve']['nodes']}
    todo = [i for i, p in byid.items() if p['name'] in pkgs]
    assert len(todo) == len(set(pkgs)), (sorted(pkgs), [byid[i]['name'] for i in todo])
    seen = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        todo.extend(nodes[i]['dependencies'])
    local = {}
    for i in seen:
        p = byid[i]
        if p['source'] is None:
            local[Path(p['manifest_path']).parent.relative_to(WT).as_posix()] = p['name']
    return local


def make_mirror(name, pkgs):
    local = closure_dirs(pkgs)
    root = W / f'signing-root-{name}'
    assert not root.exists(), root
    support = ['scripts/cellos-sign', 'scripts/sign-cell.py', 'scripts/unsafe-allowlist.toml',
               'scripts/cellos_sign']
    raw = subprocess.run(['git', 'ls-files', '-z', '--', *sorted(local), *support], cwd=WT,
                         stdout=subprocess.PIPE, check=True).stdout
    tracked = sorted(x.decode() for x in raw.split(b'\0') if x)
    files = set(tracked) | {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'}
    root.mkdir(mode=0o700)
    prov = {}
    for rel in sorted(files):
        src = WT / rel
        dst = root / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
        prov[rel] = sha(src)
    cell_dirs = sorted(d for d in local if d.startswith('cells/'))
    info = {'root': str(root), 'packages_requested': sorted(pkgs), 'local_closure': dict(sorted(local.items())),
            'cell_crates_in_scope': cell_dirs, 'file_count': len(files), 'files_sha256': prov}
    (OUT / f'signing-scope-{name}.json').write_text(json.dumps(info, indent=2, sort_keys=True) + '\n')
    return root


def scoped_sign(label, mirror, targets):
    """Check then sign through the historical scripts/cellos-sign with --repo = mirror."""
    return run(label, ['python3', mirror / 'scripts/cellos-sign', '--repo', mirror, '--objcopy', 'objcopy',
                       '--sign', *targets])


def sections(data):
    import struct
    shoff = struct.unpack_from('<Q', data, 40)[0]
    entsize, count, names = struct.unpack_from('<HHH', data, 58)
    raw = [struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * 64) for i in range(count)]
    st = raw[names]
    table = data[st[4]:st[4] + st[5]]
    out = []
    for sh in raw:
        end = table.find(b'\0', sh[0])
        out.append(table[sh[0]:end].decode())
    return out


# ------------------------------------------------------------------------ headless
CELLS = [('app-shell', '/bin/shell'), ('service-vfs', '/bin/vfs'), ('service-config', '/bin/config'),
         ('service-net', '/bin/net'), ('service-input', '/bin/input'), ('platform', '/bin/platform'),
         ('driver-nvme', '/bin/nvme'), ('driver-e1000', '/bin/e1000'), ('driver-igb', '/bin/igb'),
         ('driver-ahci', '/bin/ahci'), ('driver-xhci', '/bin/xhci'), ('driver-serial-x86', '/bin/serial'),
         ('ls', '/bin/ls'), ('cat', '/bin/cat'), ('echo', '/bin/echo'), ('ps', '/bin/ps'),
         ('kill', '/bin/kill'), ('tier2-smoke', '/bin/tier2-smoke'), ('tier2-exploit', '/bin/tier2-exploit')]


def phase_headless_build():
    # Steps and order of scripts/build-x86_64-cells.ps1
    ccargo('10-build-app-shell', ['app-shell'])
    ccargo('11-build-service-vfs', ['service-vfs'])
    ccargo('12-build-service-config', ['service-config'])
    ccargo('13-build-service-input', ['service-input'])
    ccargo('14-build-service-net', ['service-net'])
    for i, pkg in enumerate(['service-platform', 'driver-nvme', 'driver-e1000', 'driver-igb',
                             'driver-ahci', 'driver-xhci', 'driver-serial-x86']):
        ccargo(f'15-build-{pkg}', [pkg])
    ccargo('16-build-app-sys-tools', ['app-sys-tools'])
    ccargo('17-build-app-init', ['app-init'], ['--features', 'input'])
    ccargo('18-build-tier2', ['tier2-smoke', 'tier2-exploit'])
    names = [b for b, _ in CELLS] + ['app-init']
    for n in names:
        assert (REL / n).is_file(), f'missing built artifact {REL / n}'


def phase_headless_image():
    names = [b for b, _ in CELLS if b not in ('tier2-smoke', 'tier2-exploit')] + ['app-init']
    # preserve unsigned originals
    for n in names + ['tier2-smoke', 'tier2-exploit']:
        shutil.copyfile(REL / n, OUT / 'unsigned' / f'headless-{n}')
        assert '__ViCell_sig' not in sections((REL / n).read_bytes()), f'{n} already signed'
    pkgs = ['app-shell', 'service-vfs', 'service-config', 'service-net', 'service-input', 'service-platform',
            'driver-nvme', 'driver-e1000', 'driver-igb', 'driver-ahci', 'driver-xhci', 'driver-serial-x86',
            'app-sys-tools', 'app-init']
    mirror = make_mirror('headless', pkgs)
    # whole-workspace refusal, retained as evidence (historical tool, unscoped)
    p = subprocess.run(['python3', 'scripts/cellos-sign', '--check', '--strict'], cwd=WT, env=base_env(),
                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (OUT / 'logs' / 'whole-workspace-check.log').write_bytes(p.stdout)
    (OUT / 'logs' / 'whole-workspace-check.rc').write_text(str(p.returncode) + '\n')
    scoped_sign('20-sign-headless', mirror, [REL / n for n in names])
    for n in names:
        shutil.copyfile(REL / n, OUT / 'signed' / f'headless-{n}')
    shutil.copyfile(REL / 'app-init', EMB / 'init')
    policy = TARGET / 'ViCell_x86_POLICY.BIN'
    run('21-sign-policy', ['python3', 'scripts/sign-policy.py', '--out', policy])
    img = EMB / 'kernel_fs.img'
    argv = ['python3', 'tools/mkfat32.py', '--config-features', 'input', '--config-output-dir',
            EMB / 'boot-config', img]
    for b, dst in CELLS:
        argv += [REL / b, dst]
    argv += [policy, '/POLICY.BIN']
    run('22-mkfat32-headless', argv)
    run('23-inspect-fat-headless', ['python3', 'tools/inspect_fat.py', img])
    lay = (LOGS / '23-inspect-fat-headless.log').read_text()
    assert 'SFN POLICY.BIN' in lay and "LFN[1] 'net'" in lay, 'layout assertion failed'
    policy.unlink()
    shutil.copyfile(img, OUT / 'headless-kernel_fs.img')
    shutil.copyfile(EMB / 'init', OUT / 'headless-init')
    for f in sorted((EMB / 'boot-config').glob('*.toml')):
        shutil.copyfile(f, OUT / f'headless-bootcfg-{f.name}')
    # kernel
    cargo('24-build-kernel-headless', ['build', '--release', '--locked', '--offline', '-p', 'cellos-kernel',
                                       *TGT, *BS], kernel_env(), KERNEL_OV)
    k = TARGET / 'x86_64-unknown-none/release/cellos-kernel'
    shutil.copyfile(k, OUT / 'headless-kernel.elf')
    env = base_env()
    env['X86_KERNEL'] = str(OUT / 'headless-kernel.elf')
    env['X86_ISO_ROOT'] = str(W / 'iso-root-headless')
    run('25-iso-headless', ['bash', 'scripts/x86/make-iso-ci.sh', OUT / 'headless-vicell-x86.iso'], env=env,
        overrides={'X86_KERNEL': env['X86_KERNEL'], 'X86_ISO_ROOT': env['X86_ISO_ROOT']})
    shutil.copyfile(WT / 'scripts/x86/limine.conf', OUT / 'limine.conf')


# ------------------------------------------------------------------------ desktop
def phase_desktop():
    # restore the tracked embedded base (run-desktop.sh --rebuild starts from it)
    run('30-restore-tracked-embedded', ['git', 'checkout', '--', 'kernel/src/embedded-x86_64/kernel_fs.img',
                                        'kernel/src/embedded-x86_64/init'])
    kfs = EMB / 'kernel_fs.img'
    shutil.copyfile(EMB / 'kernel_fs.img', OUT / 'desktop-base-tracked-kernel_fs.img')
    shutil.copyfile(EMB / 'init', OUT / 'desktop-base-tracked-init')
    (WT / 'build').mkdir(exist_ok=True)
    shutil.copyfile(kfs, WT / 'build/kernel_fs.saved.img')
    ccargo('31-build-ocel-desktop', ['ocel'], ['--no-default-features', '--features', 'desktop'])
    ccargo('32-build-service-net-qemu-dev-https', ['service-net'], ['--no-default-features', '--features',
                                                                   'qemu-dev-https'])
    for n in ('ocel', 'service-net'):
        shutil.copyfile(REL / n, OUT / 'unsigned' / f'desktop-{n}')
        assert '__ViCell_sig' not in sections((REL / n).read_bytes()), f'{n} already signed'
    mirror = make_mirror('desktop', ['ocel', 'service-net'])
    scoped_sign('33-sign-desktop', mirror, [REL / 'ocel', REL / 'service-net'])
    for n in ('ocel', 'service-net'):
        shutil.copyfile(REL / n, OUT / 'signed' / f'desktop-{n}')
    run('34-package-ocel', ['python3', 'scripts/package-ocel-profile.py', kfs, REL / 'ocel'])
    run('35-package-net', ['python3', 'scripts/package-ocel-profile.py', kfs, REL / 'service-net',
                           '--dest', '/bin/net'])
    # B2 comparator overlay: source-built signed shell + workers replace the stale tracked ones
    for n, dst in [('app-shell', '/bin/shell'), ('ls', '/bin/ls'), ('cat', '/bin/cat'), ('echo', '/bin/echo'),
                   ('ps', '/bin/ps'), ('kill', '/bin/kill')]:
        run('36-package-b2-' + n, ['python3', 'scripts/package-ocel-profile.py', kfs,
                                   OUT / 'signed' / f'headless-{n}', '--dest', dst])
    shutil.copyfile(kfs, OUT / 'desktop-kernel_fs.img')
    shutil.copyfile(EMB / 'init', OUT / 'desktop-init')
    cargo('37-build-kernel-desktop', ['build', '--release', '--locked', '--offline', '-p', 'cellos-kernel',
                                      *TGT, '--features', DESKTOP_KERNEL_FEATURES], kernel_env(), KERNEL_OV)
    # run-desktop.sh restores the embedded image right after the kernel build
    shutil.copyfile(WT / 'build/kernel_fs.saved.img', kfs)
    k = TARGET / 'x86_64-unknown-none/release/cellos-kernel'
    shutil.copyfile(k, OUT / 'desktop-kernel.elf')
    env = base_env()
    iso = OUT / 'desktop-cellos-x86_64-desktop.iso'
    env['X86_KERNEL'] = str(OUT / 'desktop-kernel.elf')
    env['X86_ISO_ROOT'] = str(W / 'iso-root-desktop')
    run('38-iso-desktop', ['bash', 'scripts/x86/make-iso-ci.sh', iso], env=env,
        overrides={'X86_KERNEL': env['X86_KERNEL'], 'X86_ISO_ROOT': env['X86_ISO_ROOT']})
    run('39-limine-bios-install-desktop', ['limine/limine-8.7.0/bin/limine', 'bios-install', iso])
    run('40-qemu-img-desktop', ['qemu-img', 'convert', '-f', 'raw', '-O', 'qcow2', iso,
                                OUT / 'desktop-cellos-x86_64-desktop.qcow2'])
    # same post-processing for headless ISO copy (extra, labelled)
    hiso = OUT / 'headless-vicell-x86.bios-installed.iso'
    shutil.copyfile(OUT / 'headless-vicell-x86.iso', hiso)
    run('41-limine-bios-install-headless', ['limine/limine-8.7.0/bin/limine', 'bios-install', hiso])
    run('42-qemu-img-headless', ['qemu-img', 'convert', '-f', 'raw', '-O', 'qcow2', hiso,
                                 OUT / 'headless-vicell-x86.qcow2'])


if __name__ == '__main__':
    phase = sys.argv[1]
    globals()['phase_' + phase.replace('-', '_')]()
    print('PHASE OK', phase)
