#!/usr/bin/env python3
"""Exercise the real native coordinator/worker path on isolated x86 QEMU.

This is a correctness smoke, not a hardware performance benchmark. CPU evidence
is checked kernel admission plus hardware render instruction endpoints, not
all-core CPU time or simultaneous overlap. Keeps serial evidence in target/,
and searches only output produced after each command.
"""
import argparse
import csv
from pathlib import Path
import re
import socket
import subprocess
import time


DOMAIN_ADMISSION = (
    "[domain] admitted cell 'c2c-render-domain-worker' to Tier 2 Paged Domain (CR3 isolation)"
)
SHARED_REFUSAL = 'c2c-render: TIER2 SHARED OUTPUT REFUSED'


def parse_processor_ids(value):
    if value == '-':
        return set()
    if re.fullmatch(r'(0|[1-9]\d*)(;(0|[1-9]\d*))?', value) is None:
        raise RuntimeError('malformed processor endpoint IDs')
    ids = [int(part) for part in value.split(';')]
    if len(set(ids)) != len(ids) or ids != sorted(ids) or any(i > 0xffffffff for i in ids):
        raise RuntimeError('duplicate, unordered or out-of-range processor endpoint IDs')
    return set(ids)


def check_cpu_topology(output, cpus):
    text = output.decode(errors='replace')
    marker = '[x86-smp] topology '
    # Kernel log::info adds its UART level prefix; retain the stable payload.
    topology = [line[line.index(marker):] for line in text.splitlines() if marker in line]
    if len(topology) != 1:
        raise RuntimeError('missing or duplicate actual kernel CPU topology evidence')
    record = re.fullmatch(
        r'\[x86-smp\] topology online=(1|2) processor_ids=(\d+(?:;\d+)?) qualified=yes',
        topology[0],
    )
    if record is None:
        raise RuntimeError('kernel CPU topology is not qualified')
    online, value = record.groups()
    # Kernel logical ordering need not be physical-ID sorted.
    ids = [int(part) for part in value.split(';')]
    if (int(online) != cpus or len(ids) != cpus or len(set(ids)) != cpus
            or any(i > 0xffffffff for i in ids)):
        raise RuntimeError('actual admitted kernel CPU topology differs from --cpus')
    return set(ids)


def check_render_output(output, width, height, samples, depth, tile_size, workers, modes,
                        cpu_ids, require_multicore=True):
    text = output.decode(errors='replace')
    expected_modes = set(modes)
    tile_count = ((width + tile_size - 1) // tile_size
                  * ((height + tile_size - 1) // tile_size))
    effective_workers = min(workers, tile_count)
    domain_workers = effective_workers if 'tier2' in expected_modes else 0
    if text.count(DOMAIN_ADMISSION) != domain_workers:
        raise RuntimeError(f'expected {domain_workers} exact Tier 2 worker CR3 admissions')
    refusal_count = domain_workers if 'baseline' in expected_modes else 0
    if text.count(SHARED_REFUSAL) != refusal_count:
        raise RuntimeError(f'expected {refusal_count} Tier 2 shared-output refusals')
    header = None
    rows = {}
    records = {mode: {} for mode in expected_modes if mode != 'baseline'}
    lines = [line.removeprefix('USER: ') for line in text.splitlines()]
    for line in lines:
        if not line.startswith('c2c-render: worker '):
            continue
        record = re.fullmatch(
            r'c2c-render: worker mode=(copy|shared|tier2) index=(\d+) tid=(\d+) tiles=(\d+)'
            r' processor_ids=(\S+)',
            line,
        )
        if record is None:
            raise RuntimeError('malformed render worker record')
        mode, index, tid, tiles, value = record.groups()
        index, tid, tiles = int(index), int(tid), int(tiles)
        if mode not in records or index in records[mode]:
            raise RuntimeError('unexpected or duplicate render worker record')
        if tid <= 0 or tiles <= 0:
            raise RuntimeError('render worker record lacks a live TID or completed tiles')
        observed = parse_processor_ids(value)
        if not observed or not observed <= cpu_ids:
            raise RuntimeError('worker render processor endpoints are missing or not admitted')
        records[mode][index] = (tid, tiles, observed)
    for fields in csv.reader(lines):
        if fields[:2] == ['mode', 'scene_hash']:
            if header is not None or len(set(fields)) != len(fields):
                raise RuntimeError('duplicate render CSV header or field')
            header = fields
        elif fields and fields[0] in {'baseline', 'copy', 'shared', 'tier2'}:
            if (fields[0] not in expected_modes or header is None
                    or len(fields) != len(header) or fields[0] in rows):
                raise RuntimeError('malformed, unexpected or duplicate render CSV row')
            rows[fields[0]] = dict(zip(header, fields))
    if rows.keys() != expected_modes:
        raise RuntimeError(f'missing render CSV rows: expected {sorted(expected_modes)}')
    expected_rgb_bytes = width * height * 3
    expected_config = {
        'width': width, 'height': height, 'samples_per_pixel': samples,
        'max_bounces': depth, 'tile_size': tile_size, 'RGB_bytes': expected_rgb_bytes,
        'tiles': tile_count, 'total_samples': width * height * samples,
        'workers_requested': workers,
    }
    try:
        for mode, row in rows.items():
            for field, expected in expected_config.items():
                if int(row[field]) != expected:
                    raise RuntimeError(f'{mode} CSV {field} differs from requested render')
            expected_workers = 0 if mode == 'baseline' else effective_workers
            for field in ('workers_started', 'peak_outstanding'):
                if int(row[field]) != expected_workers:
                    raise RuntimeError(f'{mode} CSV {field} differs from effective worker count')
            copied_bytes = expected_rgb_bytes if mode in {'copy', 'tier2'} else 0
            if int(row['pixel_payload_bytes']) != copied_bytes:
                raise RuntimeError(f'{mode} CSV copied pixel payload differs from expected bytes')
            observed = parse_processor_ids(row['processor_ids'])
            if not observed or not observed <= cpu_ids:
                raise RuntimeError(f'{mode} CSV processor endpoints are missing or not admitted')
            if mode != 'baseline':
                worker_records = records[mode]
                if worker_records.keys() != set(range(effective_workers)):
                    raise RuntimeError(f'{mode} worker indices differ from effective pool')
                if len({tid for tid, _, _ in worker_records.values()}) != effective_workers:
                    raise RuntimeError(f'{mode} workers do not have distinct live TIDs')
                if sum(tiles for _, tiles, _ in worker_records.values()) != tile_count:
                    raise RuntimeError(f'{mode} worker completed tiles do not sum to total tiles')
                worker_ids = set().union(*(ids for _, _, ids in worker_records.values()))
                if observed != worker_ids:
                    raise RuntimeError(f'{mode} CSV endpoints differ from worker render records')
                # The main workload supplies per-mode multicore evidence.
                # Tiny tail/recovery waves check ownership and output; their
                # tasks can legitimately finish before a peer schedules them.
                if (require_multicore and len(cpu_ids) == 2
                        and effective_workers >= 2 and worker_ids != cpu_ids):
                    raise RuntimeError(f'{mode} lacks render endpoints on both admitted CPUs')
        if 'baseline' in rows:
            for mode, row in rows.items():
                for field in ('scene_hash', 'seed', 'timer_hz', 'width', 'height',
                              'samples_per_pixel', 'max_bounces', 'tile_size', 'tiles',
                              'rays', 'total_samples', 'RGB_bytes', 'pixel_hash'):
                    if rows['baseline'][field] != row[field]:
                        raise RuntimeError(f'baseline/{mode} CSV mismatch in {field}')
    except (KeyError, ValueError) as error:
        raise RuntimeError(f'malformed render CSV metrics: {error}') from error


def check_saved_output(output, path, width, height):
    saved = f'c2c-render: saved {path} bytes={width * height * 3} save_ns='.encode()
    if saved not in output:
        raise RuntimeError('missing completed PPM save evidence')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--iso', default='build/vicell-x86-c2c-render.iso')
    parser.add_argument('--log', default='target/c2c-render-smoke.log')
    parser.add_argument('--mode', choices=('compare', 'compare-tier2'), default='compare',
                        help='Compare native local workers or the copied Tier 2 domain worker')
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--width', type=int, default=32)
    parser.add_argument('--height', type=int, default=24)
    parser.add_argument('--samples', type=int, default=2)
    parser.add_argument('--depth', type=int, default=4)
    parser.add_argument('--tile-size', type=int, default=8)
    parser.add_argument('--workers', type=int, choices=range(1, 5), default=1,
                        help='Number of independent worker Cells (1..4; not CPU parallelism)')
    parser.add_argument('--cpus', type=int, choices=(1, 2), default=1,
                        help='QEMU CPUs; require matching qualified kernel topology and render endpoints')
    parser.add_argument('--accel', choices=('tcg', 'kvm'), default='tcg',
                        help='QEMU accelerator; KVM uses the host CPU model by default')
    parser.add_argument('--cpu-model',
                        help='Explicit QEMU CPU model (default: host for KVM, qemu64,+pdpe1gb for TCG)')
    parser.add_argument('--exercise-expiry', action='store_true',
                        help='Exercise kernel operation expiry, mode-specific teardown, and recovery')
    args = parser.parse_args()
    cpu_model = args.cpu_model or ('host' if args.accel == 'kvm' else 'qemu64,+pdpe1gb')
    tier2 = args.mode == 'compare-tier2'
    worker_mode = 'tier2' if tier2 else 'shared'
    success_marker = ('c2c-render: COMPARE OK baseline=tier2' if tier2
                      else 'c2c-render: COMPARE OK baseline=copy=shared')
    repo = Path(__file__).resolve().parents[2]
    iso = repo / args.iso
    if not iso.is_file():
        parser.error(f'missing image: {iso}; run bash scripts/build-x86_64-c2c-render-ci.sh')
    log = repo / args.log
    log.parent.mkdir(parents=True, exist_ok=True)
    serial_log = bytearray()
    process = None
    connection = None
    with socket.socket() as server:
        server.bind(('127.0.0.1', 0))
        server.listen(1)
        server.settimeout(30)
        command = [
            'qemu-system-x86_64', '-machine', 'q35', '-accel', args.accel, '-cpu', cpu_model,
            '-smp', str(args.cpus),
            '-m', '256M', '-nographic', '-cdrom', str(iso), '-boot', 'd',
            '-no-reboot', '-monitor', 'none',
            '-serial', f'tcp:127.0.0.1:{server.getsockname()[1]}',
        ]
        try:
            process = subprocess.Popen(command, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            connection, _ = server.accept()
            connection.settimeout(0.5)

            def until(marker, start=0, timeout=None, allow_error=False):
                deadline = time.monotonic() + (args.timeout if timeout is None else timeout)
                target = marker.encode()
                while time.monotonic() < deadline:
                    current = serial_log[start:]
                    if target in current:
                        return bytes(current)
                    if not allow_error and b'c2c-render: ERROR' in current and target != b'c2c-render: ERROR':
                        raise RuntimeError(current.decode(errors='replace'))
                    if process.poll() is not None:
                        raise RuntimeError(f'QEMU exited {process.returncode}')
                    try:
                        chunk = connection.recv(65536)
                        if not chunk:
                            raise RuntimeError('QEMU serial disconnected')
                        serial_log.extend(chunk)
                    except socket.timeout:
                        pass
                raise TimeoutError(f'timed out waiting for {marker!r}\n'
                                   + serial_log[start:].decode(errors='replace'))

            def send(command):
                start = len(serial_log)
                connection.sendall(command.encode() + b'\n')
                return start

            until('Cellos >', timeout=60)
            cpu_ids = check_cpu_topology(bytes(serial_log), args.cpus)
            start = send(f'c2c-render {args.mode} --width {args.width} --height {args.height} '
                         f'--samples {args.samples} --depth {args.depth} '
                         f'--tile-size {args.tile_size} --workers {args.workers} '
                         '--output /tmp/c2c-render.ppm')
            until(success_marker, start)
            output = until('Cellos >', start)
            print(output.decode(errors='replace'))
            modes = ('baseline', 'tier2') if tier2 else ('baseline', 'copy', 'shared')
            check_render_output(output, args.width, args.height, args.samples,
                                args.depth, args.tile_size, args.workers, modes, cpu_ids)
            # The success marker follows checked VFS writes and final file Stat.
            # Shell vcat is UTF-8-only and cannot validate a binary PPM.
            check_saved_output(output, '/tmp/c2c-render.ppm', args.width, args.height)
            if args.workers > 1:
                # Retain boundary coverage in the standalone pool smoke: one
                # partial tail wave and fewer tiles than requested workers.
                for label, width, height in (
                        ('tail', 8 * args.workers + 1, 7), ('clamp', 8, 8)):
                    path = f'/tmp/c2c-pool-{label}.ppm'
                    start = send(f'c2c-render {args.mode} --width {width} --height {height} '
                                 f'--samples 1 --depth 2 --tile-size 8 --workers {args.workers} '
                                 f'--output {path}')
                    until(success_marker, start)
                    boundary = until('Cellos >', start)
                    check_render_output(boundary, width, height, 1, 2, 8,
                                        args.workers, modes, cpu_ids, require_multicore=False)
                    check_saved_output(boundary, path, width, height)
                    print(f'C2C_RENDER_POOL_{label.upper()}=PASS')
            start = send(f'c2c-render {worker_mode} --width 0')
            until('c2c-render: ERROR', start)
            until('Cellos >', start, allow_error=True)
            if args.exercise_expiry:
                expiry_width = 18 * args.workers
                start = send(f'c2c-render {worker_mode} --width {expiry_width} --height 18 '
                             f'--samples 65536 --depth 64 --tile-size 18 --workers {args.workers} '
                             '--output /tmp/c2c-expired.ppm')
                until('operation terminal: Indeterminate', start, timeout=90, allow_error=True)
                expired = until('Cellos >', start, timeout=30, allow_error=True)
                if b'c2c-render: RENDER OK' in expired or b'c2c-render: saved ' in expired:
                    raise RuntimeError('expired pool reported successful render or saved image')
                expired_text = expired.decode(errors='replace')
                victims = re.findall(r'\[kernel\] ForceExit: task (\d+) killed by task \d+',
                                     expired_text)
                if len(victims) != args.workers or len(set(victims)) != args.workers:
                    raise RuntimeError('expiry did not ForceExit every distinct pool worker')
                admissions = args.workers if tier2 else 0
                if expired_text.count(DOMAIN_ADMISSION) != admissions:
                    raise RuntimeError(f'expiry expected {admissions} exact Tier 2 admissions')
                print(expired_text)
                recovery_width = 8 * args.workers
                start = send(f'c2c-render {worker_mode} --width {recovery_width} --height 8 '
                             f'--samples 1 --depth 2 --tile-size 8 --workers {args.workers} '
                             '--output /tmp/c2c-after-expiry.ppm')
                until('c2c-render: RENDER OK', start)
                recovered = until('Cellos >', start)
                check_render_output(recovered, recovery_width, 8, 1, 2, 8,
                                    args.workers, (worker_mode,), cpu_ids,
                                    require_multicore=False)
                check_saved_output(recovered, '/tmp/c2c-after-expiry.ppm', recovery_width, 8)
                teardown = 'copied/private-root teardown' if tier2 else 'shared-buffer teardown'
                print(f'C2C_RENDER_EXPIRY=PASS (indeterminate operation, {teardown}, '
                      f'successful {worker_mode} recovery)')
            evidence = ('baseline/tier2 CSV equality, copied pixels, CR3 admission, shared refusal'
                        if tier2 else 'native baseline/copy/shared equality')
            processor_evidence = ';'.join(str(i) for i in sorted(cpu_ids))
            topology = (f'actual qualified online CPUs={len(cpu_ids)} '
                        f'processor_ids={processor_evidence}')
            endpoints = ('per-worker-mode render endpoints on both admitted CPUs'
                         if len(cpu_ids) == 2 and min(
                             args.workers,
                             ((args.width + args.tile_size - 1) // args.tile_size
                              * ((args.height + args.tile_size - 1) // args.tile_size)),
                         ) >= 2 else 'valid admitted render processor endpoints')
            print(f'C2C_RENDER_SMOKE=PASS ({evidence}, {topology}, {endpoints}, '
                  f'not simultaneous-overlap proof, PPM, invalid input)\nLOG={log}')
        finally:
            if connection is not None:
                connection.close()
            if process is not None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                if process.stderr is not None:
                    error = process.stderr.read()
                    if error:
                        print(error.decode(errors='replace'))
                    process.stderr.close()
            log.write_bytes(serial_log)


if __name__ == '__main__':
    main()
