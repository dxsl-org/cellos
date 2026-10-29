#!/usr/bin/env python3
"""Discriminating QEMU-side NVMe trace/memory capture of unchanged retained candidate.
No qualification verdict is emitted. Only a private sparse copy of the prior fixture is writable.
"""
import hashlib
import json
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
BASE = Path('/tmp/cellbox-s10-3d-investigation')
RUN = BASE / ('probe-' + sys.argv[1])
RUN.mkdir(exist_ok=False)
ISO = Path(sys.argv[2] if len(sys.argv) > 2 else '/tmp/cellbox-s10-3c-resume/cand/headless.iso')
SOURCE = Path('/tmp/cellbox-phase10-w5t1anar/disk-ssh/disk.img')
sha = lambda p: hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()
iso_before, disk_before = sha(ISO), sha(SOURCE)
subprocess.run(['cp', '--reflink=auto', '--sparse=always', str(SOURCE), str(RUN / 'disk.img')], check=True)
argv = ['qemu-system-x86_64', '-machine', 'q35', '-accel', 'tcg', '-cpu', 'qemu64,+pdpe1gb,+rdrand,+rdseed',
        '-m', '256M', '-display', 'none', '-serial', 'file:' + str(RUN / 'serial.log'),
        '-qmp', 'unix:' + str(RUN / 'qmp.sock') + ',server=on,wait=off', '-monitor', 'none',
        '-drive', f'file={ISO},media=cdrom,readonly=on,format=raw', '-boot', 'd', '-no-reboot',
        '-netdev', 'user,id=net0', '-device', 'e1000,netdev=net0',
        '-drive', f'file={RUN / "disk.img"},if=none,format=raw,id=p10fx',
        '-device', 'nvme,drive=p10fx,serial=cellosp10',
        '-trace', f'enable=pci_nvme_*,file={RUN / "nvme.trace"}',
        '-trace', 'enable=pci_cfg_write']
(RUN / 'argv.json').write_text(json.dumps(argv, indent=2) + '\n')
with (RUN / 'qemu.stderr.log').open('wb') as stderr:
    proc = subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=stderr)
    qmp = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        until = time.monotonic() + 15
        while True:
            try:
                qmp.connect(str(RUN / 'qmp.sock'))
                break
            except (FileNotFoundError, ConnectionRefusedError):
                if proc.poll() is not None or time.monotonic() >= until:
                    raise
                time.sleep(.05)
        qfile = qmp.makefile('rb')
        qfile.readline()
        def command(name, args=None):
            qmp.sendall((json.dumps({'execute': name, **({'arguments': args} if args else {})}) + '\n').encode())
            while True:
                reply = json.loads(qfile.readline())
                if 'return' in reply or 'error' in reply: return reply
        command('qmp_capabilities')
        start = time.monotonic()
        reason = 'diagnostic-window-expired'
        while time.monotonic() - start < 100 and proc.poll() is None:
            text = (RUN / 'serial.log').read_text(errors='replace') if (RUN / 'serial.log').exists() else ''
            if '[nvme] admin timeout' in text:
                reason = 'admin-timeout'
                break
            if 'Cellos shell ready' in text:
                reason = 'booted-to-shell'
                break
            time.sleep(.05)
        command('stop')
        pci = command('human-monitor-command', {'command-line': 'info pci'})
        (RUN / 'pci.json').write_text(json.dumps(pci, indent=2) + '\n')
        text = (RUN / 'serial.log').read_text(errors='replace')
        addrs = re.search(r'admin.sq=0x([0-9a-f]+) admin.cq=0x([0-9a-f]+) io.sq=0x([0-9a-f]+) io.cq=0x([0-9a-f]+)', text)
        dumps = {}
        dumps['nvme-pci-config'] = command('pmemsave', {
            'val': 0xb0018000, 'size': 256, 'filename': str(RUN / 'nvme-pci-config.bin')})
        if addrs:
            for name, value in zip(['admin-sq', 'admin-cq', 'io-sq', 'io-cq'], addrs.groups()):
                address = int(value, 16)
                dumps[name] = command('pmemsave', {'val': address, 'size': 4096, 'filename': str(RUN / (name + '.bin'))})
        identify = re.search(r'\[nvme\] dma iovas: identify=0x([0-9a-f]+)', text)
        if identify:
            dumps['identify'] = command('pmemsave', {'val': int(identify[1],16), 'size': 4096, 'filename': str(RUN / 'identify.bin')})
        result = {'reason': reason, 'seconds': round(time.monotonic() - start, 3), 'dumps': dumps,
                  'iso_before': iso_before, 'source_disk_before': disk_before,
                  'iso_after': sha(ISO), 'source_disk_after': sha(SOURCE)}
        (RUN / 'capture.json').write_text(json.dumps(result, indent=2) + '\n')
        command('quit')
        print(json.dumps(result, indent=2), flush=True)
    finally:
        qmp.close()
        if proc.poll() is None:
            proc.terminate()
        proc.wait(timeout=15)
