#!/usr/bin/env python3
"""S10-7.2 no-VM: assemble ISO/qcow2 for the four `test-hooks` measurement kernels and verify statically.

Sets: cand-desktop, cand-headless, b2-desktop, b2-headless.  Inputs are the outputs of build_kernels.py
(/tmp/cellbox-s10-7/th/<label>/{control,test-hooks}.kernel.elf).  Nothing is booted.
Writes /tmp/cellbox-s10-7/measure/<set>/ and <evidence>/<set>.static-verification.json.  Exit 1 on any failed check.
"""
import hashlib, importlib.util, json, mmap, os, shutil, subprocess, sys, tempfile
from pathlib import Path

sys.dont_write_bytecode = True
W = Path('/tmp/cellbox-s10-7')
OUT = W / 'measure'
EVID = Path('/home/dmin/cellos/.agents/261020-1750-cellbox-cli/reports/s10-7-evidence/test-hooks-kernels')
CAND = Path.home() / '.cache/cellos-candidates/4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821'
B2I = W / 'b2-image'
env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
cj = json.loads((CAND / 'candidate.json').read_text())
b2sv = json.loads((EVID.parent / 'b2-image/static-verification.json').read_text())


def cand_files(p):
    return {k: v['sha256'] for k, v in cj['profiles'][p]['files'].items()}


def b2_files(p):
    return {k: v['sha256'] for k, v in b2sv['profiles'][p]['files'].items()}


SETS = {
    'cand-desktop': dict(tree=W / 'cand-measure', label='cand', ctl_sha='dfbe8c555f54cf15293a8a59eead841d5c417b2faadb2e422ca97aa19744dc1a',
                         embed=CAND / 'desktop', img='3848cefb25f0e7c4abc67feccf3d8886292e25da60a88b4633552651292479ec',
                         init='c0bd14580fda0d697a2edd66b19168107fa6877a190bc06c1a8a482b467b23f3',
                         files=cand_files('desktop'), base_iso=CAND / 'desktop.iso', base_kernel=CAND / 'desktop.kernel.elf',
                         iso_name='cellos-x86_64-desktop-test-hooks.iso', sign_elfs=['/bin/cellbox', '/bin/vfs']),
    'cand-headless': dict(tree=W / 'cand-measure', label='cand-headless', ctl_sha='7efb030f3ff8f0f5cac965e12112e1dd567585eb59bc935f1c16e416266efb74',
                          embed=CAND / 'headless', img='384c04fa298c4d234bacee149af8787cc917aeb4541dea912365073ae790fcb3',
                          init='fc1e6559c47f271f59609197c714dc130f4ce9e8ea001e8b682a11caf5ded7ab',
                          files=cand_files('headless'), base_iso=CAND / 'headless.iso', base_kernel=CAND / 'headless.kernel.elf',
                          iso_name='vicell-x86-test-hooks.iso', sign_elfs=['/bin/cellbox', '/bin/vfs']),
    'b2-desktop': dict(tree=W / 'b2-measure', label='b2-desktop', ctl_sha='5959acd79291fe693aa0deaacac5cc67dcc16cb76e911705f5290bfa1e3ba828',
                       embed=W / 'th/embed-b2-desktop', img='37040c38f62fd1b28999517f8b1bd9b646f2ed12d3073e810ea7dae601593f83',
                       init='0c2f607bb29e090b31c7820290aafc1974c818c0610d216f00b5f88a10104104',
                       files=b2_files('desktop'), base_iso=B2I / 'desktop-cellos-x86_64-desktop.iso', base_kernel=B2I / 'desktop-kernel.elf',
                       iso_name='cellos-x86_64-desktop-b2-test-hooks.iso', sign_elfs=['/bin/shell', '/bin/ls', '/bin/cat', '/bin/echo', '/bin/ps', '/bin/kill']),
    'b2-headless': dict(tree=W / 'b2-measure', label='b2-headless', ctl_sha='28ba57c88c49d284f9f5fb3b2a2be349c0eee8035dc7c9429b902d79cdf5c672',
                        embed=W / 'th/embed-b2-headless', img='f75f3c0651107571b03f6249f4f2c4c12d0ef747620ccd12228039b47ece0330',
                        init='920c0294727b5b10cf8ca88738ef3c71f82366363b34b7f88547eebc522b3621',
                        files=b2_files('headless'), base_iso=B2I / 'headless-vicell-x86.iso', base_kernel=B2I / 'headless-kernel.elf',
                        iso_name='vicell-x86-b2-test-hooks.iso', sign_elfs=['/bin/shell', '/bin/ls', '/bin/cat', '/bin/echo', '/bin/ps', '/bin/kill']),
}
# candidate-headless builds reused the same tree and its own th/ label
SETS['cand-headless']['th'] = W / 'th/cand-headless'
SETS['cand-desktop']['th'] = W / 'th/cand'
SETS['b2-desktop']['th'] = W / 'th/b2-desktop-nbs'  # the S10-7.1 desktop kernel route had no -Z build-std
SETS['b2-headless']['th'] = W / 'th/b2-headless'


def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def sh(argv, **kw):
    return subprocess.run([str(a) for a in argv], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=kw.pop('env', env), **kw)


def tree_of(iso, dest):
    r = sh(['xorriso', '-osirrox', 'on', '-indev', iso, '-extract', '/', dest])
    out = {}
    for p in sorted(Path(dest).rglob('*')):
        if p.is_file():
            out[p.relative_to(dest).as_posix()] = sha(p)
    return out, r


def run_set(name, s):
    d = OUT / name
    d.mkdir(parents=True, exist_ok=True)
    res = {'set': name, 'checks': [], 'ok': True, 'artifacts': {}}

    def check(n, ok, detail=None):
        res['checks'].append({'name': n, 'ok': bool(ok), 'detail': detail})
        if not ok:
            res['ok'] = False

    th = s['th']
    ctl, hook = th / 'control.kernel.elf', th / 'test-hooks.kernel.elf'
    shutil.copyfile(hook, d / f'{name}-test-hooks.kernel.elf')
    shutil.copyfile(ctl, d / f'{name}-control.kernel.elf')
    shutil.copyfile(s['embed'] / 'kernel_fs.img', d / 'kernel_fs.img')
    shutil.copyfile(s['embed'] / 'init', d / 'init')
    hk = d / f'{name}-test-hooks.kernel.elf'
    check('control kernel (non-hooks, same recipe) == previously qualified/built kernel (byte-reproduced)',
          sha(ctl) == s['ctl_sha'] and sha(s['base_kernel']) == s['ctl_sha'], {'control': sha(ctl), 'expected': s['ctl_sha']})
    check('test-hooks kernel differs from control kernel', sha(hook) != sha(ctl), {'hooks': sha(hook), 'control_bytes': ctl.stat().st_size, 'hooks_bytes': hook.stat().st_size})
    check('embedded kernel_fs.img sha256 as expected', sha(d / 'kernel_fs.img') == s['img'], s['img'])
    check('embedded init sha256 as expected', sha(d / 'init') == s['init'], s['init'])
    img, init = (d / 'kernel_fs.img').read_bytes(), (d / 'init').read_bytes()
    offs = {}
    for tag, k in (('control', ctl), ('test-hooks', hook)):
        with open(k, 'rb') as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
            offs[tag] = {'kernel_fs.img': m.find(img), 'init': m.find(init)}
            # exactly one occurrence each
            offs[tag]['img_second'] = m.find(img, offs[tag]['kernel_fs.img'] + 1) if offs[tag]['kernel_fs.img'] >= 0 else -1
        check(f'{tag} kernel embeds kernel_fs.img byte-for-byte (single copy)', offs[tag]['kernel_fs.img'] >= 0 and offs[tag]['img_second'] < 0, offs[tag])
        check(f'{tag} kernel embeds init byte-for-byte', offs[tag]['init'] >= 0, offs[tag])
    res['embed_offsets'] = offs
    # every file in the embedded image equals the previously verified build output
    spec = importlib.util.spec_from_file_location('g', s['tree'] / 'scripts/generate-boot-config.py')
    g = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(g)
    files = g.image_files(d / 'kernel_fs.img')
    got = {p: hashlib.sha256(v).hexdigest() for p, v in files.items()}
    want = s['files']
    check('embedded image has exactly the file set of the non-hooks build', set(got) == set(want), {'count': len(got), 'extra': sorted(set(got) - set(want)), 'missing': sorted(set(want) - set(got))})
    check('every embedded-image file sha256 == non-hooks build record (cellbox/B2 ELFs byte-identical)', all(got.get(p) == h for p, h in want.items()), [p for p, h in want.items() if got.get(p) != h])
    res['image_file_sha256'] = got
    # signatures of key ELFs
    tmp = Path(tempfile.mkdtemp(prefix='thv-'))
    sc = s['tree'] / 'scripts/sign-cell.py'
    for p in s['sign_elfs']:
        if p not in files:
            check(f'{p} present in image', False)
            continue
        fp = tmp / p.strip('/').replace('/', '_')
        fp.write_bytes(files[p])
        r = sh(['python3', sc, '--verify', '--in', fp])
        check(f'{p} dev signature verifies', r.returncode == 0, r.stdout.decode(errors='replace').strip()[-160:])
    shutil.rmtree(tmp)
    check('/POLICY.BIN present', '/POLICY.BIN' in files)
    for c in ('/etc/cellos/autoload.toml', '/etc/cellos/services.toml', '/etc/cellos/system.toml'):
        check(f'{c} present', c in files)
    shell_elf = files.get('/bin/cellbox') or files.get('/bin/shell')
    check('shell ELF carries the "Cellos > " prompt literal', b'Cellos > ' in (shell_elf or b''))
    sysd = files.get('/etc/cellos/system.toml', b'').decode(errors='replace')
    res['image_system_toml'] = sysd
    # ELF headers
    for tag, k in (('control', ctl), ('test-hooks', hook)):
        r = sh(['readelf', '-hlW', k])
        (d / f'{tag}-readelf.txt').write_bytes(r.stdout)
        t = r.stdout.decode()
        check(f'{tag} kernel is ELF64 x86-64 EXEC', 'Class:                             ELF64' in t and 'Advanced Micro Devices X86-64' in t and 'EXEC (Executable file)' in t)
        res.setdefault('entry', {})[tag] = [l.split(':', 1)[1].strip() for l in t.splitlines() if 'Entry point' in l]
    # ISO
    iso = d / s['iso_name']
    if iso.exists():
        iso.unlink()
    envx = dict(env, X86_KERNEL=str(hk), X86_ISO_ROOT=str(d / 'iso-root'))
    shutil.rmtree(d / 'iso-root', ignore_errors=True)
    r = sh(['bash', 'scripts/x86/make-iso-ci.sh', iso], cwd=s['tree'], env=envx)
    (d / 'make-iso.log').write_bytes(r.stdout)
    check('make-iso-ci.sh succeeded', r.returncode == 0 and iso.is_file(), r.returncode)
    t_new, r1 = tree_of(iso, tmp / 'new') if (tmp := Path(tempfile.mkdtemp(prefix='thiso-'))) else None
    t_old, r2 = tree_of(s['base_iso'], tmp / 'old')
    (d / 'iso-files.txt').write_text(''.join(f'{h}  {p}\n' for p, h in sorted(t_new.items())))
    check('ISO /boot/kernel.elf == test-hooks kernel', t_new.get('boot/kernel.elf') == sha(hk), t_new.get('boot/kernel.elf'))
    check('ISO /boot/limine.conf == tree scripts/x86/limine.conf', t_new.get('boot/limine.conf') == sha(s['tree'] / 'scripts/x86/limine.conf'))
    diff = sorted(p for p in set(t_new) | set(t_old) if t_new.get(p) != t_old.get(p))
    check('ISO tree differs from the non-hooks ISO tree ONLY in boot/kernel.elf', diff == ['boot/kernel.elf'], diff)
    check('non-hooks ISO boot/kernel.elf == control kernel', t_old.get('boot/kernel.elf') == sha(ctl))
    shutil.rmtree(tmp)
    # qcow2 (bios-installed ISO copy, as the S10-7.1 route)
    bios = d / (s['iso_name'].replace('.iso', '.bios-installed.iso'))
    shutil.copyfile(iso, bios)
    limine = s['tree'] / 'limine/limine-8.7.0/bin/limine'
    r = sh([limine, 'bios-install', bios])
    (d / 'limine-bios-install.log').write_bytes(r.stdout)
    check('limine bios-install succeeded', r.returncode == 0, r.stdout.decode(errors='replace').strip()[-120:])
    qc = d / (s['iso_name'].replace('.iso', '.qcow2'))
    if qc.exists():
        qc.unlink()
    r = sh(['qemu-img', 'convert', '-f', 'raw', '-O', 'qcow2', bios, qc])
    check('qemu-img convert succeeded', r.returncode == 0)
    raw = d / 'roundtrip.raw'
    sh(['qemu-img', 'convert', '-f', 'qcow2', '-O', 'raw', qc, raw])
    check('qcow2 round-trips to the bios-installed ISO', raw.is_file() and sha(raw) == sha(bios))
    with open(raw, 'rb') as f:
        mbr = f.read(512)
    check('disk LBA0 0x55AA boot signature', mbr[510:512] == b'\x55\xaa')
    raw.unlink()
    r = sh(['qemu-img', 'check', qc])
    check('qcow2 qemu-img check clean', r.returncode == 0, r.stdout.decode(errors='replace').strip()[-120:])
    res['artifacts'] = {p.name: {'sha256': sha(p), 'bytes': p.stat().st_size} for p in sorted(d.iterdir()) if p.is_file()}
    (EVID / f'{name}.static-verification.json').write_text(json.dumps(res, indent=2, sort_keys=True) + '\n')
    print(name, 'OK' if res['ok'] else 'FAIL')
    for c in res['checks']:
        if not c['ok']:
            print('  FAILED:', c)
    return res['ok']


if __name__ == '__main__':
    names = sys.argv[1:] or list(SETS)
    ok = all([run_set(n, SETS[n]) for n in names])
    sys.exit(0 if ok else 1)
