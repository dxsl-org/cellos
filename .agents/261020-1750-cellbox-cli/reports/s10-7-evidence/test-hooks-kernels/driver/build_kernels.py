#!/usr/bin/env python3
"""S10-7.2 measurement kernels: build the non-hooks control and the `test-hooks` kernel for one tree.

usage: build_kernels.py <label> <tree> <features-control> <extra-env-json>
Everything runs under /tmp/cellbox-s10-7/th/<label>; one attempt per step, fail-fast, logs retained.
Control   = <features-control>
Measure   = <features-control>,test-hooks
Same recipe as the qualified build: `cargo build --release -p cellos-kernel --target x86_64-unknown-none
-Z build-std=core,alloc --features ...` with the kernel per-target rustflags (the repository
.cargo/config.toml x86_64-unknown-none block == the CI CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS value),
plus --locked --offline and the freestanding C toolchain env (as S10-3/S10-5/S10-7.1 driver).
"""
import hashlib, json, os, shutil, subprocess, sys, time
from pathlib import Path

sys.dont_write_bytecode = True
label, tree, feats = sys.argv[1], Path(sys.argv[2]), sys.argv[3]
extra_env = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
# '__no_build_std': true reproduces a historical route that used the prebuilt x86_64-unknown-none core/alloc
NO_BUILD_STD = bool(extra_env.pop('__no_build_std', False))
W = Path('/tmp/cellbox-s10-7/th') / label
LOGS = W / 'logs'
LOGS.mkdir(parents=True, exist_ok=True)
KERNEL_RUSTFLAGS = '-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static'
STRIP = ['CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS', 'CELLBOX_PRIVATE_FIXTURE_SIGNING',
         'CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI', 'CELLOS_SIGN_SEED_HEX', 'CELLOS_PHASE02_CELL_PUBLIC_KEY',
         'CELLOS_CONFIG_DIR', 'CELLOS_INIT_FEATURES', 'EMBEDDED_OVERRIDE', 'RUSTUP_TOOLCHAIN', 'X86_KERNEL',
         'X86_ISO_ROOT', 'XORRISO', 'OBJCOPY', 'GIT_DIR', 'GIT_WORK_TREE', 'CARGO_TARGET_DIR']


def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''):
            h.update(b)
    return h.hexdigest()


def env():
    e = dict(os.environ)
    for k in STRIP:
        e.pop(k, None)
    e['PYTHONDONTWRITEBYTECODE'] = '1'
    e['CARGO_NET_OFFLINE'] = 'true'
    e['CARGO_TARGET_DIR'] = str(W / 'target')
    e['CC_x86_64_unknown_none'] = 'cc'
    e['CFLAGS_x86_64_unknown_none'] = ('-ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx '
                                       f'-DLFS_NO_INTRINSICS -I{tree}/third_party/freestanding-include')
    e['BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none'] = '--target=x86_64-linux-gnu'
    e['CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS'] = KERNEL_RUSTFLAGS
    e.update(extra_env)
    return e


records = []


def run(name, argv):
    log = LOGS / f'{name}.log'
    assert not log.exists(), f'one attempt per step: {log}'
    t0 = time.time()
    with log.open('wb') as f:
        p = subprocess.run([str(a) for a in argv], cwd=tree, env=env(), stdout=f, stderr=subprocess.STDOUT)
    records.append({'step': name, 'argv': [str(a) for a in argv], 'cwd': str(tree), 'rc': p.returncode,
                    'seconds': round(time.time() - t0, 1), 'log': str(log), 'log_sha256': sha(log)})
    (W / 'commands.json').write_text(json.dumps(records, indent=2) + '\n')
    if p.returncode:
        (W / 'STOP.json').write_text(json.dumps({'status': 'STOPPED', 'step': name, 'rc': p.returncode,
                                                  'log': str(log)}, indent=2) + '\n')
        raise SystemExit(f'STOP {name} rc={p.returncode}')


def build(tag, features):
    run(f'build-{tag}', ['cargo', '+nightly-2026-05-01', 'build', '--release', '--locked', '--offline',
                         '-p', 'cellos-kernel', '--target', 'x86_64-unknown-none',
                         *([] if NO_BUILD_STD else ['-Z', 'build-std=core,alloc']),
                         '--features', features])
    out = W / f'{tag}.kernel.elf'
    shutil.copyfile(W / 'target/x86_64-unknown-none/release/cellos-kernel', out)
    return out


run('00-rustc', ['rustc', '+nightly-2026-05-01', '-Vv'])
run('01-cargo', ['cargo', '+nightly-2026-05-01', '-V'])
c = build('control', feats)
m = build('test-hooks', feats + ',test-hooks')
res = {'status': 'BUILT', 'tree': str(tree), 'features_control': feats, 'features_measure': feats + ',test-hooks',
       'control_kernel_sha256': sha(c), 'control_kernel_bytes': c.stat().st_size,
       'measure_kernel_sha256': sha(m), 'measure_kernel_bytes': m.stat().st_size}
(W / 'build-result.json').write_text(json.dumps(res, indent=2) + '\n')
print(json.dumps(res, indent=2))
