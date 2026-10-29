#!/usr/bin/env python3
"""No-VM static boot-readiness verification + manifest for the B2 comparator images."""
import hashlib, importlib.util, json, mmap, os, shutil, subprocess, sys, tempfile
from pathlib import Path

sys.dont_write_bytecode = True
W = Path('/tmp/cellbox-s10-7')
WT = W / 'worktree'
OUT = W / 'b2-image'
HEAD = '3471028bc8b4db14275553b4740c17e24fa4f32b'
env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')


def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def sh(argv, **kw):
    return subprocess.run([str(a) for a in argv], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, **kw)


spec = importlib.util.spec_from_file_location('g', WT / 'scripts/generate-boot-config.py')
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)

six = {'/bin/shell': 'app-shell', '/bin/ls': 'ls', '/bin/cat': 'cat', '/bin/echo': 'echo',
       '/bin/ps': 'ps', '/bin/kill': 'kill'}
res = {'checks': [], 'ok': True}


def check(name, ok, detail=None):
    res['checks'].append({'name': name, 'ok': bool(ok), 'detail': detail})
    if not ok:
        res['ok'] = False


tmp = Path(tempfile.mkdtemp(prefix='b2-verify-'))
profiles = {}
for prof, img, kern, iso in [('headless', 'headless-kernel_fs.img', 'headless-kernel.elf', 'headless-vicell-x86.iso'),
                             ('desktop', 'desktop-kernel_fs.img', 'desktop-kernel.elf',
                              'desktop-cellos-x86_64-desktop.iso')]:
    files = gen.image_files(OUT / img)
    prof_info = {'image': img, 'file_count': len(files), 'files': {}}
    # six tools present, byte-equal to the signed B2 build outputs
    for dest, bin_ in six.items():
        present = dest in files
        check(f'{prof}: {dest} present in kernel_fs.img', present)
        if present:
            want = (OUT / 'signed' / f'headless-{bin_}').read_bytes()
            check(f'{prof}: {dest} bytes == signed B2 {bin_}', files[dest] == want,
                  hashlib.sha256(files[dest]).hexdigest())
    # signature verify every ELF in the image with the historical signer
    for dest in sorted(files):
        data = files[dest]
        rec = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
        if data[:4] == b'\x7fELF':
            p = tmp / (prof + dest.replace('/', '_'))
            p.write_bytes(data)
            r = sh(['python3', WT / 'scripts/sign-cell.py', '--verify', '--in', p])
            rec['elf'] = True
            rec['signature_verify_rc'] = r.returncode
            rec['signature_verify_out'] = r.stdout.decode(errors='replace').strip()[-200:]
        prof_info['files'][dest] = rec
    for dest in six:
        if dest in prof_info['files']:
            check(f'{prof}: {dest} dev signature verifies', prof_info['files'][dest].get('signature_verify_rc') == 0,
                  prof_info['files'][dest].get('signature_verify_out'))
    unsigned_elfs = [d for d, r in prof_info['files'].items() if r.get('elf') and r['signature_verify_rc'] != 0]
    prof_info['elfs_failing_signature_verify'] = unsigned_elfs
    check(f'{prof}: /POLICY.BIN present', '/POLICY.BIN' in files)
    for cfg in ('/etc/cellos/autoload.toml', '/etc/cellos/services.toml', '/etc/cellos/system.toml'):
        check(f'{prof}: {cfg} present', cfg in files)
    # prompt string in shell
    check(f'{prof}: shell ELF carries the "Cellos > " prompt literal', b'Cellos > ' in files['/bin/shell'])
    # embedded in kernel: image and init byte-exact inside the kernel ELF
    kbytes = OUT / kern
    with open(kbytes, 'rb') as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        img_bytes = (OUT / img).read_bytes()
        init_bytes = (OUT / ('headless-init' if prof == 'headless' else 'desktop-init')).read_bytes()
        pos_img = m.find(img_bytes)
        pos_init = m.find(init_bytes)
    check(f'{prof}: kernel ELF embeds kernel_fs.img byte-for-byte', pos_img >= 0, pos_img)
    check(f'{prof}: kernel ELF embeds init byte-for-byte', pos_init >= 0, pos_init)
    r = sh(['readelf', '-hlW', kbytes])
    (OUT / 'logs' / f'{prof}-kernel-readelf.txt').write_bytes(r.stdout)
    txt = r.stdout.decode()
    check(f'{prof}: kernel is ELF64 x86-64 EXEC', 'Class:                             ELF64' in txt and
          'Advanced Micro Devices X86-64' in txt and 'EXEC (Executable file)' in txt)
    # ISO content
    ex = tmp / (prof + '-kernel.elf')
    r = sh(['xorriso', '-osirrox', 'on', '-indev', OUT / iso, '-extract', '/boot/kernel.elf', ex,
            '-extract', '/boot/limine.conf', tmp / (prof + '-limine.conf')])
    (OUT / 'logs' / f'{prof}-xorriso-extract.log').write_bytes(r.stdout)
    check(f'{prof}: ISO /boot/kernel.elf == kernel ELF', ex.is_file() and sha(ex) == sha(kbytes), sha(ex) if ex.is_file() else None)
    check(f'{prof}: ISO /boot/limine.conf == scripts/x86/limine.conf',
          (tmp / (prof + '-limine.conf')).is_file() and sha(tmp / (prof + '-limine.conf')) == sha(WT / 'scripts/x86/limine.conf'))
    r = sh(['xorriso', '-indev', OUT / iso, '-find', '/', '-type', 'f'])
    (OUT / 'logs' / f'{prof}-iso-files.txt').write_bytes(r.stdout)
    profiles[prof] = prof_info

# qcow2 disks: round trip to raw equals the bios-installed ISO
for prof, qc, raw_src in [('desktop', 'desktop-cellos-x86_64-desktop.qcow2', 'desktop-cellos-x86_64-desktop.iso'),
                          ('headless', 'headless-vicell-x86.qcow2', 'headless-vicell-x86.bios-installed.iso')]:
    r = sh(['qemu-img', 'info', '--output=json', OUT / qc])
    info = json.loads(r.stdout)
    rawp = tmp / (prof + '.raw')
    sh(['qemu-img', 'convert', '-f', 'qcow2', '-O', 'raw', OUT / qc, rawp])
    check(f'{prof}: qcow2 round-trips to the limine-bios-installed ISO', rawp.is_file() and sha(rawp) == sha(OUT / raw_src))
    r = sh(['qemu-img', 'check', OUT / qc])
    check(f'{prof}: qcow2 qemu-img check clean', r.returncode == 0, r.stdout.decode(errors='replace').strip()[-120:])
    res.setdefault('qcow2', {})[prof] = {'format': info.get('format'), 'virtual_size': info.get('virtual-size')}
    # Limine BIOS stage1 marker: boot signature on LBA0
    with open(rawp, 'rb') as f:
        mbr = f.read(512)
    check(f'{prof}: disk LBA0 has 0x55AA boot signature', mbr[510:512] == b'\x55\xaa')

# headless vs desktop kernel differ only as intended: record feature sets from the build commands
cmds = json.loads((W / 'state/commands.json').read_text())
res['build_command_labels'] = [c['label'] for c in cmds]
res['profiles'] = profiles
(OUT / 'static-verification.json').write_text(json.dumps(res, indent=2, sort_keys=True) + '\n')
shutil.rmtree(tmp)
print('STATIC OK' if res['ok'] else 'STATIC FAIL')
for c in res['checks']:
    if not c['ok']:
        print('FAILED:', c)
sys.exit(0 if res['ok'] else 1)
