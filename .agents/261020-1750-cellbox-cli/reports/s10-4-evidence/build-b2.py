#!/usr/bin/env python3
"""Historical B2 build/sign only; never rebuild the qualified after candidate."""
import hashlib,json,os,shutil,subprocess,time
from pathlib import Path
W=Path('/tmp/cellbox-s10-4'); R=W/'baseline-worktree'; T=W/'baseline-target'
assert (W/'canonical/before.json').is_file(), 'canonical before record required'
L=W/'logs-reconciled'; L.mkdir(exist_ok=True)
env=dict(os.environ)
env['CARGO_NET_OFFLINE']='true'
for k in ['CARGO_ENCODED_RUSTFLAGS','CELLBOX_PRIVATE_FIXTURE_SIGNING','CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI','CELLOS_PHASE02_CELL_PUBLIC_KEY','CELLOS_CONFIG_DIR','CELLOS_INIT_FEATURES','EMBEDDED_OVERRIDE']:
    env.pop(k,None)
env.update(CARGO_TARGET_DIR=str(T),RUSTFLAGS='-C relocation-model=pic',CC_x86_64_unknown_none='cc',CFLAGS_x86_64_unknown_none='-ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I/home/dmin/cellos/third_party/freestanding-include',BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none='--target=x86_64-linux-gnu')
commands=[]
def run(args,name):
    commands.append({'command':args,'cwd':str(R),'log':str(L/name)})
    with (L/name).open('w') as log:
        p=subprocess.run(args,cwd=R,env=env,stdout=log,stderr=subprocess.STDOUT)
    commands[-1]['returncode']=p.returncode
    (W/'build-commands-reconciled.json').write_text(json.dumps(commands,indent=2)+'\n')
    if p.returncode: raise SystemExit(f'STOP: {name} exit {p.returncode}')
run(['rustc','+nightly-2026-05-01','-Vv'],'b2-rustc.log')
run(['cargo','+nightly-2026-05-01','-V'],'b2-cargo.log')
run(['cargo','+nightly-2026-05-01','build','--release','--locked','--offline','--target','x86_64-unknown-none','-Z','build-std=core,alloc','-p','app-shell','-p','app-sys-tools','--bin','app-shell','--bin','ls','--bin','cat','--bin','echo','--bin','ps','--bin','kill'],'b2-build.log')
unsigned=W/'b2-unsigned'; signed=W/'b2-signed'
unsigned.mkdir(exist_ok=True); signed.mkdir(exist_ok=True)
for name in ['shell','ls','cat','echo','ps','kill']:
    src=T/'x86_64-unknown-none/release'/('app-shell' if name=='shell' else name)
    if not src.is_file(): raise SystemExit(f'STOP: missing built artifact {src}')
    shutil.copy2(src,unsigned/name); shutil.copy2(src,signed/name)
run(['python3','scripts/cellos-sign','--sign',*[str(signed/n) for n in ['shell','ls','cat','echo','ps','kill']]],'b2-sign.log')
for name in ['shell','ls','cat','echo','ps','kill']:
    run(['python3','scripts/sign-cell.py','--verify','--in',str(signed/name)],f'b2-verify-{name}.log')
print('B2 build and checked dev signing complete')
