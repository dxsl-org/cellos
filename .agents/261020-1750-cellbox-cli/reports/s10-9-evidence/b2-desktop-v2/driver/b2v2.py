#!/usr/bin/env python3
"""S10-9.3 driver: B2 desktop image v2 -- every desktop service rebuilt from source at commit 3471028bc.

Replays, step by step, the commit's own desktop route (run-desktop.sh --rebuild + scripts/package-desktop-x86.py)
but with the allowed deltas ONLY:
  1. b2-manifest.patch (already applied in the disposable tree, verified byte-equal to commit+patch)
  2. b2-lock.patch     (same)
  3. scoped F1 signing (mirror root with the local-path closure of the signed packages; whole-workspace F1 still refuses)
and, per the S10-9.3 task, all desktop services/apps rebuilt from source (instead of tracked historical bytes).
Every cargo call: pinned nightly-2026-05-01, --locked --offline.  Fail-fast, one attempt per step, no retry.
"""
import hashlib, importlib.util, json, mmap, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path

sys.dont_write_bytecode = True
HEAD = '3471028bc8b4db14275553b4740c17e24fa4f32b'
MAIN_REPO = Path('/home/dmin/cellos')
W = Path('/tmp/cellbox-s10-7')
WT = W / 'worktree'
TARGET = WT / 'target'
REL = TARGET / 'x86_64-unknown-none/release'
EMB = WT / 'kernel/src/embedded-x86_64'
V1 = W / 'b2-image'
OUT = W / 'b2-image-v2'
W2 = Path('/tmp/cellbox-s10-9')
STAGE = W2 / 'stage'
STATE = W2 / 'state'
LOGS = OUT / 'cmd-logs'
CMDS = STATE / 'commands.json'
STOP_FILE = STATE / 'stop.json'
PATCHES = Path('/home/dmin/cellos/.agents/261020-1750-cellbox-cli/reports/s10-4-evidence')

KERNEL_RUSTFLAGS = '-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static'
DESKTOP_KERNEL_FEATURES = ('dev-policy-key,dev-signing-key,dev-weak-rng,signing-required,native-domains,'
                           'qemu-dev-https,large-cell-va')
STRIP_ENV = ['CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS', 'CELLBOX_PRIVATE_FIXTURE_SIGNING',
             'CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI', 'CELLOS_SIGN_SEED_HEX',
             'CELLOS_PHASE02_CELL_PUBLIC_KEY', 'CELLOS_CONFIG_DIR', 'CELLOS_INIT_FEATURES',
             'EMBEDDED_OVERRIDE', 'RUSTUP_TOOLCHAIN', 'X86_KERNEL', 'X86_ISO_ROOT', 'XORRISO',
             'OBJCOPY', 'GIT_DIR', 'GIT_WORK_TREE', 'CARGO_TARGET_DIR']

# (label, cargo package, extra cargo args, built binary name, image dest or None, headless-equivalent name)
BUILDS = [
    ('service-vfs', 'service-vfs', [], 'service-vfs', '/bin/vfs'),
    ('service-config', 'service-config', [], 'service-config', '/bin/config'),
    ('service-input', 'service-input', [], 'service-input', '/bin/input'),
    ('service-platform', 'service-platform', [], 'platform', '/bin/platform'),
    ('driver-nvme', 'driver-nvme', [], 'driver-nvme', '/bin/nvme'),
    ('driver-e1000', 'driver-e1000', [], 'driver-e1000', '/bin/e1000'),
    ('driver-igb', 'driver-igb', [], 'driver-igb', '/bin/igb'),
    ('driver-ahci', 'driver-ahci', [], 'driver-ahci', '/bin/ahci'),
    ('driver-xhci', 'driver-xhci', [], 'driver-xhci', '/bin/xhci'),
    ('driver-serial-x86', 'driver-serial-x86', [], 'driver-serial-x86', '/bin/serial'),
    ('service-compositor', 'service-compositor', [], 'service-compositor', '/bin/compositor'),
    ('fb-console', 'fb-console', [], 'fb-console', '/bin/fb-console'),
    ('service-sshd', 'service-sshd', [], 'service-sshd', '/bin/sshd'),
    ('desktop', 'desktop', [], 'desktop', '/bin/desktop'),
    ('files', 'files', [], 'files', '/bin/files'),
    ('terminal', 'terminal', [], 'terminal', '/bin/terminal'),
    ('app-chrome', 'app-chrome', [], 'chrome', '/bin/chrome'),
    ('app-shell', 'app-shell', [], 'app-shell', '/bin/shell'),
    ('ocel-desktop', 'ocel', ['--no-default-features', '--features', 'desktop'], 'ocel', '/bin/ocel'),
    ('service-net-qemu-dev-https', 'service-net', ['--no-default-features', '--features', 'qemu-dev-https'],
     'service-net', '/bin/net'),
]
# app-sys-tools produces the five B2 tools in one invocation (as scripts/build-x86_64-cells.ps1)
SYS_TOOLS = [('ls', '/bin/ls'), ('cat', '/bin/cat'), ('echo', '/bin/echo'), ('ps', '/bin/ps'), ('kill', '/bin/kill')]
# files deliberately kept as the TRACKED historical bytes (reported, not rebuilt)
TRACKED_KEPT = ['/bin/ocel-quickjs', '/bin/tier2-smoke', '/bin/tier2-exploit', '/POLICY.BIN']

AUTOLOAD = (
    b"# Applications start after the configured required services are ready.\n"
    b"version = 1\n\n"
    b"[[cells]]\n"
    b"name = \"desktop\"\n"
    b"path = \"/bin/desktop\"\n"
    b"after = [\"compositor\", \"input\"]\n"
    b"restart = \"always\"\n\n"
    b"[[cells]]\n"
    b"name = \"shell\"\n"
    b"path = \"/bin/shell\"\n"
    b"after = [\"compositor\", \"input\"]\n"
    b"restart = \"always\"\n"
)


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


CELLS_OV = {'RUSTFLAGS': '-C relocation-model=pic', 'CARGO_NET_OFFLINE': 'true', 'CARGO_TARGET_DIR': str(TARGET)}
KERNEL_OV = {'CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS': KERNEL_RUSTFLAGS, 'CARGO_NET_OFFLINE': 'true',
             'CARGO_TARGET_DIR': str(TARGET)}
BS = ['-Z', 'build-std=core,alloc']
TGT = ['--target', 'x86_64-unknown-none']


def load_cmds():
    return json.loads(CMDS.read_text()) if CMDS.exists() else []


def run(label, argv, env=None, cwd=WT, overrides=None, timeout=3300):
    LOGS.mkdir(parents=True, exist_ok=True)
    STATE.mkdir(parents=True, exist_ok=True)
    if STOP_FILE.exists():
        raise SystemExit('STOP already recorded; refusing to run ' + label)
    log = LOGS / (label + '.log')
    assert not log.exists(), f'log exists (one attempt per step): {log}'
    t0 = time.time()
    argv = [str(a) for a in argv]
    with log.open('wb') as f:
        p = subprocess.run(argv, cwd=str(cwd), env=env or base_env(), stdout=f, stderr=subprocess.STDOUT,
                           timeout=timeout)
    rec = {'label': label, 'argv': argv, 'cwd': str(cwd), 'returncode': p.returncode, 'log': str(log),
           'log_sha256': sha(log), 'start': t0, 'seconds': round(time.time() - t0, 1),
           'env_overrides': overrides or {}}
    cmds = load_cmds()
    cmds.append(rec)
    CMDS.write_text(json.dumps(cmds, indent=2) + '\n')
    if p.returncode:
        STOP_FILE.write_text(json.dumps({'status': 'STOPPED', 'failing_label': label, 'argv': argv,
                                         'cwd': str(cwd), 'returncode': p.returncode, 'log': str(log)},
                                        indent=2) + '\n')
        print(f'STOP: {label} exit {p.returncode}; see {log}', file=sys.stderr)
        raise SystemExit(1)
    return rec


def cargo(label, args, env, overrides):
    return run(label, ['cargo', '+nightly-2026-05-01', *args], env=env, overrides=overrides)


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


def git_blob(path):
    return subprocess.run(['git', '-C', str(MAIN_REPO), 'show', f'{HEAD}:{path}'], stdout=subprocess.PIPE,
                          check=True).stdout


def load_mod(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


# ------------------------------------------------------------------------------------- setup
def phase_setup():
    for sub in ('unsigned', 'signed', 'patches', 'logs', 'cmd-logs'):
        (OUT / sub).mkdir(parents=True, exist_ok=True)
    # 1. tree == commit + the two authorised patches (plain tree, no .git): compare every tracked blob
    ents = [e for e in subprocess.run(['git', '-C', str(MAIN_REPO), 'ls-tree', '-r', '-z', HEAD],
                                      stdout=subprocess.PIPE, check=True).stdout.split(b'\0') if e]
    diff, missing, n = [], [], 0
    for e in ents:
        meta, path = e.split(b'\t', 1)
        mode, typ, h = meta.split()
        p = WT / path.decode()
        if typ != b'blob':
            continue
        n += 1
        if not os.path.lexists(p):
            missing.append(path.decode())
            continue
        d = os.readlink(p).encode() if mode == b'120000' else p.read_bytes()
        if hashlib.sha1(b'blob %d\0' % len(d) + d).hexdigest() != h.decode():
            diff.append(path.decode())
    assert not missing, missing
    assert sorted(diff) == ['Cargo.lock', 'Cargo.toml'], diff
    tmp = Path(tempfile.mkdtemp(prefix='s109-patchchk-'))
    for f in ('Cargo.toml', 'Cargo.lock'):
        (tmp / f).write_bytes(git_blob(f))
    for name in ('b2-manifest.patch', 'b2-lock.patch'):
        shutil.copyfile(PATCHES / name, OUT / 'patches' / name)
        r = subprocess.run(['git', 'apply', '--', str(OUT / 'patches' / name)], cwd=tmp, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT)
        assert r.returncode == 0, r.stdout
    for f in ('Cargo.toml', 'Cargo.lock'):
        assert (tmp / f).read_bytes() == (WT / f).read_bytes(), f
    shutil.rmtree(tmp)
    # 2. embedded tracked base files in the tree equal the commit blobs
    base_img = git_blob('kernel/src/embedded-x86_64/kernel_fs.img')
    base_init = git_blob('kernel/src/embedded-x86_64/init')
    assert (EMB / 'kernel_fs.img').read_bytes() == base_img
    assert (EMB / 'init').read_bytes() == base_init
    (OUT / 'desktop-base-tracked-kernel_fs.img').write_bytes(base_img)
    (OUT / 'desktop-base-tracked-init').write_bytes(base_init)
    json.dump({'commit': HEAD, 'tracked_blobs_compared': n, 'differing_from_commit': diff,
               'explained_by': ['b2-manifest.patch -> Cargo.toml', 'b2-lock.patch -> Cargo.lock'],
               'patched_files_equal_commit_plus_patches': True,
               'embedded_kernel_fs_img_sha256': hashlib.sha256(base_img).hexdigest(),
               'embedded_init_sha256': hashlib.sha256(base_init).hexdigest()},
              (OUT / 'logs' / 'tree-vs-commit.json').open('w'), indent=2)
    run('04-rustc', ['rustc', '+nightly-2026-05-01', '-Vv'])
    run('05-cargo', ['cargo', '+nightly-2026-05-01', '-V'])
    run('06-rustc-active', ['rustc', '-Vv'])
    assert (WT / 'limine/limine-8.7.0/bin/limine').is_file()


# ------------------------------------------------------------------------------------- build
def phase_build():
    def stash(label, binname):
        src = REL / binname
        assert src.is_file(), src
        data = src.read_bytes()
        assert '__ViCell_sig' not in sections(data), f'{label}: cargo output already signed'
        (OUT / 'unsigned').mkdir(exist_ok=True)
        shutil.copyfile(src, OUT / 'unsigned' / label)

    n = 0
    for label, pkg, extra, binname, _dest in BUILDS:
        n += 1
        cargo(f'1{n:02d}-build-{label}', ['build', '--release', '--locked', '--offline', '-p', pkg, *extra, *TGT, *BS],
              cells_env(), CELLS_OV)
        stash(label, binname)
    cargo('150-build-app-sys-tools', ['build', '--release', '--locked', '--offline', '-p', 'app-sys-tools', *TGT, *BS],
          cells_env(), CELLS_OV)
    for b, _ in SYS_TOOLS:
        stash(b, b)


# ------------------------------------------------------------------------------------- sign
def closure_dirs(pkgs):
    meta = STATE / 'metadata.json'
    if not meta.exists():
        with meta.open('wb') as f:
            p = subprocess.run(['cargo', '+nightly-2026-05-01', 'metadata', '--format-version', '1', '--locked',
                                '--offline'], cwd=WT, env=base_env(), stdout=f, stderr=subprocess.PIPE)
        (OUT / 'logs' / 'metadata.stderr.log').write_bytes(p.stderr)
        assert p.returncode == 0, p.stderr
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
    support = ['scripts/cellos-sign', 'scripts/sign-cell.py', 'scripts/unsafe-allowlist.toml', 'scripts/cellos_sign']
    prefixes = sorted(local) + support
    ents = [e for e in subprocess.run(['git', '-C', str(MAIN_REPO), 'ls-tree', '-r', '-z', '--name-only', HEAD],
                                      stdout=subprocess.PIPE, check=True).stdout.split(b'\0') if e]
    tracked = sorted(e.decode() for e in ents if any(e.decode() == p or e.decode().startswith(p + '/')
                                                     for p in prefixes))
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


def phase_sign():
    names = [l for l, *_ in BUILDS] + [b for b, _ in SYS_TOOLS]
    pkgs = sorted({pkg for _l, pkg, *_ in BUILDS} | {'app-sys-tools'})
    mirror = make_mirror('desktop-v2', pkgs)
    # whole-workspace refusal retained as evidence (historical tool, unscoped, check only)
    p = subprocess.run(['python3', 'scripts/cellos-sign', '--check', '--strict'], cwd=WT, env=base_env(),
                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (OUT / 'logs' / 'whole-workspace-check.log').write_bytes(p.stdout)
    (OUT / 'logs' / 'whole-workspace-check.rc').write_text(str(p.returncode) + '\n')
    # sign STAGED COPIES (the build output in target/ stays unsigned); basename == cargo binary name
    shutil.rmtree(STAGE, ignore_errors=True)
    STAGE.mkdir(parents=True)
    bins = {}
    for l, _pkg, _extra, binname, _d in BUILDS:
        bins[l] = binname
    for b, _ in SYS_TOOLS:
        bins[b] = b
    staged = []
    for label in names:
        d = STAGE / label
        d.mkdir()
        shutil.copyfile(OUT / 'unsigned' / label, d / bins[label])
        staged.append(d / bins[label])
    run('200-sign-desktop-v2', ['python3', mirror / 'scripts/cellos-sign', '--repo', mirror, '--objcopy', 'objcopy',
                                '--sign', *staged])
    for label in names:
        shutil.copyfile(STAGE / label / bins[label], OUT / 'signed' / label)


# ------------------------------------------------------------------------------------- image
def phase_image():
    reader = load_mod('cellos_profile_reader', WT / 'scripts/generate-boot-config.py')
    writer = load_mod('cellos_profile_writer', WT / 'tools/mkfat32.py')
    base = OUT / 'desktop-base-tracked-kernel_fs.img'
    files = reader.image_files(base)
    dest_of = {l: d for l, _p, _e, _b, d in BUILDS}
    dest_of.update({b: d for b, d in SYS_TOOLS})
    for label, dest in dest_of.items():
        data = (OUT / 'signed' / label).read_bytes()
        existing = next((k for k in files if k.lower() == dest.lower()), None)
        files[existing or dest] = data
    files['/etc/cellos/autoload.toml'] = AUTOLOAD
    with tempfile.TemporaryDirectory(prefix='desktop-v2-', dir=OUT) as staging:
        staging = Path(staging)
        pairs = []
        for index, (destination, data) in enumerate(sorted(files.items())):
            src = staging / str(index)
            src.write_bytes(data)
            pairs.append((str(src), destination))
        output = staging / 'kernel_fs.img'
        writer.create_fat32_image(str(output), pairs)
        out_files = reader.image_files(output)
        assert out_files == files, 'FAT packaging changed contents'
        shutil.copyfile(output, OUT / 'desktop-kernel_fs.img')
    shutil.copyfile(OUT / 'desktop-base-tracked-init', OUT / 'desktop-init')
    run('210-inspect-fat-desktop', ['python3', 'tools/inspect_fat.py', OUT / 'desktop-kernel_fs.img'])
    lay = (LOGS / '210-inspect-fat-desktop.log').read_text()
    assert 'SFN POLICY.BIN' in lay and "LFN[1] 'net'" in lay, 'layout assertion failed'


# ------------------------------------------------------------------------------------- kernels + ISOs
def build_iso(tag, kernel, iso_name):
    env = base_env()
    env['X86_KERNEL'] = str(kernel)
    env['X86_ISO_ROOT'] = str(W / f'iso-root-v2-{tag}')
    iso = OUT / f'{iso_name}.iso'
    run(f'3{tag}-iso', ['bash', 'scripts/x86/make-iso-ci.sh', iso], env=env,
        overrides={'X86_KERNEL': env['X86_KERNEL'], 'X86_ISO_ROOT': env['X86_ISO_ROOT']})
    run(f'3{tag}-limine-bios-install', ['limine/limine-8.7.0/bin/limine', 'bios-install', iso])
    run(f'3{tag}-qemu-img', ['qemu-img', 'convert', '-f', 'raw', '-O', 'qcow2', iso, OUT / f'{iso_name}.qcow2'])


def phase_kernels():
    kfs = EMB / 'kernel_fs.img'
    shutil.copyfile(OUT / 'desktop-kernel_fs.img', kfs)
    shutil.copyfile(OUT / 'desktop-init', EMB / 'init')
    try:
        # non-hooks: exactly run-desktop.sh's kernel command (no -Z build-std), plus --locked --offline
        cargo('300-build-kernel-desktop', ['build', '--release', '--locked', '--offline', '-p', 'cellos-kernel',
                                           *TGT, '--features', DESKTOP_KERNEL_FEATURES], kernel_env(), KERNEL_OV)
        shutil.copyfile(TARGET / 'x86_64-unknown-none/release/cellos-kernel', OUT / 'desktop-kernel.elf')
        # hooks: only the kernel gains feature test-hooks (cells/image/init identical)
        cargo('301-build-kernel-desktop-test-hooks',
              ['build', '--release', '--locked', '--offline', '-p', 'cellos-kernel', *TGT,
               '--features', DESKTOP_KERNEL_FEATURES + ',test-hooks'], kernel_env(), KERNEL_OV)
        shutil.copyfile(TARGET / 'x86_64-unknown-none/release/cellos-kernel', OUT / 'desktop-test-hooks-kernel.elf')
    finally:
        # run-desktop.sh restores the tracked embedded image right after the kernel build
        shutil.copyfile(OUT / 'desktop-base-tracked-kernel_fs.img', kfs)
        shutil.copyfile(OUT / 'desktop-base-tracked-init', EMB / 'init')
    build_iso('10', OUT / 'desktop-kernel.elf', 'desktop-cellos-x86_64-desktop')
    build_iso('11', OUT / 'desktop-test-hooks-kernel.elf', 'desktop-test-hooks-cellos-x86_64-desktop')
    shutil.copyfile(WT / 'scripts/x86/limine.conf', OUT / 'limine.conf')


if __name__ == '__main__':
    phase = sys.argv[1]
    globals()['phase_' + phase.replace('-', '_')]()
    print('PHASE OK', phase)
