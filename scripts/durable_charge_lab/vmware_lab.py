#!/usr/bin/env python3
"""Provision/run the isolated Linux durable-charge lab. Never touches an existing VM."""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path(__file__).resolve().parent
IMAGE_SHA256 = '2f119f50bf29fe40e73b69ae2a736e8210d8146796f036e0324315b6b3a777df'
IMAGE_URL = 'https://cloud-images.ubuntu.com/minimal/releases/noble/release/ubuntu-24.04-minimal-cloudimg-amd64.img'
SPEC = importlib.util.spec_from_file_location('vmboot', ROOT / 'scripts/x86/vmware-boot-test.py')
VM = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VM)


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for part in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(part)
    return h.hexdigest()


def save(path, value):
    Path(path).write_text(json.dumps(value, sort_keys=True, indent=2) + '\n')


def command(argv, log, timeout=180):
    code = VM.run_logged([str(x) for x in argv], Path(log), timeout)
    if code:
        raise RuntimeError('Command failed; see ' + str(log))


def host_tools():
    vmrun = VM.find_vmrun(None)
    if not VM.IS_WSL_WIN:
        raise RuntimeError('This profile requires the observed WSL-to-Windows Workstation tuple')
    return vmrun


def provision(args):
    host_tools()
    image = Path(args.image).resolve(strict=True)
    if digest(image) != IMAGE_SHA256:
        raise RuntimeError('Official guest image differs from pinned checksum')
    root = Path(args.workspace_root).resolve()
    if not str(root).startswith('/mnt/c/'):
        raise RuntimeError('VMware workspace must be Windows-visible under /mnt/c/')
    root.mkdir(parents=True, exist_ok=True)
    lab = Path(tempfile.mkdtemp(prefix='durable-charge-lab-', dir=root))
    os.chmod(lab, 0o700)
    save(lab / 'ownership.json', {'format': 'cellos-private-vmware-lab-v1', 'namespace': str(uuid.uuid4()), 'lab': str(lab)})
    command(['qemu-img', 'convert', '-f', 'qcow2', '-O', 'vmdk', '-o', 'subformat=monolithicSparse,adapter_type=ide', image, lab / 'boot.vmdk'], lab / 'convert-boot.log')
    raw = lab / 'data.raw'
    with raw.open('xb') as stream:
        stream.truncate(1024 * 1024 * 1024)
    command(['mkfs.ext4', '-F', '-L', 'CELLOS_DCO_LAB', raw], lab / 'mkfs-own-data.log')
    fs_uuid = subprocess.check_output(['blkid', '-p', '-s', 'UUID', '-o', 'value', str(raw)], text=True).strip()
    command(['qemu-img', 'convert', '-f', 'raw', '-O', 'vmdk', '-o', 'subformat=monolithicSparse,adapter_type=ide', raw, lab / 'data.vmdk'], lab / 'convert-data.log')
    raw.unlink()
    save(lab / 'profile.json', {'format': 'cellos-private-vmware-lab-v1', 'image_url': IMAGE_URL, 'image_sha256': IMAGE_SHA256, 'data_uuid': fs_uuid, 'cpus': 1, 'memory_mib': 1024, 'network': False, 'backend': 'guest SQLite FULL/DELETE over ext4 on dedicated VMware IDE VMDK', 'threat_model': 'ordinary virtual-volume VM crash/restart; no independent antirollback or physical power-loss qualification'})
    print(str(lab), flush=True)


def checked_lab(path):
    lab = Path(path).resolve(strict=True)
    owner = json.loads((lab / 'ownership.json').read_text())
    if owner.get('format') != 'cellos-private-vmware-lab-v1' or owner.get('lab') != str(lab):
        raise RuntimeError('Not an owned lab namespace')
    for name in ('boot.vmdk', 'data.vmdk', 'profile.json'):
        if not (lab / name).is_file() or (lab / name).is_symlink():
            raise RuntimeError('Missing/unsafe owned lab operand: ' + name)
    return lab, json.loads((lab / 'profile.json').read_text())


BOOT_LAUNCHER = '''#!/bin/bash
set -euo pipefail
exec >/dev/ttyS0 2>&1
mkdir -p /mnt/cellos-seed
mountpoint -q /mnt/cellos-seed || mount -o ro /dev/sr0 /mnt/cellos-seed
exec /bin/bash /mnt/cellos-seed/guest-bootstrap.sh
'''


BOOT_SCRIPT = r'''#!/bin/bash
set -euo pipefail
exec >/dev/ttyS0 2>&1
trap 'echo LAB-BOOT-FAIL; exit 1' ERR
mkdir -p /mnt/cellos-seed /var/lib/cellos-lab
mountpoint -q /mnt/cellos-seed || mount -o ro /dev/sr0 /mnt/cellos-seed
EXPECTED=$(python3 -c 'import json; print(json.load(open("/mnt/cellos-seed/lab-profile.json"))["data_uuid"])')
DEVICE=$(blkid -t UUID="$EXPECTED" -o device)
[[ "$DEVICE" = /dev/sdb ]]
[[ $(blkid -s TYPE -o value "$DEVICE") = ext4 ]]
mountpoint -q /var/lib/cellos-lab || mount -o rw,noatime "$DEVICE" /var/lib/cellos-lab
# Persist provisioning on the boot volume before any scenario hard-stop. With no
# NIC this service has no useful online condition; do not wait for it next boot.
systemctl mask systemd-networkd-wait-online.service
sync -f /
export CELLOS_LAB_DATA_UUID="$EXPECTED"
python3 -u - <<'PY'
import json,sys,sqlite3,ssl,platform,subprocess,os,hashlib
from pathlib import Path
import cryptography
hashes={name:hashlib.sha256(Path(path).read_bytes()).hexdigest() for name,path in {'launcher':'/usr/local/sbin/cellos-durable-lab','service':'/etc/systemd/system/cellos-durable-lab.service','bootstrap':'/mnt/cellos-seed/guest-bootstrap.sh'}.items()}
print('LAB-GUEST '+json.dumps({'kernel':platform.release(),'python':platform.python_version(),'sqlite':sqlite3.sqlite_version,'cryptography':cryptography.__version__,'openssl':ssl.OPENSSL_VERSION,'mount':subprocess.check_output(['findmnt','-n','-o','SOURCE,FSTYPE,OPTIONS','/var/lib/cellos-lab'],text=True).strip(),'data_uuid':os.environ['CELLOS_LAB_DATA_UUID'],'bootstrap_hashes':hashes}),flush=True)
PY
SCENARIO=$(python3 -c 'import json; print(json.load(open("/mnt/cellos-seed/scenario.json"))["scenario"])')
PHASE=$(python3 -c 'import json; print(json.load(open("/mnt/cellos-seed/scenario.json"))["phase"])')
exec python3 -u /mnt/cellos-seed/payload/scenarios.py --data /var/lib/cellos-lab --scenario "$SCENARIO" --phase "$PHASE"
'''

SERVICE = '''[Unit]
Description=Cellos isolated durable-charge lab
After=local-fs.target
[Service]
Type=oneshot
ExecStart=/usr/local/sbin/cellos-durable-lab
TimeoutStartSec=infinity
[Install]
WantedBy=multi-user.target
'''


def source_pins():
    return {name: digest(SOURCE / name) for name in ('auth.py', 'runtime.py', 'scenarios.py', 'vmware_lab.py')}


def seed(lab, profile, run, scenario, phase):
    staging = run / 'seed'
    staging.mkdir()
    payload = staging / 'payload'
    payload.mkdir()
    pins = source_pins()
    for name in ('auth.py', 'runtime.py', 'scenarios.py'):
        shutil.copyfile(SOURCE / name, payload / name)
    (staging / 'guest-bootstrap.sh').write_text(BOOT_SCRIPT)
    for name in ('auth.py', 'runtime.py', 'scenarios.py'):
        if digest(payload / name) != pins[name]:
            raise RuntimeError('Copied payload differs from source: ' + name)
    config = {'users': [], 'disable_root': True, 'ssh_pwauth': False, 'write_files': [
        {'path': '/usr/local/sbin/cellos-durable-lab', 'permissions': '0700', 'content': BOOT_LAUNCHER},
        {'path': '/etc/systemd/system/cellos-durable-lab.service', 'permissions': '0644', 'content': SERVICE}],
        'runcmd': [['systemctl', 'daemon-reload'], ['systemctl', 'enable', 'cellos-durable-lab.service'], ['systemctl', 'start', '--no-block', 'cellos-durable-lab.service']]}
    (staging / 'user-data').write_text('#cloud-config\n' + json.dumps(config))
    namespace = json.loads((lab / 'ownership.json').read_text())['namespace']
    (staging / 'meta-data').write_text('instance-id: ' + namespace + '\nlocal-hostname: cellos-durable-lab\n')
    (staging / 'network-config').write_text('version: 2\nethernets: {}\n')
    save(staging / 'scenario.json', {'scenario': scenario, 'phase': phase})
    save(staging / 'lab-profile.json', profile)
    command(['xorriso', '-as', 'mkisofs', '-V', 'cidata', '-J', '-r', '-o', run / 'seed.iso', staging], run / 'make-seed.log')
    return pins


def vmx_text(lab, run):
    # The pinned image's built-in ata_piix boots IDE; lsilogic did not expose root.
    entries = {
        '.encoding': 'UTF-8', 'config.version': '8', 'virtualHW.version': '14',
        'displayName': 'Cellos private durable charge lab', 'guestOS': 'ubuntu-64',
        'firmware': 'bios', 'memsize': '1024', 'numvcpus': '1',
        'cpuid.coresPerSocket': '1', 'bios.bootOrder': 'hdd',
        'msg.autoAnswer': 'TRUE', 'hpet0.present': 'TRUE',
        'ide0:0.present': 'TRUE', 'ide0:0.fileName': VM.to_host_path(lab / 'boot.vmdk'),
        'ide0:1.present': 'TRUE', 'ide0:1.fileName': VM.to_host_path(lab / 'data.vmdk'),
        'ide1:0.present': 'TRUE', 'ide1:0.deviceType': 'cdrom-image',
        'ide1:0.fileName': VM.to_host_path(run / 'seed.iso'), 'ide1:0.startConnected': 'TRUE',
        'serial0.present': 'TRUE', 'serial0.fileType': 'file',
        'serial0.fileName': VM.to_host_path(run / 'serial.log'),
        'serial0.startConnected': 'TRUE', 'serial0.yieldOnMsrRead': 'TRUE',
        'ethernet0.present': 'FALSE', 'floppy0.present': 'FALSE',
        'sound.present': 'FALSE', 'usb.present': 'FALSE', 'ehci.present': 'FALSE',
        'usb_xhci.present': 'FALSE', 'vmci0.present': 'FALSE', 'scsi0.present': 'FALSE',
        'sata0.present': 'FALSE', 'mks.enable3d': 'FALSE', 'isolation.tools.hgfs.disable': 'TRUE'}
    return ''.join(key + ' = ' + VM.vmx_string(value) + '\n' for key, value in entries.items())


def markers(raw, prefix):
    rows = []
    for line in raw.decode(errors='replace').splitlines(keepends=True):
        if not line.endswith(chr(10)):
            continue
        if line.startswith(prefix + ' '):
            body = line[len(prefix) + 1:]
            if len(body) > 65536:
                raise RuntimeError('Oversized guest marker')
            rows.append(json.loads(body))
    return rows


class GuestRunFailure(RuntimeError):
    def __init__(self, run, result):
        super().__init__('Guest run failed: ' + str(run))
        self.run, self.result = run, result


def run_guest(lab_path, scenario, phase, expect_cut=None, window=300):
    vmrun = host_tools()
    lab, profile = checked_lab(lab_path)
    if not re.fullmatch(r'[a-zA-Z0-9_-]{1,80}', scenario) or phase not in ('run', 'prepare', 'recover'):
        raise RuntimeError('Invalid scenario/phase')
    run = lab / ('run-' + scenario + '-' + phase + '-' + uuid.uuid4().hex[:8])
    run.mkdir()
    result = {'scenario': scenario, 'phase': phase, 'expected_cut': expect_cut, 'window_seconds': window, 'status': 'FAIL', 'errors': [], 'cleanup_confirmed': False, 'profile': profile}
    vmx = lab / 'lab.vmx'
    started = False
    start = time.monotonic()
    try:
        # Refuse concurrent use of this one persistent namespace.
        listing = subprocess.check_output([str(vmrun), '-T', 'ws', 'list'], text=True)
        target = VM.to_host_path(vmx).replace('\\', '/').casefold()
        if target in {x.strip().replace('\\', '/').casefold() for x in listing.splitlines()[1:]}:
            raise RuntimeError('Own lab VM is already running')
        result['source_pins'] = seed(lab, profile, run, scenario, phase)
        text = vmx_text(lab, run)
        vmx.write_text(text)
        (run / 'requested.vmx').write_text(text)
        result['seed_sha256'] = digest(run / 'seed.iso')
        argv = [str(vmrun), '-T', 'ws', 'start', VM.to_host_path(vmx), 'nogui']
        result['command'] = argv
        started = True
        command(argv, run / 'vmrun-start.log', 45)
        deadline = start + window
        while time.monotonic() < deadline:
            raw = VM.read_serial(run / 'serial.log')
            if len(raw) > 8 * 1024 * 1024:
                raise RuntimeError('Serial capture exceeds lab bound')
            if b'LAB-BOOT-FAIL' in raw or re.search(rb'(?:Kernel panic|Traceback \(most recent)', raw):
                raise RuntimeError('Guest bootstrap/runtime failure')
            cuts = markers(raw, 'LAB-CUT')
            results = markers(raw, 'LAB-RESULT')
            if expect_cut and cuts:
                if len(cuts) != 1 or cuts[0].get('label') != expect_cut or results:
                    raise RuntimeError('Unexpected crash-cut result')
                result['observed_cut'] = cuts[0]
                result['status'] = 'CUT_OBSERVED'
                break
            if results:
                if expect_cut or len(results) != 1 or results[0].get('scenario') != scenario or results[0].get('ok') is not True:
                    raise RuntimeError('Guest scenario reported failure/unexpected result')
                result['guest_result'] = results[0]
                result['status'] = 'PASS'
                break
            time.sleep(.1)
        else:
            raise RuntimeError('Guest scenario timed out; retained serial is authoritative')
    except Exception as error:
        result['errors'].append(str(error))
        result['status'] = 'FAIL'
    finally:
        if started:
            try:
                VM.stopped_vm(vmrun, vmx, run, 30)
                result['cleanup_confirmed'] = True
            except Exception as error:
                result['errors'].append('cleanup: ' + str(error))
                result['status'] = 'FAIL'
        result['elapsed_seconds'] = time.monotonic() - start
        raw = VM.read_serial(run / 'serial.log')
        result['serial_sha256'] = hashlib.sha256(raw).hexdigest()
        try:
            result['guest_backend'] = markers(raw, 'LAB-GUEST')
            expected_bootstrap = {name: hashlib.sha256(text.encode()).hexdigest()
                                  for name, text in {'launcher': BOOT_LAUNCHER,
                                                     'service': SERVICE, 'bootstrap': BOOT_SCRIPT}.items()}
            if (len(result['guest_backend']) != 1 or
                    result['guest_backend'][0].get('data_uuid') != profile['data_uuid'] or
                    result['guest_backend'][0].get('bootstrap_hashes') != expected_bootstrap):
                raise RuntimeError('Actual guest bootstrap/profile differs from pinned run')
        except (ValueError, RuntimeError) as error:
            result['errors'].append('backend marker: ' + str(error))
            result['status'] = 'FAIL'
        if result.get('source_pins') != source_pins():
            result['errors'].append('Lab source changed during capture')
            result['status'] = 'FAIL'
        log = lab / 'vmware.log'
        if log.exists():
            shutil.copyfile(log, run / 'vmware.log')
        if result['cleanup_confirmed']:
            try:
                result['stopped_artifact_sha256'] = {
                    name: digest(lab / name)
                    for name in ('boot.vmdk', 'data.vmdk', 'lab.vmx', 'profile.json', 'ownership.json')}
                result['requested_vmx_sha256'] = digest(run / 'requested.vmx')
                result['copied_payload_sha256'] = {
                    name: digest(run / 'seed' / 'payload' / name)
                    for name in ('auth.py', 'runtime.py', 'scenarios.py')}
                if any(result['copied_payload_sha256'][name] != result['source_pins'][name]
                       for name in result['copied_payload_sha256']):
                    raise RuntimeError('Retained payload differs from source pins')
            except (OSError, KeyError, RuntimeError) as error:
                result['errors'].append('stopped provenance: ' + str(error))
                result['status'] = 'FAIL'
        save(run / 'result.json', result)
    print(json.dumps({'run': str(run), 'status': result['status'], 'errors': result['errors']}), flush=True)
    if result['status'] == 'FAIL':
        raise GuestRunFailure(run, result)
    return run, result


def matrix(args):
    pins = source_pins()
    manifest = json.loads(subprocess.check_output([sys.executable, str(SOURCE / 'scenarios.py'), '--list-json'], text=True))
    if source_pins() != pins:
        raise RuntimeError('Scenario sources changed while generating manifest')
    rows = []
    lab, _ = checked_lab(args.lab)
    output = lab / ('matrix-' + uuid.uuid4().hex + '.json')
    summary = {'scope': 'Private Linux/SQLite/Ed25519 VMware runtime only; not native Cellos/DCO1 or physical persistence qualification', 'source_pins': pins, 'manifest': manifest, 'runs': rows, 'status': 'FAIL'}
    save(output, summary)

    def record(name, phase, cut=None):
        if source_pins() != pins:
            raise RuntimeError('Source changed before matrix boot')
        try:
            run, result = run_guest(lab, name, phase, cut, args.window)
        except GuestRunFailure as error:
            rows.append({'path': str(error.run), 'result': error.result})
            save(output, summary)
            raise
        rows.append({'path': str(run), 'result': result})
        save(output, summary)
        if result.get('source_pins') != pins:
            raise RuntimeError('Run payload differs from matrix source pins')

    try:
        for name in manifest['normal']:
            record(name, 'run')
        for case in manifest['crash']:
            record(case['name'], 'prepare', case['cut'])
            record(case['name'], 'recover')
        if source_pins() != pins:
            raise RuntimeError('Source changed between matrix boots')
        summary['status'] = 'PASS'
    except Exception as error:
        summary['error'] = str(error)
        raise
    finally:
        save(output, summary)
        print('MATRIX ' + str(output), flush=True)


def main():
    def interrupted(signum, _frame):
        raise RuntimeError('Lab controller interrupted by signal ' + str(signum))

    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    p = sub.add_parser('provision')
    p.add_argument('--image', required=True)
    p.add_argument('--workspace-root', default='/mnt/c/Temp/cellos-vmware')
    p = sub.add_parser('run')
    p.add_argument('--lab', required=True)
    p.add_argument('--scenario', required=True)
    p.add_argument('--phase', choices=('run', 'prepare', 'recover'), default='run')
    p.add_argument('--expect-cut')
    p.add_argument('--window', type=int, default=300)
    p = sub.add_parser('matrix')
    p.add_argument('--lab', required=True)
    p.add_argument('--window', type=int, default=300)
    args = parser.parse_args()
    if getattr(args, 'window', 1) <= 0:
        parser.error('window must be positive')
    if args.action == 'provision':
        provision(args)
    else:
        lab, _ = checked_lab(args.lab)
        with (lab / 'controller.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            if args.action == 'run':
                run_guest(lab, args.scenario, args.phase, args.expect_cut, args.window)
            else:
                matrix(args)


if __name__ == '__main__':
    main()
