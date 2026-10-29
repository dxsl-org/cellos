#!/usr/bin/env python3
import hashlib, json, os, subprocess, sys, time
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


def out(argv, cwd=WT):
    return subprocess.run([str(a) for a in argv], cwd=cwd, env=env, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT).stdout.decode(errors='replace')


# extra init signature verification (init is embedded separately, not in the FAT image)
init_verify = {}
for n in ('headless-init', 'desktop-init', 'desktop-base-tracked-init'):
    r = subprocess.run(['python3', str(WT / 'scripts/sign-cell.py'), '--verify', '--in', str(OUT / n)],
                       env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    init_verify[n] = {'rc': r.returncode, 'out': r.stdout.decode(errors='replace').strip()[-200:]}

git_status = out(['git', 'status', '--porcelain', '--untracked-files=all'])
git_diff_names = out(['git', 'diff', '--name-status'])
git_tree = out(['git', 'rev-parse', 'HEAD^{tree}']).strip()
(OUT / 'patches' / 'all-deltas-in-worktree.patch').write_bytes(
    subprocess.run(['git', 'diff', '--binary'], cwd=WT, stdout=subprocess.PIPE, check=True).stdout)

sign_tools = ['scripts/cellos-sign', 'scripts/sign-cell.py', 'scripts/unsafe-allowlist.toml',
              'scripts/sign-policy.py', 'tools/mkfat32.py', 'tools/inspect_fat.py',
              'scripts/generate-boot-config.py', 'scripts/package-ocel-profile.py',
              'scripts/x86/make-iso-ci.sh', 'scripts/x86/limine.conf', 'scripts/build-x86_64-cells.ps1',
              'run-desktop.sh', 'scripts/qemu-x86_64-test.sh', 'scripts/lib-sign-cells.sh',
              'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'kernel/Cargo.toml',
              'limine/limine-8.7.0/bin/limine-bios-cd.bin', 'limine/limine-8.7.0/bin/limine-bios.sys',
              'limine/limine-8.7.0/bin/limine-uefi-cd.bin', 'limine/limine-8.7.0/bin/BOOTX64.EFI',
              'limine/limine-8.7.0/bin/limine.c', 'limine/limine-8.7.0/bin/limine']
sign_tools += sorted(p.relative_to(WT).as_posix() for p in (WT / 'scripts/cellos_sign').glob('*.py'))
src = {rel: sha(WT / rel) for rel in sign_tools}
orig = {}
for rel in ('Cargo.toml', 'Cargo.lock'):
    orig[rel] = hashlib.sha256(subprocess.run(['git', 'show', f'{HEAD}:{rel}'], cwd=WT,
                                              stdout=subprocess.PIPE, check=True).stdout).hexdigest()

versions = {'rustc': out(['rustc', '+nightly-2026-05-01', '-Vv']), 'cargo': out(['cargo', '+nightly-2026-05-01', '-V']),
            'python3': out(['python3', '--version']), 'cc': out(['cc', '--version']).splitlines()[0],
            'objcopy': out(['objcopy', '--version']).splitlines()[0],
            'xorriso': out(['xorriso', '-version']).splitlines()[0:2],
            'qemu-img': out(['qemu-img', '--version']).splitlines()[0],
            'cryptography': out(['python3', '-c', 'import cryptography;print(cryptography.__version__)']).strip()}

cmds = json.loads((W / 'state/commands.json').read_text())

artifacts = {}
for p in sorted(OUT.rglob('*')):
    if p.is_file() and p.name not in ('manifest.json', 'result.json', 'evidence.sha256'):
        artifacts[p.relative_to(OUT).as_posix()] = {'bytes': p.stat().st_size, 'sha256': sha(p)}

boot_disks = {
    'headless': {'iso': 'headless-vicell-x86.iso', 'qcow2': 'headless-vicell-x86.qcow2',
                 'kernel': 'headless-kernel.elf', 'init': 'headless-init', 'kernel_fs': 'headless-kernel_fs.img',
                 'qemu_boot_command_from_scripts/qemu-x86_64-test.sh (NOT RUN)':
                 'qemu-system-x86_64 -machine q35 -cpu qemu64,+pdpe1gb -m 256M -nographic -cdrom headless-vicell-x86.iso -boot d -no-reboot'},
    'desktop': {'iso': 'desktop-cellos-x86_64-desktop.iso', 'qcow2': 'desktop-cellos-x86_64-desktop.qcow2',
                'kernel': 'desktop-kernel.elf', 'init': 'desktop-init', 'kernel_fs': 'desktop-kernel_fs.img',
                'qemu_boot_command_from_run-desktop.sh (NOT RUN)':
                'qemu-system-x86_64 -cpu qemu64,+pdpe1gb -machine q35,vmport=on -m 2048M -smp 2 -drive file=desktop-cellos-x86_64-desktop.qcow2,format=qcow2,if=virtio -netdev user,id=net0,hostfwd=tcp::2222-:22 -device e1000,netdev=net0 -device virtio-keyboard-pci -device virtio-tablet-pci -device virtio-mouse-pci -display gtk,zoom-to-fit=on -vnc 127.0.0.1:0 -qmp unix:<sock>,server,nowait -serial mon:stdio'},
}

manifest = {
    'status': 'BUILT',
    'purpose': 'S10-7.1 option (a): historical-matched runtime comparator B2 (non-pristine) at commit ' + HEAD,
    'source': {'commit': HEAD, 'tree': git_tree, 'worktree': str(WT), 'git_status_after_all_steps': git_status,
               'git_diff_name_status': git_diff_names,
               'original_sha256_at_commit': orig},
    'patches': {n: {'sha256': sha(OUT / 'patches' / n)} for n in sorted(os.listdir(OUT / 'patches'))},
    'authorized_deltas': ['b2-manifest.patch (drop absent cells/apps/vm-runner member)',
                          'b2-lock.patch (offline lock reconciliation: app-chrome, files, terminal)',
                          'F1 scoped-signing exception (D7-4): signing via cellos-sign --repo <scoped mirror root>; '
                          'whole-workspace F1 still REFUSES (retained logs/whole-workspace-check.log)'],
    'deltas_beyond_authorized': [],
    'toolchain_and_tools': versions,
    'tooling_and_source_hashes': src,
    'init_signature_verify': init_verify,
    'boot_disks': boot_disks,
    'commands': cmds,
    'driver_scripts': {n: sha(W / 'driver' / n) for n in ('b2.py', 'verify.py', 'manifest.py')},
    'artifacts': artifacts,
}
(OUT / 'manifest.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
print('manifest written', len(artifacts), 'artifacts')
print(git_status)
print(git_diff_names)
print(json.dumps(init_verify, indent=1))
