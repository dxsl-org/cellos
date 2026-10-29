#!/usr/bin/env python3
"""No-VM static verification + manifest for the B2 desktop v2 image set."""
import hashlib, importlib.util, json, mmap, os, shutil, struct, subprocess, sys, tempfile
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
import b2v2 as B

OUT, WT, V1 = B.OUT, B.WT, B.V1
env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
sha = B.sha


def sh(argv, **kw):
    return subprocess.run([str(a) for a in argv], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, **kw)


gen = B.load_mod('g', WT / 'scripts/generate-boot-config.py')
res = {'checks': [], 'ok': True}


def check(name, ok, detail=None):
    res['checks'].append({'name': name, 'ok': bool(ok), 'detail': detail})
    if not ok:
        res['ok'] = False


def sha_b(b):
    return hashlib.sha256(b).hexdigest()


tmp = Path(tempfile.mkdtemp(prefix='b2v2-verify-'))
files = gen.image_files(OUT / 'desktop-kernel_fs.img')
base = gen.image_files(OUT / 'desktop-base-tracked-kernel_fs.img')
v1 = gen.image_files(V1 / 'desktop-kernel_fs.img')

dest_of = {l: d for l, _p, _e, _b, d in B.BUILDS}
dest_of.update({b: d for b, d in B.SYS_TOOLS})
new_paths = {'/bin/desktop', '/bin/files', '/bin/terminal', '/bin/chrome'}
check('image file set == tracked base file set + {desktop,files,terminal,chrome}',
      set(files) == set(base) | new_paths, {'extra': sorted(set(files) - set(base) - new_paths),
                                             'missing': sorted(set(base) - set(files))})
for label, dest in sorted(dest_of.items()):
    check(f'{dest} bytes == signed source-built {label}', files.get(dest) == (OUT / 'signed' / label).read_bytes(),
          sha_b(files[dest]) if dest in files else None)
for dest in B.TRACKED_KEPT:
    check(f'{dest} unchanged tracked historical bytes (deliberately not rebuilt)', files.get(dest) == base.get(dest),
          sha_b(base[dest]))
others = sorted(set(base) - set(dest_of.values()) - set(B.TRACKED_KEPT) - {'/etc/cellos/autoload.toml'})
check('all other tracked files byte-identical to base (data files, toml configs)',
      all(files[p] == base[p] for p in others), others)
check('/etc/cellos/autoload.toml == package-desktop-x86.py autoload (desktop + shell)',
      files['/etc/cellos/autoload.toml'] == B.AUTOLOAD)
# six B2 bytes equal the S10-4 B2 / S10-7.1 signed bytes
six = {'/bin/shell': 'app-shell', '/bin/ls': 'ls', '/bin/cat': 'cat', '/bin/echo': 'echo', '/bin/ps': 'ps',
       '/bin/kill': 'kill'}
for dest, b in six.items():
    check(f'{dest} == S10-7.1 signed B2 {b} (S10-4 B2 hash)', files[dest] == (V1 / 'signed' / f'headless-{b}').read_bytes(),
          sha_b(files[dest]))
# reproducibility vs the 7.1 builds where the same unsigned artifact existed
repro = {}
for label, v1name in [('app-shell', 'headless-app-shell'), ('ls', 'headless-ls'), ('cat', 'headless-cat'),
                      ('echo', 'headless-echo'), ('ps', 'headless-ps'), ('kill', 'headless-kill'),
                      ('service-vfs', 'headless-service-vfs'), ('service-config', 'headless-service-config'),
                      ('service-input', 'headless-service-input'), ('service-platform', 'headless-platform'),
                      ('driver-nvme', 'headless-driver-nvme'), ('driver-e1000', 'headless-driver-e1000'),
                      ('driver-igb', 'headless-driver-igb'), ('driver-ahci', 'headless-driver-ahci'),
                      ('driver-xhci', 'headless-driver-xhci'), ('driver-serial-x86', 'headless-driver-serial-x86'),
                      ('ocel-desktop', 'desktop-ocel'), ('service-net-qemu-dev-https', 'desktop-service-net')]:
    old_u = V1 / 'unsigned' / v1name
    old_s = V1 / 'signed' / v1name
    repro[label] = {'unsigned_equals_7.1': old_u.is_file() and sha(OUT / 'unsigned' / label) == sha(old_u),
                    'signed_equals_7.1': old_s.is_file() and sha(OUT / 'signed' / label) == sha(old_s)}
res['reproducibility_vs_s10_7_1'] = repro
check('every rebuilt artifact also built in 7.1 is byte-identical (unsigned and signed)',
      all(v['unsigned_equals_7.1'] and v['signed_equals_7.1'] for v in repro.values()),
      {k: v for k, v in repro.items() if not (v['unsigned_equals_7.1'] and v['signed_equals_7.1'])})

# root-cause regression: vfs heap vs ocel size
def load_segs(data):
    phoff, = struct.unpack_from('<Q', data, 32)
    phentsize, phnum = struct.unpack_from('<HH', data, 54)
    segs = []
    for i in range(phnum):
        t, fl, off, va, pa, fsz, msz, al = struct.unpack_from('<IIQQQQQQ', data, phoff + i * phentsize)
        if t == 1:
            segs.append({'vaddr': va, 'filesz': fsz, 'memsz': msz, 'flags': fl})
    return segs


def heap_memsz(data):
    return max((s['memsz'] for s in load_segs(data) if s['filesz'] == 0), default=0)


heaps = {'tracked_vfs': heap_memsz(base['/bin/vfs']), 'v2_vfs': heap_memsz(files['/bin/vfs']),
         'v1_vfs': heap_memsz(v1['/bin/vfs'])}
res['vfs_heap_memsz_bytes'] = heaps
res['ocel_bytes'] = {'tracked': len(base['/bin/ocel']), 'rebuilt': len(files['/bin/ocel'])}
check('rebuilt vfs heap segment >= 16 MiB (VFS_HEAP_BYTES) and > rebuilt ocel size 4341072',
      heaps['v2_vfs'] >= 16 * 1024 * 1024 and heaps['v2_vfs'] > len(files['/bin/ocel']), heaps)
# [INFERENCE] tracked vfs RW-zero segment = 4 MiB heap + 1 MiB (stack/guard) = 5 MiB+0x40; the OOM size 4341072 is
# exactly the rebuilt ocel ELF size and exceeds a 4 MiB heap (declare_custom_heap!(VFS_HEAP_BYTES) is 16 MiB in source)
check('7.1/7.2 failure cause visible statically: tracked vfs RW segment (5 MiB+64) leaves a 4 MiB heap < rebuilt ocel size '
      '4341072; source heap is 16 MiB',
      heaps['tracked_vfs'] - (1 << 20) < len(files['/bin/ocel']) < heaps['v2_vfs'] - (1 << 20), heaps)

# signature verification of every ELF in the image
profile = {'file_count': len(files), 'files': {}}
for dest in sorted(files):
    data = files[dest]
    rec = {'bytes': len(data), 'sha256': sha_b(data)}
    if data[:4] == b'\x7fELF':
        p = tmp / dest.replace('/', '_')
        p.write_bytes(data)
        r = sh(['python3', WT / 'scripts/sign-cell.py', '--verify', '--in', p])
        rec['elf'] = True
        rec['signature_verify_rc'] = r.returncode
        rec['signature_verify_out'] = r.stdout.decode(errors='replace').strip()[-200:]
        rec['origin'] = ('tracked-historical' if dest in B.TRACKED_KEPT else 'source-built-signed-v2')
    else:
        rec['origin'] = 'tracked' if files[dest] == base.get(dest) else 'v2-generated'
    profile['files'][dest] = rec
failing = [d for d, r in profile['files'].items() if r.get('elf') and r['signature_verify_rc'] != 0]
res['unsigned_tracked_test_fixtures'] = failing
check('every ELF in the image passes the historical dev-signature verify except the two tracked tier2 test fixtures '
      '(unsigned in the tracked base and 7.1; not spawned at boot, signing-required would deny them)',
      sorted(failing) == ['/bin/tier2-exploit', '/bin/tier2-smoke'], failing)
res['image'] = profile
res['unchanged_from_7_1_desktop_image'] = sorted(p for p in files if v1.get(p) == files[p])
res['changed_vs_7_1_desktop_image'] = sorted(p for p in files if v1.get(p) != files[p])
check('/POLICY.BIN, services.toml, system.toml present',
      all(c in files for c in ('/POLICY.BIN', '/etc/cellos/services.toml', '/etc/cellos/system.toml')))
check('shell ELF carries the "Cellos > " prompt literal', b'Cellos > ' in files['/bin/shell'])
for dest in ('/bin/desktop', '/bin/terminal', '/bin/files', '/bin/chrome', '/bin/compositor', '/bin/fb-console',
             '/bin/sshd', '/bin/vfs', '/bin/net', '/bin/input'):
    check(f'{dest} present', dest in files)

# kernels
init_bytes = (OUT / 'desktop-init').read_bytes()
img_bytes = (OUT / 'desktop-kernel_fs.img').read_bytes()
check('init == tracked init (not a service; unchanged)', init_bytes == (OUT / 'desktop-base-tracked-init').read_bytes())
r = sh(['python3', WT / 'scripts/sign-cell.py', '--verify', '--in', OUT / 'desktop-init'])
check('init dev signature verifies', r.returncode == 0, r.stdout.decode(errors='replace').strip()[-200:])
for tag, kfile in (('non-hooks', 'desktop-kernel.elf'), ('test-hooks', 'desktop-test-hooks-kernel.elf')):
    with open(OUT / kfile, 'rb') as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        a = m.find(img_bytes)
        b = m.find(img_bytes, a + 1) if a >= 0 else -1
        c = m.find(init_bytes)
    check(f'{tag} kernel embeds v2 kernel_fs.img byte-for-byte (single copy)', a >= 0 and b < 0, a)
    check(f'{tag} kernel embeds init byte-for-byte', c >= 0, c)
    with open(OUT / kfile, 'rb') as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        check(f'{tag} kernel does NOT embed the tracked historical kernel_fs.img',
              m.find((OUT / 'desktop-base-tracked-kernel_fs.img').read_bytes()) < 0)
    r = sh(['readelf', '-hlW', OUT / kfile])
    (OUT / 'logs' / f'{tag}-kernel-readelf.txt').write_bytes(r.stdout)
    t = r.stdout.decode()
    check(f'{tag} kernel is ELF64 x86-64 EXEC', 'Class:                             ELF64' in t and
          'Advanced Micro Devices X86-64' in t and 'EXEC (Executable file)' in t)
check('test-hooks kernel differs from non-hooks kernel', sha(OUT / 'desktop-kernel.elf') != sha(OUT / 'desktop-test-hooks-kernel.elf'),
      {'nonhooks': sha(OUT / 'desktop-kernel.elf'), 'hooks': sha(OUT / 'desktop-test-hooks-kernel.elf')})
check('non-hooks kernel differs from the 7.1 desktop kernel (different embedded image)',
      sha(OUT / 'desktop-kernel.elf') != sha(V1 / 'desktop-kernel.elf'))


def tree_of(iso, dest):
    r = sh(['xorriso', '-osirrox', 'on', '-indev', iso, '-extract', '/', dest])
    out = {}
    for p in sorted(Path(dest).rglob('*')):
        if p.is_file():
            out[p.relative_to(dest).as_posix()] = sha(p)
    return out, r


trees = {}
for tag, iso, kfile in (('non-hooks', 'desktop-cellos-x86_64-desktop', 'desktop-kernel.elf'),
                        ('test-hooks', 'desktop-test-hooks-cellos-x86_64-desktop', 'desktop-test-hooks-kernel.elf')):
    t, r = tree_of(OUT / f'{iso}.iso', tmp / f'iso-{tag}')
    (OUT / 'logs' / f'{tag}-iso-files.txt').write_text(''.join(f'{h}  {p}\n' for p, h in sorted(t.items())))
    trees[tag] = t
    check(f'{tag} ISO /boot/kernel.elf == kernel ELF', t.get('boot/kernel.elf') == sha(OUT / kfile))
    check(f'{tag} ISO /boot/limine.conf == scripts/x86/limine.conf', t.get('boot/limine.conf') == sha(WT / 'scripts/x86/limine.conf'))
    # qcow2 round trip == bios-installed ISO
    q = OUT / f'{iso}.qcow2'
    rr = sh(['qemu-img', 'info', '--output=json', q])
    check(f'{tag} qcow2 info is qcow2', b'"format": "qcow2"' in rr.stdout)
    raw = tmp / f'{tag}.raw'
    sh(['qemu-img', 'convert', '-f', 'qcow2', '-O', 'raw', q, raw])
    check(f'{tag} qcow2 round-trips to the bios-installed ISO', raw.is_file() and sha(raw) == sha(OUT / f'{iso}.iso'))
    with open(raw, 'rb') as f:
        mbr = f.read(512)
    check(f'{tag} disk LBA0 has 0x55AA boot signature', mbr[510:512] == b'\x55\xaa')
    raw.unlink()
    rc = sh(['qemu-img', 'check', q])
    check(f'{tag} qcow2 qemu-img check clean', rc.returncode == 0, rc.stdout.decode(errors='replace').strip()[-120:])
diff = sorted(p for p in set(trees['non-hooks']) | set(trees['test-hooks']) if trees['non-hooks'].get(p) != trees['test-hooks'].get(p))
check('hooks ISO tree differs from non-hooks ISO tree ONLY in boot/kernel.elf', diff == ['boot/kernel.elf'], diff)
# build commands: features, locked/offline, pinned toolchain
cmds = json.loads(B.CMDS.read_text())
res['build_command_labels'] = [c['label'] for c in cmds]
cargo_cmds = [c for c in cmds if len(c['argv']) > 1 and c['argv'][0] == 'cargo' and c['argv'][2] == 'build']
check('every cargo build used +nightly-2026-05-01 --release --locked --offline',
      all(c['argv'][1] == '+nightly-2026-05-01' and '--locked' in c['argv'] and '--offline' in c['argv'] and '--release' in c['argv'] for c in cargo_cmds),
      len(cargo_cmds))
k_plain = next(c for c in cmds if c['label'] == '300-build-kernel-desktop')['argv']
k_hooks = next(c for c in cmds if c['label'] == '301-build-kernel-desktop-test-hooks')['argv']
check('hooks kernel command differs from non-hooks only by feature test-hooks',
      k_hooks[:-1] == k_plain[:-1] and k_hooks[-1] == k_plain[-1] + ',test-hooks', [k_plain[-1], k_hooks[-1]])
check('test-hooks appears on the kernel build only',
      all('test-hooks' not in ' '.join(c['argv']) for c in cargo_cmds if c['label'] != '301-build-kernel-desktop-test-hooks'))
shutil.rmtree(tmp)
(OUT / 'static-verification.json').write_text(json.dumps(res, indent=2, sort_keys=True) + '\n')
print('STATIC OK' if res['ok'] else 'STATIC FAIL', len(res['checks']), 'checks')
for c in res['checks']:
    if not c['ok']:
        print('FAILED:', c)
sys.exit(0 if res['ok'] else 1)
