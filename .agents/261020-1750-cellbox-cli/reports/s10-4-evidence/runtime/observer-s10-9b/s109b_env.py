"""S10-9b environment: the ONE B2 desktop (v2 image) measurement set, its run token, evidence roots and private work root.

Everything generic (original deadlines, ELF helpers, path layout) is the retained S10-7 module `s107_env`, imported unchanged
(through the unchanged S10-8/S10-9 modules `s108_env`/`s109_env`).  S10-7/8/9 `RUN_CONSUMED-*` tokens and evidence roots are never
touched: this run uses `RUN_CONSUMED-s109-b2-desktop` next to this file and `reports/s10-9-evidence/b2-desktop-run/`.

The set is NOT a `s107_env.SetSpec` (those live under `/tmp/cellbox-s10-7/measure/<name>` and the old b2-desktop entry there is the
failed 7.1 image).  The v2 artifacts are the read-only S10-9.3 build outputs in `/tmp/cellbox-s10-7/b2-image-v2/`; their sha256
values are pinned here AND re-read from that directory's `manifest.json`.
"""
import json
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
for _name in ('observer-s10-9', 'observer-s10-8', 'observer-s10-7'):
    sys.path.insert(0, str(HERE.parent / _name))
import s109_env as ENV9  # noqa: E402
import s107_env as E7  # noqa: E402

RUNTIME = E7.RUNTIME
REPORTS = E7.REPORTS
PLAN = E7.PLAN
REPO = E7.REPO
EnvError = E7.EnvError
DEADLINES = E7.DEADLINES
sha256_file = E7.sha256_file
dump = E7.dump
elf_symbols, unique_symbol, elf_load_segments = E7.elf_symbols, E7.unique_symbol, E7.elf_load_segments
kernel_has_test_hooks = E7.kernel_has_test_hooks

EVIDENCE = REPORTS / 's10-9-evidence' / 'b2-desktop-run'
HARNESS_EVIDENCE = EVIDENCE / 'harness'
RUN_EVIDENCE = EVIDENCE / 'measurement-run'          # created by the run only (its absence is part of the go/no-go)
WORK_ROOT = Path('/tmp/cellbox-s10-9b')
IMAGE_ROOT = Path('/tmp/cellbox-s10-7/b2-image-v2')
V2_EVIDENCE = REPORTS / 's10-9-evidence' / 'b2-desktop-v2'
SCHEDULE = HERE / 'schedule-b2-desktop.json'
SET_NAME = 'b2-desktop'
SET_NAMES = (SET_NAME,)

# sha256 pins of the S10-9.3 hooks desktop set (== IMAGE_ROOT/manifest.json boot_sets['test-hooks'] and result.json).
PINNED = {
    'kernel': 'de5c9662202b805278ff3366bd4fbb2fdea84547b7e2a2f983a36809b091dc83',
    'iso': 'cf134c7af9da1a4ec881e90d7222e1b02ca71de8e4da954d79c3794b7c7b42ba',
    'qcow2': '0f8ea10d14c6985c11991432e475937a72d67f292de8b39d947ee7d78393b1c8',
    'kernel_fs_img': 'b05ab5e197b6c9a4bbf4e7d703e77536e57dc6c4a0d001980904a342e9d463cd',
    'init': '0c2f607bb29e090b31c7820290aafc1974c818c0610d216f00b5f88a10104104',
}
PINNED_CONTROL_KERNEL = '486d1883665d7667c6ffbcefdea3858e82ab8b75ffc379d037f3c4a7adc44f8c'     # non-hooks desktop kernel (control)
PINNED_MANIFEST = '1fef5496e0cc57d919822d849c6303372fc915563fb67e29247b8b6dcf4e1f1d'            # b2-image-v2/manifest.json
PINNED_STATIC_VERIFICATION = 'c92d63a490961e17b70c05a6457eca490f72a255c39826df2fa822003ecf5ed5'
KERNEL_FEATURES = ('dev-policy-key', 'dev-signing-key', 'dev-weak-rng', 'signing-required', 'native-domains', 'qemu-dev-https',
                   'large-cell-va', 'test-hooks')                                                  # build command 301 (manifest.json)
GLYPH_SHA = '820c92d2dc0aa5c1f50533a42283974b07998111adaff8e416f56dd4481eec79'                  # retained common.GLYPH_SHA
PRIOR_CENSUS_TOKEN_NOTE = 'tokens of S10-7/8/9 stay untouched'


@dataclass(frozen=True)
class B2DesktopSpec:
    name: str = SET_NAME
    profile: str = 'desktop'                    # retained Machine/Surface profile
    kind: str = 'b2'
    user_stack_pages: tuple = (16, 64)          # stack_pages_for(): B2 shell 16 pages, terminal 64 (S10-7.2 result.json)
    features: tuple = KERNEL_FEATURES

    @property
    def image_root(self):
        return IMAGE_ROOT

    @property
    def iso(self):
        return IMAGE_ROOT / 'desktop-test-hooks-cellos-x86_64-desktop.iso'

    @property
    def kernel(self):
        return IMAGE_ROOT / 'desktop-test-hooks-kernel.elf'

    @property
    def control_kernel(self):
        return IMAGE_ROOT / 'desktop-kernel.elf'

    @property
    def qcow2(self):
        return IMAGE_ROOT / 'desktop-test-hooks-cellos-x86_64-desktop.qcow2'

    @property
    def kernel_fs_img(self):
        return IMAGE_ROOT / 'desktop-kernel_fs.img'

    @property
    def init(self):
        return IMAGE_ROOT / 'desktop-init'

    @property
    def signed(self):
        return IMAGE_ROOT / 'signed'

    @property
    def glyph_bin(self):
        return E7.SETS['cand-desktop'].glyph_bin           # offline-rebuilt oracle, sha == retained GLYPH_SHA (checked in preflight)

    @property
    def dir(self):
        return WORK_ROOT                                    # run dirs live here: WORK_ROOT/run-<stamp>

    @property
    def token(self):
        return token()


SPEC = B2DesktopSpec()
SETS = {SET_NAME: SPEC}


def token(name=SET_NAME, root=None):
    """Run token of the ONE run (evaluated at call time: tests redirect HERE through `root`)."""
    if name != SET_NAME:
        raise KeyError(name)
    return Path(root if root is not None else HERE) / f'RUN_CONSUMED-s109-{SET_NAME}'


def claim_run(name=SET_NAME, path=None):
    """Atomically (O_EXCL) claim the one approved B2 desktop run; a second claim raises FileExistsError."""
    target = Path(path) if path is not None else token(name)
    descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(descriptor, json.dumps({'set': name, 'claimed_ns': time.monotonic_ns(), 'wall': time.time(), 'pid': os.getpid(),
                                     'meaning': 'S10-9b ONE B2 desktop run (CP1 S10-9 allowance); never remove, never rerun, no retry whatever the result'}).encode())
    os.close(descriptor)
    return target


def run_consumed(name=SET_NAME, path=None):
    return (Path(path) if path is not None else token(name)).exists()


# ------------------------------------------------------------------ hashes / provenance
def manifest():
    return json.loads((IMAGE_ROOT / 'manifest.json').read_text())


def static_verification():
    return json.loads((IMAGE_ROOT / 'static-verification.json').read_text())


def manifest_hashes():
    """test-hooks boot-set sha256 values as recorded by the S10-9.3 build (`manifest.json`)."""
    record = manifest()['boot_sets']['test-hooks']['sha256']
    return {'kernel': record['kernel'], 'iso': record['iso'], 'qcow2': record['qcow2'], 'kernel_fs_img': record['kernel_fs'], 'init': record['init']}


def expected_hashes():
    return dict(PINNED, control_kernel=PINNED_CONTROL_KERNEL)


def verify_set_files(spec=SPEC):
    """Hash-match kernel/control kernel/ISO/qcow2/kernel_fs.img/init against the pins; also pins vs manifest.json."""
    want = expected_hashes()
    paths = {'kernel': spec.kernel, 'control_kernel': spec.control_kernel, 'iso': spec.iso, 'qcow2': spec.qcow2,
             'kernel_fs_img': spec.kernel_fs_img, 'init': spec.init}
    got = {key: (sha256_file(path) if Path(path).is_file() else None) for key, path in paths.items()}
    try:
        recorded = manifest_hashes()
        pins_equal_manifest = all(recorded[k] == PINNED[k] for k in PINNED) and sha256_file(IMAGE_ROOT / 'manifest.json') == PINNED_MANIFEST
    except (OSError, KeyError, ValueError):
        pins_equal_manifest = False
    return {'set': spec.name, 'paths': {k: str(v) for k, v in paths.items()}, 'expected': want, 'actual': got,
            'match': {k: got[k] == want[k] for k in want}, 'pins_equal_manifest_json': pins_equal_manifest,
            'all_match': all(got[k] == want[k] for k in want) and pins_equal_manifest}


def provenance(spec=SPEC):
    """Kernel provenance recorded in every memory record: kernel_features MUST contain test-hooks."""
    return {'kernel_build': 'b2-desktop-v2-test-hooks', 'kernel_sha256': PINNED['kernel'], 'kernel_features': list(spec.features),
            'kernel_path': str(spec.kernel), 'profile': spec.profile, 'set': spec.name, 'kind': spec.kind,
            'control_kernel_non_hooks_sha256': PINNED_CONTROL_KERNEL,
            'stack_pattern': '0xA5 (kernel test-hooks prime of the whole usable range at spawn)',
            'measurement_kernel_is_not_the_promoted_kernel': True,
            'image': 'S10-9.3 B2 desktop v2 (services rebuilt from source at 3471028bc, non-pristine, historical comparator)'}


def schedule(path=None):
    return json.loads(Path(path if path is not None else SCHEDULE).read_text())


def schedule_snapshots(sched):
    return tuple(p['name'] for p in sched['phases'] if p.get('snapshot'))
