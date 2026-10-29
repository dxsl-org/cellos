#!/usr/bin/env python3
"""One boot-only smoke per set (D7-8). No retry. Kills only its own QEMU PID."""
import json, os, re, shutil, socket, subprocess, sys, time, hashlib
SET = 'b2-desktop-nonhooks'
MEAS = None
SMOKE = '/tmp/cellbox-s10-7/smoke-diag'
EV = '/home/dmin/cellos/.agents/261020-1750-cellbox-cli/reports/s10-7-evidence/b2-desktop-diagnostic'
KIND = 'desktop'
src='desktop-cellos-x86_64-desktop.qcow2'
work = f'{SMOKE}/{SET}'; shutil.rmtree(work, ignore_errors=True); os.makedirs(work)
out = EV; os.makedirs(out, exist_ok=True)
disk = f'{work}/{src}'
shutil.copy(f'/tmp/cellbox-s10-7/b2-image/{src}', disk)
sha_in = hashlib.sha256(open(f'/tmp/cellbox-s10-7/b2-image/{src}','rb').read()).hexdigest()
serial = f'{out}/serial.log'
qmp = f'{work}/qmp.sock'
if KIND == 'headless':
    cmd = ['qemu-system-x86_64','-machine','q35','-cpu','qemu64,+pdpe1gb','-m','256M','-nographic',
           '-cdrom',disk,'-boot','d','-no-reboot']
    ready_re = re.compile(rb'Cellos > ')
else:
    cmd = ['qemu-system-x86_64','-cpu','qemu64,+pdpe1gb','-machine','q35,vmport=on','-m','2048M','-smp','2',
           '-drive',f'file={disk},format=qcow2,if=virtio',
           '-netdev','user,id=net0','-device','e1000,netdev=net0',
           '-device','virtio-keyboard-pci','-device','virtio-tablet-pci','-device','virtio-mouse-pci',
           '-display','none','-vnc','127.0.0.1:38',
           '-qmp',f'unix:{qmp},server,nowait','-serial',f'file:{serial}']
    ready_re = re.compile(rb'\[desktop\] Ready\. Entering event loop\.')
rec = {'set':SET,'kind':KIND,'argv':cmd,'accel':'tcg (default; no -enable-kvm)',
 'deviations_from_recorded':
   (['no -display gtk','-display none','hostfwd removed (no inbound)','-vnc 127.0.0.1:38 (unique port)','-qmp unix socket path private','-serial file instead of mon:stdio','private qcow2 copy','recorded -drive file= replaced by private copy']
    if KIND=='desktop' else ['private ISO copy; stdout/stderr (serial) redirected to serial.log; stdin held open, no input sent']),
 'disk_source':f'/tmp/cellbox-s10-7/b2-image/{src}','disk_source_sha256':sha_in,'disk_copy':disk}
json.dump(rec, open(f'{out}/command.json','w'), indent=2)
t0 = time.monotonic(); wall0 = time.time()
if KIND == 'headless':
    sf = open(serial,'wb')
    p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=sf, stderr=subprocess.STDOUT)
else:
    p = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=open(f'{work}/qemu.stdout','wb'), stderr=subprocess.STDOUT)
rec['qemu_pid'] = p.pid
TIMEOUT = 900
PANIC_RE = re.compile(rb'KERNEL PANIC|panicked at|BOOT-FATAL')
status = None; reason = ''; t_ready = None; t_panic = None
def data():
    try: return open(serial,'rb').read()
    except FileNotFoundError: return b''
try:
    while True:
        time.sleep(0.25)
        el = time.monotonic() - t0
        d = data()
        if t_panic is None and PANIC_RE.search(d):
            t_panic = el
        if t_panic is not None and el - t_panic > 5:
            status, reason = 'FAILED', 'panic marker in serial'; break
        if t_panic is None and ready_re.search(d):
            t_ready = el; status = 'READY'; break
        if p.poll() is not None:
            time.sleep(0.5)
            status, reason = 'FAILED', f'QEMU exited rc={p.returncode} before ready'; break
        if el > TIMEOUT:
            status, reason = 'TIMEOUT', f'no ready marker within {TIMEOUT}s'; break
    t_end = time.monotonic() - t0
    shot = None
    if status == 'READY' and KIND == 'desktop':
        time.sleep(2)  # let the compositor draw after the marker
        s = socket.socket(socket.AF_UNIX); s.settimeout(30); s.connect(qmp)
        f = s.makefile('rw'); f.readline()
        def q(c):
            f.write(json.dumps(c)+'\n'); f.flush()
            while True:
                r = json.loads(f.readline())
                if 'return' in r or 'error' in r: return r
        q({'execute':'qmp_capabilities'})
        ppm = f'{work}/screen.ppm'
        r = q({'execute':'screendump','arguments':{'filename':ppm,'format':'png'}})
        shot = r
        if 'error' in r:
            r = q({'execute':'screendump','arguments':{'filename':ppm}}); shot = r
        if os.path.exists(ppm):
            try:
                if open(ppm,'rb').read(4) == b'\x89PNG': shutil.copy(ppm, f'{out}/screen.png')
                else:
                    subprocess.run(['python3','-c','from PIL import Image;Image.open("%s").save("%s")'%(ppm,f'{out}/screen.png')],check=True)
            except Exception as e: shot = {'convert_error':str(e),'qmp':shot}
        time.sleep(1)
finally:
    if p.poll() is None:
        p.kill()
    try: p.wait(timeout=20)
    except Exception: pass
    time.sleep(1)
raw = data()
txt = raw.decode('utf-8','replace')
lines = txt.replace('\r','').split('\n')
def sel(pat): return [l for l in lines if re.search(pat,l)]
if t_panic is not None and status=='READY': status = 'FAILED'
rec.update({
 'status': status, 'reason': reason,
 'wall_time_to_ready_s': round(t_ready,2) if t_ready is not None else None,
 'wall_time_total_s': round(time.monotonic()-t0,2),
 'start_epoch': wall0, 'timeout_s': TIMEOUT,
 'qemu_exit_code': p.returncode, 'serial_bytes': len(raw),
 'screendump': shot if KIND=='desktop' else None,
 'markers': {
   'S22-X86-ADMISSION-ENABLED': sel(r'S22-X86-ADMISSION-ENABLED'),
   'stack-baseline': sel(r'\[stack-baseline\]'),
   'panic': sel(r'KERNEL PANIC|panicked|\bPANIC\b|BOOT-FATAL|[Aa]ssertion'),
   'FAIL': sel(r'\bFAIL\b'),
   'ready_line': sel(r'Cellos > $|\[desktop\] Ready\. Entering event loop\.')[:3],
 },
 'final_60_serial_lines': lines[-60:],
})
json.dump(rec, open(f'{out}/result.json','w'), indent=2)
print(SET, status, reason, rec['wall_time_to_ready_s'], rec['wall_time_total_s'], 'bytes', len(raw))
