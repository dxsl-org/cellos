#!/usr/bin/env python3
"""S10-7.4 pre-run input preparation (NO VM, NO QEMU): per-set fixture NVMe disk and, for desktop, the host glyph oracle.

    PYTHONDONTWRITEBYTECODE=1 python3 -B s107_prepare.py cand-desktop|cand-headless|b2-headless

Mirrors scripts/qemu-phase10-canonical-test.sh (main fixture disk: phase07_rg_fixture.py + the phase10 FAT data tree via
mcopy with read-back; glyph renderer built offline into a private target dir) but writes only under
`/tmp/cellbox-s10-7/measure/<set>/run-inputs/`.  b2-headless needs neither (the B2 shared subset reads the boot image
only; the run drops the NVMe data disk).  The rebuilt glyph oracle must hash to the retained `GLYPH_SHA`.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402

GLYPH_SHA = '820c92d2dc0aa5c1f50533a42283974b07998111adaff8e416f56dd4481eec79'   # retained common.GLYPH_SHA
SCRIPTS = ENV.REPO / 'scripts'


def run(argv, **kwargs):
    return subprocess.run([str(a) for a in argv], check=True, **kwargs)


def build_glyph(spec):
    base = spec.run_inputs
    host = base / 'glyph-host'
    (host / 'src').mkdir(parents=True, exist_ok=True)
    template = (SCRIPTS / 'phase04-gui-glyphs/Cargo.toml.in').read_text().replace('@ROOT@', str(ENV.REPO))
    (host / 'Cargo.toml').write_text(template)
    shutil.copyfile(SCRIPTS / 'phase04-gui-glyphs/src/main.rs', host / 'src/main.rs')
    shutil.copyfile(ENV.REPO / 'Cargo.lock', host / 'Cargo.lock')
    env = dict(os.environ, CARGO_TARGET_DIR=str(base / 'host-target'))
    run(['cargo', 'build', '--release', '--offline', '--target', 'x86_64-unknown-linux-gnu', '--manifest-path', host / 'Cargo.toml'], env=env)
    got = ENV.sha256_file(spec.glyph_bin)
    if got != GLYPH_SHA:
        raise ENV.EnvError(f'rebuilt glyph oracle {got} != retained GLYPH_SHA {GLYPH_SHA}')
    return got


def build_fixture_disk(spec):
    base = spec.run_inputs
    disk_dir = base / 'disk-main'
    shutil.rmtree(disk_dir, ignore_errors=True)
    shutil.rmtree(base / 'fixtures', ignore_errors=True)
    disk_dir.mkdir(parents=True)
    bootcfg = ENV.CAND_CACHE / spec.profile / 'boot-config'
    record, files = ENV.candidate_files(spec.profile)
    for name in ('autoload', 'services', 'system'):
        want = files[f'/etc/cellos/{name}.toml']['sha256']
        if ENV.sha256_file(bootcfg / f'{name}.toml') != want:
            raise ENV.EnvError(f'boot-config {name}.toml differs from candidate.json')
    disk = spec.fixture_disk
    run([sys.executable, SCRIPTS / 'phase07_rg_fixture.py', disk, disk_dir / 'rg-fixture-expected.json', bootcfg])
    data = base / 'fixtures/data'
    data.mkdir(parents=True)
    manifest = base / 'data-fixtures.json'
    run([sys.executable, SCRIPTS / 'phase10_canonical_drive.py', 'fixtures', data, '--manifest', manifest])
    target = f'{disk}@@{2048 * 512}'
    env = dict(os.environ, MTOOLS_SKIP_CHECK='1')
    for top in sorted(data.iterdir()):
        run(['mcopy', '-s', '-i', target, top, '::/'], env=env)
    listing = json.loads(manifest.read_text())
    for guest, item in listing.items():
        rel = guest[len('/mnt/sd/'):]
        got = subprocess.run(['mcopy', '-n', '-i', target, '::/' + rel, '-'], check=True, capture_output=True, env=env).stdout
        if hashlib.sha256(got).hexdigest() != item['sha256'] or len(got) != item['length']:
            raise ENV.EnvError(f'{guest} reads back different bytes from the fixture disk')
    return {'disk': str(disk), 'sha256': ENV.sha256_file(disk), 'data_files_read_back': len(listing),
            'boot_config': str(bootcfg), 'rg_expected_sha256': ENV.sha256_file(disk_dir / 'rg-fixture-expected.json')}


def prepare(name):
    spec = ENV.SETS[name]
    result = {'set': name, 'run_inputs': str(spec.run_inputs)}
    spec.run_inputs.mkdir(parents=True, exist_ok=True)
    if spec.kind == 'b2':
        result['note'] = 'b2-headless needs no fixture disk and no glyph oracle'
    else:
        result['fixture'] = build_fixture_disk(spec)
        if spec.profile == 'desktop':
            result['glyph_sha256'] = build_glyph(spec)
    (spec.run_inputs / 'prepare-result.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


if __name__ == '__main__':
    if len(sys.argv) != 2 or sys.argv[1] not in ENV.SET_NAMES:
        raise SystemExit(__doc__)
    print(json.dumps(prepare(sys.argv[1]), indent=2))
