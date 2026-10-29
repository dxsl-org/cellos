#!/usr/bin/env python3
"""Build a diagnostic-only platform yield probe; retain all candidate inputs read-only."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
B = Path('/tmp/cellbox-s10-3d-investigation')
R = Path('/home/dmin/cellos')
C = Path('/tmp/cellbox-s10-3c-resume/cand')
O = B / (sys.argv[1] if len(sys.argv) > 1 else 'instrumented')
O.mkdir(exist_ok=False)
env = dict(os.environ, CARGO_TARGET_DIR=str(B / 'build-target'), RUSTFLAGS='-C relocation-model=pic')
env.pop('CARGO_ENCODED_RUSTFLAGS', None)
with (O / 'build.log').open('w') as out:
    subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path', str(B / 'platform-probe/Cargo.toml'),
                    '--target', 'x86_64-unknown-none', '-Z', 'build-std=core,alloc'], cwd=R, env=env,
                   stdout=out, stderr=subprocess.STDOUT, check=True)
platform = O / 'platform'
shutil.copy2(B / 'build-target/x86_64-unknown-none/release/platform', platform)
nvme = None
if len(sys.argv) > 2 and sys.argv[2] == 'delay-nvme':
    with (O / 'build-nvme.log').open('w') as out:
        subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path',
                        str(B / 'nvme-probe/Cargo.toml'), '--target', 'x86_64-unknown-none',
                        '-Z', 'build-std=core,alloc'], cwd=R, env=env,
                       stdout=out, stderr=subprocess.STDOUT, check=True)
    nvme = O / 'driver-nvme'
    shutil.copy2(B / 'build-target/x86_64-unknown-none/release/driver-nvme', nvme)
    shutil.copy2(B / 'nvme-probe/src/main.rs', O / 'nvme-main.rs')
with (O / 'sign.log').open('w') as out:
    subprocess.run([sys.executable, str(R / 'scripts/cellos-sign'), '--sign', str(platform),
                    *([str(nvme)] if nvme else [])], cwd=R,
                   stdout=out, stderr=subprocess.STDOUT, check=True)
candidate = json.loads((C / 'candidate.json').read_text())
profile = candidate['profiles']['headless']
command = [sys.executable, str(R / 'tools/mkfat32.py'), '--config-features', 'input',
           '--config-output-dir', str(O / 'boot-config'), str(O / 'kernel_fs.img')]
for destination, entry in profile['files'].items():
    if destination.startswith('/etc/cellos/'): continue
    replacement = platform if destination == '/bin/platform' else nvme if destination == '/bin/nvme' else None
    command.extend([str(replacement) if replacement else entry['file'], destination])
with (O / 'image.log').open('w') as out:
    subprocess.run(command, cwd=R, stdout=out, stderr=subprocess.STDOUT, check=True)
old_image = (C / 'headless/kernel_fs.img').read_bytes()
new_image = (O / 'kernel_fs.img').read_bytes()
old_kernel = (C / 'headless.kernel.elf').read_bytes()
assert len(old_image) == len(new_image), (len(old_image), len(new_image))
assert old_kernel.count(old_image) == 1
# Diagnostic image substitution only; this is never candidate or qualification evidence.
(O / 'kernel.elf').write_bytes(old_kernel.replace(old_image, new_image))
with (O / 'iso.log').open('w') as out:
    subprocess.run(['bash', str(R / 'scripts/x86/make-iso-ci.sh'), str(O / 'probe.iso')], cwd=R,
                   env=dict(os.environ, X86_KERNEL=str(O / 'kernel.elf'), X86_ISO_ROOT=str(O / 'iso-root')),
                   stdout=out, stderr=subprocess.STDOUT, check=True)
sha = lambda p: hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()
result = {'diagnostic_only': True, 'original_candidate_sha256': sha(C / 'candidate.json'),
          'original_iso_sha256': sha(C / 'headless.iso'), 'probe_iso_sha256': sha(O / 'probe.iso'),
          'original_nvme_sha256': sha(C / 'headless/files/nvme'),
          'instrumented_nvme_sha256': sha(nvme) if nvme else None,
          'instrumented_platform_sha256': sha(platform),
          'change': 'Only disposable platform-probe sources change; exact source copied beside this provenance.'}
(O / 'provenance.json').write_text(json.dumps(result, indent=2) + '\n')
shutil.copy2(B / 'platform-probe/src/scan.rs', O / 'scan.rs')
print(json.dumps(result, indent=2))
