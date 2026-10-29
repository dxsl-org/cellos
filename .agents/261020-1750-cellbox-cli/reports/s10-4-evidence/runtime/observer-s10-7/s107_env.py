"""S10-7.4 environment: paths, original deadlines, measurement-set registry, run tokens, ELF helpers.

Nothing here starts a VM.  Retained modules are imported, never edited; where a retained module
hard-codes a path that no longer exists (`/tmp/cellbox-s10-3d-resume`, `/tmp/cellbox-s10-4`,
`/tmp/cellbox-phase10-*`) the runner redirects the module attribute at run time (`redirect_retained`)
and records that fact; the retained files stay byte-identical (go_no_go checks their hashes).
"""
import hashlib
import json
import os
import struct
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
RUNTIME = HERE.parent
REPORTS = RUNTIME.parent.parent                      # .../reports
PLAN = REPORTS.parent                                # .../261020-1750-cellbox-cli
REPO = PLAN.parent.parent                            # /home/dmin/cellos
EVIDENCE = REPORTS / 's10-7-evidence' / 'harness'
TH_KERNELS = REPORTS / 's10-7-evidence' / 'test-hooks-kernels'
MEASURE_ROOT = Path('/tmp/cellbox-s10-7/measure')
B2_IMAGE = Path('/tmp/cellbox-s10-7/b2-image')
CAND_CACHE = Path.home() / '.cache/cellos-candidates/4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821'
CAND_SHA = '4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821'
CAND_RETAINED_ROOT = '/tmp/cellbox-s10-3d-resume/cand'   # prefix recorded inside candidate.json (directory no longer exists)
FEED_HOOK = RUNTIME / 'observer-feed-hook'
MEMORY = RUNTIME / 'observer-memory'
RUN_EVIDENCE = REPORTS / 's10-7-evidence' / 'measurement-runs'

PAGE = 4096
SLOT_BASE = 0x1_0000_0000          # retained observer.is_entry: first cell VA slot
SLOT_STRIDE = 32 * 1024 * 1024     # retained observer.is_entry stride (headless/embedded; desktop 256 MiB slots are multiples)
SLOT_COUNT = 512

# Original deadlines (S10-4.4 common.Surface / run.py). Tests prove they equal the retained source.
DEADLINES = {
    'command_s': 120,          # common.Surface.command(timeout=120): finite command wait
    'status_s': 60,            # common.Surface.status: self.command('status', 'echo st=$?', 60)
    'total_window_s': 900,     # common.Surface.check / ready: 900 s measurement window
}
STATUS_CMD = 'echo st=$?'

SET_NAMES = ('cand-desktop', 'cand-headless', 'b2-headless')
UNRUNNABLE_SETS = {'b2-desktop': 'boot smoke panicked (D7-8: tlb_shootdown timeout / cell heap OOM); desktop historical comparator is null'}


class EnvError(Exception):
    """The measurement environment is not what the plan requires (fail closed)."""


def sha256_file(path):
    with Path(path).open('rb') as handle:
        return hashlib.file_digest(handle, 'sha256').hexdigest()


def dump(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=False) + '\n')


@dataclass(frozen=True)
class SetSpec:
    name: str
    profile: str                 # headless | desktop (retained Machine/Surface profile)
    kind: str                    # candidate | b2
    iso_name: str
    features: tuple              # cargo features of the measurement kernel (must include test-hooks)
    user_stack_pages: tuple      # accepted usable-page counts of the measured task stacks
    notes: dict = field(default_factory=dict, compare=False)

    @property
    def dir(self):
        return MEASURE_ROOT / self.name

    @property
    def iso(self):
        return self.dir / self.iso_name

    @property
    def kernel(self):
        return self.dir / f'{self.name}-test-hooks.kernel.elf'

    @property
    def control_kernel(self):
        return self.dir / f'{self.name}-control.kernel.elf'

    @property
    def token(self):
        return HERE / f'RUN_CONSUMED-{self.name}'

    @property
    def run_inputs(self):
        return self.dir / 'run-inputs'

    @property
    def fixture_disk(self):
        return self.run_inputs / 'disk-main' / 'disk.img'

    @property
    def glyph_bin(self):
        return self.run_inputs / 'host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs'


SETS = {
    'cand-desktop': SetSpec('cand-desktop', 'desktop', 'candidate', 'cellos-x86_64-desktop-test-hooks.iso',
                            ('policy-required', 'test-hooks'), (16,)),
    'cand-headless': SetSpec('cand-headless', 'headless', 'candidate', 'vicell-x86-test-hooks.iso',
                             ('default', 'policy-required', 'test-hooks'), (16,)),
    'b2-headless': SetSpec('b2-headless', 'headless', 'b2', 'vicell-x86-b2-test-hooks.iso',
                           ('default', 'test-hooks'), (16, 64)),
}


def manifest():
    """The S10-7.2 test-hooks kernel manifest (hash source of truth for kernels/ISOs)."""
    return json.loads((TH_KERNELS / 'result.json').read_text())['sets']


def expected_hashes(name):
    entry = manifest()[name]
    spec = SETS[name]
    return {'kernel': entry['test_hooks_kernel_sha256'], 'control_kernel': entry['control_kernel_non_hooks_sha256'],
            'iso': entry['iso'][spec.iso_name]['sha256'], 'kernel_fs_img': entry['kernel_fs_img_sha256'],
            'init': entry['init_sha256']}


def verify_set_files(name):
    """Hash-match the set's kernels/ISO/init/kernel_fs.img against the manifest; returns a record."""
    spec, want = SETS[name], expected_hashes(name)
    paths = {'kernel': spec.kernel, 'control_kernel': spec.control_kernel, 'iso': spec.iso,
             'kernel_fs_img': spec.dir / 'kernel_fs.img', 'init': spec.dir / 'init'}
    got = {key: (sha256_file(path) if Path(path).is_file() else None) for key, path in paths.items()}
    return {'set': name, 'paths': {k: str(v) for k, v in paths.items()}, 'expected': want, 'actual': got,
            'match': {k: got[k] == want[k] for k in want}, 'all_match': all(got[k] == want[k] for k in want)}


def provenance(name, *, kernel_sha256=None):
    """Kernel provenance recorded in every memory record: kernel_features MUST contain test-hooks."""
    spec = SETS[name]
    sha = kernel_sha256 or expected_hashes(name)['kernel']
    return {'kernel_build': f'{name}-test-hooks', 'kernel_sha256': sha, 'kernel_features': list(spec.features),
            'kernel_path': str(spec.kernel), 'profile': spec.profile, 'set': name, 'kind': spec.kind,
            'control_kernel_non_hooks_sha256': expected_hashes(name)['control_kernel'],
            'stack_pattern': '0xA5 (kernel test-hooks prime of the whole usable range at spawn)',
            'measurement_kernel_is_not_the_promoted_kernel': True}


# ------------------------------------------------------------------ one-run tokens
def claim_run(name, path=None):
    """Atomically claim the one approved run of `name`; a second claim raises FileExistsError."""
    token = Path(path) if path is not None else SETS[name].token
    descriptor = os.open(token, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(descriptor, json.dumps({'set': name, 'claimed_ns': time.monotonic_ns(), 'wall': time.time(),
                                     'pid': os.getpid(),
                                     'meaning': f'S10-7.4 ONE run of set {name}; never remove, never rerun (D7-3)'}).encode())
    os.close(descriptor)
    return token


def run_consumed(name, path=None):
    return (Path(path) if path is not None else SETS[name].token).exists()


# ------------------------------------------------------------------ ELF helpers (no subprocess)
def elf_symbols(path):
    """All .symtab symbols of an ELF64LE file as dicts (raw mangled names as bytes)."""
    data = Path(path).read_bytes()
    if data[:6] != b'\x7fELF\x02\x01':
        raise EnvError(f'{path}: not ELF64LE')
    shoff = struct.unpack_from('<Q', data, 40)[0]
    shent, shnum, _shstr = struct.unpack_from('<HHH', data, 58)
    sections = [struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * shent) for i in range(shnum)]
    symtab = next((s for s in sections if s[1] == 2), None)
    if symtab is None:
        raise EnvError(f'{path}: no .symtab (stripped)')
    strtab = sections[symtab[6]]
    names = data[strtab[4]:strtab[4] + strtab[5]]
    out = []
    for at in range(symtab[4], symtab[4] + symtab[5], 24):
        name, info, _other, shndx, value, size = struct.unpack_from('<IBBHQQ', data, at)
        raw = names[name:names.index(b'\0', name)]
        out.append({'name': raw, 'info': info, 'shndx': shndx, 'value': value, 'size': size})
    return out


def unique_symbol(symbols, *needles, size=None, what=None):
    """The single symbol whose mangled name contains every needle (bytes); fail closed otherwise."""
    hits = [s for s in symbols if all(n in s['name'] for n in needles)]
    if len(hits) != 1:
        raise EnvError(f'{what or needles}: expected exactly one symbol, found {len(hits)}')
    if size is not None and hits[0]['size'] != size:
        raise EnvError(f'{what or needles}: symbol size {hits[0]["size"]:#x} != {size:#x}')
    return hits[0]


def elf_load_segments(path):
    data = Path(path).read_bytes()
    phoff = struct.unpack_from('<Q', data, 32)[0]
    phent, phnum = struct.unpack_from('<HH', data, 54)
    loads = []
    for i in range(phnum):
        kind, flags, off, va, _pa, filesz, memsz, _align = struct.unpack_from('<IIQQQQQQ', data, phoff + i * phent)
        if kind == 1:
            loads.append({'va': va, 'offset': off, 'filesz': filesz, 'memsz': memsz, 'flags': flags})
    return loads


def kernel_has_test_hooks(path):
    """True iff the kernel ELF contains the feature-gated `test_hook_prime_watermark` symbol."""
    return any(b'test_hook_prime_watermark' in s['name'] for s in elf_symbols(path))


# ------------------------------------------------------------------ retained-module redirection
def candidate_files(profile):
    """candidate.json (cache copy, identical sha) with the vanished /tmp prefix mapped to the cache."""
    path = CAND_CACHE / 'candidate.json'
    if sha256_file(path) != CAND_SHA:
        raise EnvError('candidate.json cache copy differs from the qualified candidate sha')
    record = json.loads(path.read_text())['profiles'][profile]

    def remap(text):
        text = str(text)
        return str(CAND_CACHE / text[len(CAND_RETAINED_ROOT) + 1:]) if text.startswith(CAND_RETAINED_ROOT + '/') else text
    files = {name: dict(item, file=remap(item['file'])) for name, item in record['files'].items()}
    return record, files


def redirect_retained(modules):
    """Point retained module attributes at paths that still exist; returns a record of every redirect.

    modules: dict with keys `common` (runtime/common.py) and `observer` (runtime/observer.py): their `W` work-dir attribute
    (heap-instruction file) is pointed at a copy of the archived protocol file.  No retained file is edited.
    """
    record = {}
    protocol = REPORTS / 's10-4-evidence'
    work = Path('/tmp/cellbox-s10-7/harness-prep/retained-W')
    (work / 'observability').mkdir(parents=True, exist_ok=True)
    target = work / 'observability/heap-instructions.json'
    source = protocol / 'protocol/heap-instructions.json'
    if not target.exists():
        target.write_bytes(source.read_bytes())
    if sha256_file(target) != sha256_file(source):
        raise EnvError('heap-instruction copy differs from the archived protocol file')
    for key in ('common', 'observer'):
        module = modules.get(key)
        if module is not None:
            record[f'{key}.W'] = [str(module.W), str(work)]
            module.W = work
    return record
